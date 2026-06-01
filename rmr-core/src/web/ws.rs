use actix_web::{web, HttpRequest, HttpResponse};
use futures_util::StreamExt;
use std::sync::Arc;
use serde_json::Value;

use crate::web::SystemState;

pub async fn ws_handler(
    req: HttpRequest,
    stream: web::Payload,
    state: web::Data<Arc<SystemState>>,
) -> Result<HttpResponse, actix_web::Error> {
    let (res, mut session, mut msg_stream) = actix_ws::handle(&req, stream)?;
    let state_clone = state.into_inner();

    actix_web::rt::spawn(async move {
        let mut broadcast_rx = state_clone.ws_broadcast_tx.subscribe();
        
        loop {
            tokio::select! {
                val_res = broadcast_rx.recv() => {
                    match val_res {
                        Ok(val) => {
                            let text = val.to_string();
                            if session.text(text).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
                msg_opt = msg_stream.next() => {
                    match msg_opt {
                        Some(Ok(actix_ws::Message::Text(text))) => {
                            let response = ws_router(&text, &state_clone).await;
                            let text_resp = response.to_string();
                            if session.text(text_resp).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(actix_ws::Message::Ping(bytes))) => {
                            if session.pong(&bytes).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(actix_ws::Message::Close(reason))) => {
                            let _ = session.close(reason).await;
                            break;
                        }
                        Some(Err(_)) | None => {
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    Ok(res)
}

pub async fn ws_router(
    raw_payload: &str,
    state: &SystemState,
) -> Value {
    let val: Value = match serde_json::from_str(raw_payload) {
        Ok(v) => v,
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

    let id = val.get("id").cloned().unwrap_or(Value::Null);
    let method = match val.get("method").and_then(|m| m.as_str()) {
        Some(m) => m,
        None => {
            return serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32600,
                    "message": "Invalid Request: missing method"
                },
                "id": id
            });
        }
    };

    let params = val.get("params").cloned().unwrap_or(Value::Null);

    match method {
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
            if params.get("script").and_then(|s| s.as_str()).is_some() {
                let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                let _ = state.klippy_tx.send(crate::klippy::KlippyCommand::JsonRpcRequest {
                    method: "printer.gcode.script".to_string(),
                    params: params.clone(),
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
            } else {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32602,
                        "message": "Invalid params: missing script"
                    },
                    "id": id
                })
            }
        }
        _ => {
            serde_json::json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {}", method)
                },
                "id": id
            })
        }
    }
}
