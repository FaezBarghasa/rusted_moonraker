use actix_web::{web, HttpResponse, Responder};
use actix_multipart::Multipart;
use std::path::Path;
use serde_json::json;

use crate::files::manager::save_upload;
use crate::files::analyzer::{analyze_gcode, GcodeMetadata};
use crate::state_recovery::StateMachine;
use crate::web::AppState;

pub async fn get_history(data: web::Data<AppState>) -> impl Responder {
    let mut response = match data.db.query("SELECT * FROM checkpoints").await {
        Ok(res) => res,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    let history: Vec<crate::state_recovery::PrintRecoveryCheckpoint> = match response.take(0) {
        Ok(h) => h,
        Err(e) => return HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    };

    HttpResponse::Ok().json(history)
}

pub async fn upload_gcode(data: web::Data<AppState>, payload: Multipart) -> impl Responder {
    let dest = Path::new("./uploads");
    if !dest.exists() {
        if let Err(e) = std::fs::create_dir_all(dest) {
            return HttpResponse::InternalServerError().json(json!({"error": format!("Failed to create upload dir: {}", e)}));
        }
    }

    match save_upload(payload, dest).await {
        Ok(filename) => {
            let filepath = dest.join(&filename).to_string_lossy().into_owned();
            let db = data.db.clone();
            let fname_clone = filename.clone();
            
            match tokio::task::spawn_blocking(move || analyze_gcode(&filepath)).await {
                Ok(Ok(metadata)) => {
                    let insert_res: Result<Option<GcodeMetadata>, _> = db.create(("metadata", &fname_clone)).content(&metadata).await;
                    match insert_res {
                        Ok(_) => HttpResponse::Ok().json(json!({
                            "message": "Upload and analysis complete",
                            "filename": filename,
                            "metadata": metadata
                        })),
                        Err(e) => HttpResponse::InternalServerError().json(json!({"error": format!("Database insert failed: {}", e)})),
                    }
                },
                Ok(Err(e)) => HttpResponse::InternalServerError().json(json!({"error": format!("Analysis failed: {}", e)})),
                Err(e) => HttpResponse::InternalServerError().json(json!({"error": format!("Task failed: {}", e)})),
            }
        },
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": e.to_string()})),
    }
}

pub async fn trigger_recovery(data: web::Data<AppState>, query: web::Query<std::collections::HashMap<String, String>>) -> impl Responder {
    let job_id = match query.get("job_id") {
        Some(id) => id,
        None => return HttpResponse::BadRequest().json(json!({"error": "Missing job_id"})),
    };

    let state_machine = StateMachine::new(data.db.clone());
    match state_machine.resume_print_job(job_id).await {
        Ok(checkpoint) => HttpResponse::Ok().json(json!({"message": "Recovery successful", "checkpoint": checkpoint})),
        Err(e) => HttpResponse::InternalServerError().json(json!({"error": format!("Recovery failed: {:?}", e)})),
    }
}