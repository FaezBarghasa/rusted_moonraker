use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch, broadcast};
use serde_json::Value;

pub mod middleware;
pub mod handlers;
pub mod ws;

pub struct TempHistoryPoint {
    pub time: u64,
    pub tool_temp: f64,
    pub target_temp: f64,
}

pub struct SystemState {
    pub config: crate::config::MoonrakerConfig,
    pub db: crate::db::DatabaseManager,
    pub file_manager: crate::files::FileManager,
    pub klippy_tx: mpsc::Sender<crate::klippy::KlippyCommand>,
    pub state_rx: watch::Receiver<crate::klippy::KlipperStateTree>,
    pub ws_broadcast_tx: broadcast::Sender<Value>,
    pub temp_history: Mutex<Vec<TempHistoryPoint>>,
}

pub fn start_telemetry_loop(state: Arc<SystemState>) {
    let rx = state.state_rx.clone();
    let tx = state.ws_broadcast_tx.clone();
    let state_clone = state.clone();
    
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            
            let current = rx.borrow().clone();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            
            {
                let mut history = state_clone.temp_history.lock().unwrap();
                history.push(TempHistoryPoint {
                    time: now,
                    tool_temp: current.tool_temp,
                    target_temp: current.target_temp,
                });
                if history.len() > 3600 {
                    history.remove(0);
                }
            }
            
            let update_payload = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notify_status_update",
                "params": [{
                    "extruder": {
                        "temperature": current.tool_temp,
                        "target": current.target_temp
                    },
                    "toolhead": {
                        "position": [current.x, current.y, current.z, current.e]
                    },
                    "print_stats": {
                        "state": current.print_status.to_lowercase()
                    }
                }]
            });
            let _ = tx.send(update_payload);
        }
    });
}

pub async fn start_web_server(state: Arc<SystemState>) -> Result<(), std::io::Error> {
    let host = state.config.server.host.clone();
    let port = state.config.server.port;
    
    let mut trusted_networks = Vec::new();
    for net_str in &state.config.server.trusted_clients {
        if let Ok(net) = net_str.parse::<ipnetwork::IpNetwork>() {
            trusted_networks.push(net);
        }
    }
    
    let api_key = state.config.server.api_key.clone();
    let state_data = actix_web::web::Data::new(state.clone());
    
    let server = actix_web::HttpServer::new(move || {
        actix_web::App::new()
            .app_data(state_data.clone())
            .wrap(middleware::AuthorizationMiddleware::new(
                trusted_networks.clone(),
                api_key.clone(),
            ))
            .service(handlers::printer_info)
            .service(handlers::server_config)
            .service(handlers::temperature_store)
            .service(crate::files::upload_file)
            .service(crate::files::delete_file)
            .service(crate::files::list_files)
            .route("/websocket", actix_web::web::get().to(ws::ws_handler))
    });
    
    let addr = format!("{}:{}", host, port);
    println!("Web server binding to {}", addr);
    let server = server.bind(addr)?;
    server.run().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};
    use crate::config::{MoonrakerConfig, ServerConfig, KlippyConfig, DatabaseConfig};
    use crate::db::DatabaseManager;
    use serde_json::json;

    async fn setup_test_state() -> Arc<SystemState> {
        let config = MoonrakerConfig {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                ssl_enabled: false,
                max_upload_size_mb: 10,
                trusted_clients: vec!["127.0.0.1/32".to_string(), "10.0.0.0/8".to_string()],
                api_key: Some("test-secret-key".to_string()),
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
    async fn test_web_routes_and_security() {
        let state = setup_test_state().await;
        let state_data = actix_web::web::Data::new(state.clone());

        let app = test::init_service(
            App::new()
                .app_data(state_data)
                .wrap(middleware::AuthorizationMiddleware::new(
                    vec!["127.0.0.1/32".parse().unwrap()],
                    Some("test-secret-key".to_string()),
                ))
                .service(handlers::printer_info)
                .service(handlers::server_config)
        )
        .await;

        let req = test::TestRequest::get()
            .uri("/printer/info")
            .insert_header(("X-Forwarded-For", "127.0.0.1"))
            .to_request();

        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());

        let req_untrusted = test::TestRequest::get()
            .uri("/printer/info")
            .insert_header(("X-Forwarded-For", "192.168.1.50"))
            .to_request();

        let resp_untrusted = test::call_service(&app, req_untrusted).await;
        assert_eq!(resp_untrusted.status(), actix_web::http::StatusCode::FORBIDDEN);

        let req_api_key = test::TestRequest::get()
            .uri("/printer/info")
            .insert_header(("X-Forwarded-For", "192.168.1.50"))
            .insert_header(("X-Api-Key", "test-secret-key"))
            .to_request();

        let resp_api_key = test::call_service(&app, req_api_key).await;
        assert!(resp_api_key.status().is_success());
    }

    #[tokio::test]
    async fn test_ws_router() {
        let state = setup_test_state().await;

        let payload = json!({
            "jsonrpc": "2.0",
            "method": "printer.info",
            "id": 42
        });

        let resp = ws::ws_router(&payload.to_string(), &state).await;
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 42);
        assert_eq!(resp["result"]["state"], "idle");
    }
}
