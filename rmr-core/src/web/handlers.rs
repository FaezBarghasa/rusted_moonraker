use actix_web::{get, web, HttpResponse, Responder};
use std::sync::Arc;
use serde_json::json;

use crate::web::SystemState;

#[get("/printer/info")]
pub async fn printer_info(state: web::Data<Arc<SystemState>>) -> impl Responder {
    let klipper_state = state.state_rx.borrow().clone();
    let status_str = klipper_state.print_status;
    HttpResponse::Ok().json(json!({
        "state": status_str.to_lowercase(),
        "state_message": format!("Printer is in {} state", status_str),
        "hostname": "RMR-daemon"
    }))
}

#[get("/server/config")]
pub async fn server_config(state: web::Data<Arc<SystemState>>) -> impl Responder {
    HttpResponse::Ok().json(json!({
        "server": {
            "host": state.config.server.host,
            "port": state.config.server.port,
            "ssl_enabled": state.config.server.ssl_enabled,
            "max_upload_size_mb": state.config.server.max_upload_size_mb
        },
        "klippy": {
            "uds_path": state.config.klippy.uds_path,
            "api_timeout_secs": state.config.klippy.api_timeout_secs
        }
    }))
}

#[get("/server/temperature_store")]
pub async fn temperature_store(state: web::Data<Arc<SystemState>>) -> impl Responder {
    let history = state.temp_history.lock().unwrap();
    let mut tool_temps = Vec::new();
    let mut target_temps = Vec::new();
    let mut timestamps = Vec::new();

    for pt in history.iter() {
        tool_temps.push(pt.tool_temp);
        target_temps.push(pt.target_temp);
        timestamps.push(pt.time);
    }

    HttpResponse::Ok().json(json!({
        "tool_temp": tool_temps,
        "target_temp": target_temps,
        "timestamps": timestamps
    }))
}


use actix_multipart::Multipart;
use futures_util::stream::StreamExt as _;
use tokio::io::AsyncWriteExt;
use std::collections::HashMap;

#[actix_web::post("/server/files/upload")]
pub async fn upload_file(
    state: web::Data<Arc<SystemState>>,
    mut payload: Multipart,
) -> impl Responder {
    let mut filename = String::new();
    let mut root_dir = state.file_manager.root_dir.clone();

    while let Some(item) = payload.next().await {
        let mut field = match item {
            Ok(f) => f,
            Err(_) => return HttpResponse::BadRequest().body("Payload error"),
        };

        let content_disposition = field.content_disposition();
        if let Some(name) = content_disposition.get_name() {
            if name == "file" {
                if let Some(fnm) = content_disposition.get_filename() {
                    filename = fnm.to_string();
                }

                if filename.is_empty() {
                    return HttpResponse::BadRequest().body("Missing filename");
                }

                let filepath = root_dir.join(&filename);
                if let Some(parent) = filepath.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }

                let file = match tokio::fs::File::create(&filepath).await {
                    Ok(f) => f,
                    Err(e) => return HttpResponse::InternalServerError().body(format!("Failed to create file: {}", e)),
                };

                let mut buf_writer = tokio::io::BufWriter::new(file);

                let mut size = 0;
                let max_size = state.file_manager.max_upload_size_mb * 1024 * 1024;

                while let Some(chunk) = field.next().await {
                    let data = match chunk {
                        Ok(d) => d,
                        Err(_) => return HttpResponse::BadRequest().body("Chunk error"),
                    };
                    size += data.len() as u64;
                    if size > max_size {
                        return HttpResponse::PayloadTooLarge().body("File too large");
                    }
                    if let Err(e) = buf_writer.write_all(&data).await {
                        return HttpResponse::InternalServerError().body(format!("Failed to write file: {}", e));
                    }
                }

                if let Err(e) = buf_writer.flush().await {
                    return HttpResponse::InternalServerError().body(format!("Failed to flush file: {}", e));
                }

                if let Ok(meta) = crate::files::analyze_gcode(&filepath) {
                    let _ = state.db.save_gcode_metadata(&filename, &meta).await;
                }
            }
        }
    }

    if filename.is_empty() {
        return HttpResponse::BadRequest().body("No file uploaded");
    }

    HttpResponse::Ok().json(json!({ "item": { "path": filename } }))
}
