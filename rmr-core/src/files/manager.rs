use actix_multipart::Multipart;
use futures_util::TryStreamExt;
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use log::info;

pub async fn save_upload(mut payload: Multipart, dest_dir: &Path) -> Result<String, actix_web::Error> {
    let mut filename = String::new();

    while let Some(mut field) = payload.try_next().await? {
        let content_disposition = field.content_disposition();
        let fname = content_disposition.get_filename().unwrap_or("unknown.gcode").to_string();
        filename = fname.clone();
        let filepath = dest_dir.join(&fname);
        
        info!("Saving file to {:?}", filepath);
        
        let mut f = File::create(filepath).await.map_err(actix_web::error::ErrorInternalServerError)?;
        
        while let Some(chunk) = field.try_next().await? {
            f.write_all(&chunk).await.map_err(actix_web::error::ErrorInternalServerError)?;
        }
    }

    Ok(filename)
}