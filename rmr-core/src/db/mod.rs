pub mod repo;

use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;
use std::sync::Arc;
use log::info;

pub type Database = Arc<Surreal<Db>>;

pub async fn init_db(path: &str) -> surrealdb::Result<Database> {
    info!("Initializing SurrealDB at {}", path);
    let db = Surreal::new::<RocksDb>(path).await?;
    db.use_ns("moonraker").use_db("moonraker_db").await?;
    
    db.query("DEFINE TABLE spools SCHEMALESS;").await?;
    db.query("DEFINE TABLE config SCHEMAFULL;").await?;
    db.query("DEFINE TABLE metadata SCHEMALESS;").await?;
      
    Ok(Arc::new(db))
}