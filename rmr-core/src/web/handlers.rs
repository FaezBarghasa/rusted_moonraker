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
