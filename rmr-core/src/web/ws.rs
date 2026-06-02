use actix::prelude::*;
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use std::sync::Arc;
use serde_json::Value;

use crate::web::SystemState;
use crate::db::{WebLayoutPreset, PrintRecord};

// Message definition for broadcasting status updates
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct BroadcastMessage(pub Value);

// The WebSocket Session Actor representing a connected Fluidd client
pub struct FluiddSession {
    pub state: Arc<SystemState>,
}

impl Actor for FluiddSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let addr = ctx.address();
        let mut rx = self.state.ws_broadcast_tx.subscribe();
        
        actix_web::rt::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(val) => {
                        if addr.send(BroadcastMessage(val)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {}
}

impl Handler<BroadcastMessage> for FluiddSession {
    type Result = ();

    fn handle(&mut self, msg: BroadcastMessage, ctx: &mut Self::Context) {
        ctx.text(msg.0.to_string());
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for FluiddSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(bytes)) => {
                ctx.pong(&bytes);
            }
            Ok(ws::Message::Pong(_)) => {}
            Ok(ws::Message::Text(text)) => {
                let state = self.state.clone();
                let fut = async move {
                    ws_router(&text, &state).await
                }
                .into_actor(self)
                .map(|res, _actor, ctx| {
                    ctx.text(res.to_string());
                });
                ctx.spawn(fut);
            }
            Ok(ws::Message::Binary(_)) => {}
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            }
            _ => {}
        }
    }
}

pub async fn ws_handler(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<Arc<SystemState>>,
) -> Result<HttpResponse, actix_web::Error> {
    ws::start(
        FluiddSession {
            state: state.get_ref().clone(),
        },
        &req,
        stream,
    )
}

#[derive(Debug, serde::Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    #[serde(default)]
    params: Value,
    id: Option<Value>,
}

pub async fn ws_router(
    raw_payload: &str,
    state: &SystemState,
) -> Value {
    let req: JsonRpcRequest = match serde_json::from_str(raw_payload) {
        Ok(r) => r,
        Err(e) => {
            return serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32700,
                    "message": format!("Parse error: {}", e)
                },
                "id": Value::Null
            });
        }
    };

    let id = req.id.unwrap_or(Value::Null);

    match req.method.as_str() {
        "printer.info" => {
            let kstate = state.state_rx.borrow().clone();
            serde_json::json!({
                "jsonrpc": "2.0",
                "result": {
                    "state": kstate.print_status.to_lowercase(),
                    "state_message": format!("Printer is in {} state", kstate.print_status),
                    "hostname": "RMR-daemon"
                },
                "id": id
            })
        }
        "printer.emergency_stop" => {
            let _ = state.klippy_tx.send(crate::klippy::KlippyCommand::EmergencyStop).await;
            serde_json::json!({
                "jsonrpc": "2.0",
                "result": "ok",
                "id": id
            })
        }
        "printer.gcode.script" => {
            let script = match req.params.get("script").and_then(|s| s.as_str()) {
                Some(s) => s,
                None => {
                    return serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32602,
                            "message": "Invalid params: missing script"
                        },
                        "id": id
                    });
                }
            };

            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
            let _ = state.klippy_tx.send(crate::klippy::KlippyCommand::JsonRpcRequest {
                method: "printer.gcode.script".to_string(),
                params: req.params.clone(),
                response_sender: resp_tx,
            }).await;

            match resp_rx.await {
                Ok(Ok(res)) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "result": res,
                        "id": id
                    })
                }
                Ok(Err(e)) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32603,
                            "message": format!("Internal error: {:?}", e)
                        },
                        "id": id
                    })
                }
                Err(_) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32603,
                            "message": "Internal error: response channel dropped"
                        },
                        "id": id
                    })
                }
            }
        }
        "server.info" => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "result": {
                    "api_version": "0.1.0",
                    "server_version": "0.1.0",
                    "cpu_info": "Rockchip RK3328",
                    "hostname": "RMR-daemon"
                },
                "id": id
            })
        }
        "server.config" => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "result": {
                    "server": {
                        "host": state.config.server.host.clone(),
                        "port": state.config.server.port,
                        "ssl_enabled": state.config.server.ssl_enabled,
                        "max_upload_size_mb": state.config.server.max_upload_size_mb
                    },
                    "klippy": {
                        "uds_path": state.config.klippy.uds_path.to_string_lossy().to_string(),
                        "api_timeout_secs": state.config.klippy.api_timeout_secs
                    }
                },
                "id": id
            })
        }
        "server.files.list" => {
            match state.db.inner.query("SELECT * FROM gcode_files;").await {
                Ok(mut res) => {
                    let files: Vec<Value> = res.take(0).unwrap_or_default();
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "result": files,
                        "id": id
                    })
                }
                Err(e) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32603,
                            "message": format!("DB error: {:?}", e)
                        },
                        "id": id
                    })
                }
            }
        }
        "server.files.metadata" => {
            let filename = match req.params.get("filename").and_then(|f| f.as_str()) {
                Some(f) => f,
                None => {
                    return serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32602,
                            "message": "Invalid params: missing filename"
                        },
                        "id": id
                    });
                }
            };

            match state.db.inner.query("SELECT * FROM gcode_files WHERE file_path = $path;").bind(("path", filename)).await {
                Ok(mut res) => {
                    let records: Vec<Value> = res.take(0).unwrap_or_default();
                    if let Some(record) = records.first() {
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "result": record.clone(),
                            "id": id
                        })
                    } else {
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "error": {
                                "code": -32603,
                                "message": "File not found"
                            },
                            "id": id
                        })
                    }
                }
                Err(e) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32603,
                            "message": format!("DB error: {:?}", e)
                        },
                        "id": id
                    })
                }
            }
        }
        "server.history.list" => {
            match state.db.get_print_records().await {
                Ok(records) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "result": records,
                        "id": id
                    })
                }
                Err(e) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32603,
                            "message": format!("DB error: {:?}", e)
                        },
                        "id": id
                    })
                }
            }
        }
        "server.database.get_item" => {
            let namespace = match req.params.get("namespace").and_then(|n| n.as_str()) {
                Some(n) => n,
                None => {
                    return serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32602,
                            "message": "Invalid params: missing namespace"
                        },
                        "id": id
                    });
                }
            };

            let key_opt = req.params.get("key").and_then(|k| {
                if k.is_string() {
                    k.as_str().map(|s| s.to_string())
                } else if k.is_array() {
                    let arr: Vec<String> = k.as_array().unwrap().iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                    Some(arr.join("."))
                } else {
                    None
                }
            });

            if let Some(key) = key_opt {
                match state.db.get_web_layout_preset(&key).await {
                    Ok(Some(preset)) => {
                        let value_parsed: Value = serde_json::from_str(&preset.layout_data).unwrap_or(Value::String(preset.layout_data));
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "result": {
                                "namespace": namespace,
                                "key": key,
                                "value": value_parsed
                            },
                            "id": id
                        })
                    }
                    Ok(None) => {
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "error": {
                                "code": -32603,
                                "message": "Item not found"
                            },
                            "id": id
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "error": {
                                "code": -32603,
                                "message": format!("DB error: {:?}", e)
                            },
                            "id": id
                        })
                    }
                }
            } else {
                match state.db.get_web_layout_presets().await {
                    Ok(presets) => {
                        let mut map = serde_json::Map::new();
                        for p in presets {
                            let value_parsed: Value = serde_json::from_str(&p.layout_data).unwrap_or(Value::String(p.layout_data));
                            map.insert(p.name, value_parsed);
                        }
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "result": {
                                "namespace": namespace,
                                "value": Value::Object(map)
                            },
                            "id": id
                        })
                    }
                    Err(e) => {
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "error": {
                                "code": -32603,
                                "message": format!("DB error: {:?}", e)
                            },
                            "id": id
                        })
                    }
                }
            }
        }
        "server.database.post_item" => {
            let namespace = match req.params.get("namespace").and_then(|n| n.as_str()) {
                Some(n) => n,
                None => {
                    return serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32602,
                            "message": "Invalid params: missing namespace"
                        },
                        "id": id
                    });
                }
            };

            let key = match req.params.get("key") {
                Some(k) => {
                    if k.is_string() {
                        k.as_str().map(|s| s.to_string())
                    } else if k.is_array() {
                        let arr: Vec<String> = k.as_array().unwrap().iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                        Some(arr.join("."))
                    } else {
                        None
                    }
                }
                None => None,
            };

            let key = match key {
                Some(k) => k,
                None => {
                    return serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32602,
                            "message": "Invalid params: key must be string or array of strings"
                        },
                        "id": id
                    });
                }
            };

            let value = match req.params.get("value") {
                Some(v) => v.clone(),
                None => {
                    return serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32602,
                            "message": "Invalid params: missing value"
                        },
                        "id": id
                    });
                }
            };

            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            let preset = WebLayoutPreset {
                name: key.clone(),
                layout_data: value.to_string(),
                timestamp,
            };

            match state.db.save_web_layout_preset(&preset).await {
                Ok(_) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "result": {
                            "namespace": namespace,
                            "key": key,
                            "value": value
                        },
                        "id": id
                    })
                }
                Err(e) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32603,
                            "message": format!("DB error: {:?}", e)
                        },
                        "id": id
                    })
                }
            }
        }
        "server.database.delete_item" => {
            let namespace = match req.params.get("namespace").and_then(|n| n.as_str()) {
                Some(n) => n,
                None => {
                    return serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32602,
                            "message": "Invalid params: missing namespace"
                        },
                        "id": id
                    });
                }
            };

            let key = match req.params.get("key") {
                Some(k) => {
                    if k.is_string() {
                        k.as_str().map(|s| s.to_string())
                    } else if k.is_array() {
                        let arr: Vec<String> = k.as_array().unwrap().iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                        Some(arr.join("."))
                    } else {
                        None
                    }
                }
                None => None,
            };

            let key = match key {
                Some(k) => k,
                None => {
                    return serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32602,
                            "message": "Invalid params: key must be string or array of strings"
                        },
                        "id": id
                    });
                }
            };

            match state.db.delete_web_layout_preset(&key).await {
                Ok(_) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "result": {
                            "namespace": namespace,
                            "key": key
                        },
                        "id": id
                    })
                }
                Err(e) => {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "error": {
                            "code": -32603,
                            "message": format!("DB error: {:?}", e)
                        },
                        "id": id
                    })
                }
            }
        }
        _ => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {}", req.method)
                },
                "id": id
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};
    use crate::config::{MoonrakerConfig, ServerConfig, KlippyConfig, DatabaseConfig};
    use crate::db::DatabaseManager;
    use tokio::sync::{mpsc, watch, broadcast};
    use std::sync::Mutex;

    async fn setup_test_state() -> Arc<SystemState> {
        let config = MoonrakerConfig {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                ssl_enabled: false,
                max_upload_size_mb: 10,
                trusted_clients: vec!["127.0.0.1/32".to_string()],
                api_key: None,
            },
            klippy: KlippyConfig {
                uds_path: "/tmp/nonexistent.sock".into(),
                api_timeout_secs: 5,
            },
            database: DatabaseConfig {
                db_path: "".into(),
            },
        };

        let db = DatabaseManager::initialize_mem().await.unwrap();
        let file_manager = crate::files::FileManager::new(
            std::path::PathBuf::from("/tmp/rmr_test_gcodes"),
            db.clone(),
            config.server.max_upload_size_mb,
        );
        let (klippy_tx, _klippy_rx) = mpsc::channel(10);
        let (_state_tx, state_rx) = watch::channel(crate::klippy::KlipperStateTree::default());
        let (ws_broadcast_tx, _ws_broadcast_rx) = broadcast::channel(100);

        Arc::new(SystemState {
            config,
            db,
            file_manager,
            klippy_tx,
            state_rx,
            ws_broadcast_tx,
            temp_history: Mutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn test_websocket_connection_and_router() {
        let state = setup_test_state().await;
        let state_data = actix_web::web::Data::new(state.clone());

        let app = test::init_service(
            App::new()
                .app_data(state_data)
                .route("/websocket", actix_web::web::get().to(ws_handler))
        )
        .await;

        // Test the router directly for multiple methods
        let payload_info = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "printer.info",
            "id": 1
        });
        let resp_info = ws_router(&payload_info.to_string(), &state).await;
        assert_eq!(resp_info["result"]["state"], "idle");
        assert_eq!(resp_info["id"], 1);

        let payload_info_server = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "server.info",
            "id": 2
        });
        let resp_info_server = ws_router(&payload_info_server.to_string(), &state).await;
        assert_eq!(resp_info_server["result"]["cpu_info"], "Rockchip RK3328");
        assert_eq!(resp_info_server["id"], 2);

        // Test database post/get
        let payload_db_post = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "server.database.post_item",
            "params": {
                "namespace": "fluidd",
                "key": "presets.test_preset",
                "value": {"temp": 220}
            },
            "id": 3
        });
        let resp_db_post = ws_router(&payload_db_post.to_string(), &state).await;
        assert_eq!(resp_db_post["result"]["value"]["temp"], 220);

        let payload_db_get = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "server.database.get_item",
            "params": {
                "namespace": "fluidd",
                "key": "presets.test_preset"
            },
            "id": 4
        });
        let resp_db_get = ws_router(&payload_db_get.to_string(), &state).await;
        assert_eq!(resp_db_get["result"]["value"]["temp"], 220);
    }
}
