pub mod analyzer;
pub mod manager;

pub use analyzer::analyze_gcode;
pub use manager::FileManager;

use actix_web::{get, post, delete, web, HttpResponse, Responder};
use serde_json::json;
use std::sync::Arc;
use std::collections::HashMap;

use crate::web::SystemState;

#[post("/server/files/upload")]
pub async fn upload_file(
    state: web::Data<Arc<SystemState>>,
    query: web::Query<HashMap<String, String>>,
    bytes: web::Bytes,
) -> impl Responder {
    let filename = match query.get("filename") {
        Some(f) => f,
        None => return HttpResponse::BadRequest().body("Missing filename query parameter"),
    };

    match state.file_manager.process_upload(filename, &bytes).await {
        Ok(_) => HttpResponse::Ok().json(json!({ "item": { "path": filename } })),
        Err(e) => HttpResponse::InternalServerError().body(format!("Upload failed: {:?}", e)),
    }
}

#[delete("/server/files/delete")]
pub async fn delete_file(
    state: web::Data<Arc<SystemState>>,
    query: web::Query<HashMap<String, String>>,
) -> impl Responder {
    let filename = match query.get("filename") {
        Some(f) => f,
        None => return HttpResponse::BadRequest().body("Missing filename query parameter"),
    };

    match state.file_manager.delete_file(filename).await {
        Ok(_) => HttpResponse::Ok().json(json!({ "item": { "path": filename } })),
        Err(e) => HttpResponse::InternalServerError().body(format!("Delete failed: {:?}", e)),
    }
}

#[get("/server/files/list")]
pub async fn list_files(
    state: web::Data<Arc<SystemState>>,
) -> impl Responder {
    match state.db.inner.query("SELECT * FROM gcode_files;").await {
        Ok(mut res) => {
            let files: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
            HttpResponse::Ok().json(files)
        }
        Err(e) => HttpResponse::InternalServerError().body(format!("DB query failed: {:?}", e)),
    }
}
