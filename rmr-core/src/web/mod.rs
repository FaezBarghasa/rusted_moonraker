pub mod handlers;
pub mod ws;

use actix_web::{web, App, HttpServer, middleware};
use actix_cors::Cors;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashSet;
use crate::db::Database;

pub struct AppState {
    pub db: Database,
    pub ws_clients: Arc<RwLock<HashSet<ws::SessionTx>>>,
}

pub async fn start_server(db: Database) -> std::io::Result<()> {
    let ws_clients = Arc::new(RwLock::new(HashSet::new()));
    let app_state = web::Data::new(AppState {
        db,
        ws_clients: ws_clients.clone(),
    });

    // Start background broadcaster
    tokio::spawn(ws::broadcast_status(ws_clients));

    HttpServer::new(move || {
        let cors = Cors::permissive();

        App::new()
            .app_data(app_state.clone())
            .wrap(cors)
            .wrap(middleware::Compress::default())
            .wrap(middleware::Logger::default())
            .service(
                web::scope("/printer")
                    .route("/history", web::get().to(handlers::get_history))
            )
            .service(
                web::scope("/server")
                    .route("/gcode_files/upload", web::post().to(handlers::upload_gcode))
                    .route("/power_loss_recovery", web::post().to(handlers::trigger_recovery))
            )
            .route("/ws", web::get().to(ws::ws_handler))
            .service(actix_files::Files::new("/fluidd", "./fluidd").index_file("index.html"))
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}