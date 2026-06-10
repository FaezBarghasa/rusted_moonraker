use rmr_core::db::init_db;
use rmr_core::web::start_server;
use log::info;
use std::fs;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    info!("Starting rusted_moonraker App");

    let db_path = "/home/mks/printer_data/database";
    if let Err(e) = fs::create_dir_all(db_path) {
        info!("Warning: Could not create DB directory: {}", e);
    }
    
    let db = match init_db(db_path).await {
        Ok(db) => db,
        Err(e) => panic!("Failed to initialize database: {}", e),
    };

    start_server(db).await
}