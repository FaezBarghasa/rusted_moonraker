use std::path::PathBuf;
use walkdir::WalkDir;

use crate::db::DatabaseManager;
use crate::files::analyzer::analyze_gcode;

#[derive(Clone)]
pub struct FileManager {
    pub root_dir: PathBuf,
    pub db: DatabaseManager,
    pub max_upload_size_mb: u64,
}

impl FileManager {
    pub fn new(root_dir: PathBuf, db: DatabaseManager, max_upload_size_mb: u64) -> Self {
        let _ = std::fs::create_dir_all(&root_dir);
        FileManager {
            root_dir,
            db,
            max_upload_size_mb,
        }
    }

    pub async fn sync_database(&self) -> Result<(), Box<dyn std::error::Error>> {
        for entry in WalkDir::new(&self.root_dir).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() {
                let path = entry.path();
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    if ext.eq_ignore_ascii_case("gcode") || ext.eq_ignore_ascii_case("gco") {
                        let rel_path = path.strip_prefix(&self.root_dir)?
                            .to_string_lossy()
                            .to_string();

                        let mut response = self.db.inner
                            .query("SELECT * FROM gcode_files WHERE file_path = $path;")
                            .bind(("path", rel_path.clone()))
                            .await?;

                        let records: Vec<serde_json::Value> = response.take(0)?;
                        if records.is_empty() {
                            if let Ok(meta) = analyze_gcode(path) {
                                self.db.save_gcode_metadata(&rel_path, &meta).await?;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn process_upload(&self, file_name: &str, file_bytes: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let size_mb = (file_bytes.len() as f64) / (1024.0 * 1024.0);
        if size_mb > (self.max_upload_size_mb as f64) {
            return Err("File size exceeds limit".into());
        }

        let file_path = self.root_dir.join(file_name);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&file_path, file_bytes)?;

        if let Ok(meta) = analyze_gcode(&file_path) {
            self.db.save_gcode_metadata(file_name, &meta).await?;
        }

        Ok(())
    }

    pub async fn delete_file(&self, file_name: &str) -> Result<(), Box<dyn std::error::Error>> {
        let file_path = self.root_dir.join(file_name);
        if file_path.exists() {
            std::fs::remove_file(&file_path)?;
        }

        self.db.inner
            .query("DELETE gcode_files WHERE file_path = $path;")
            .bind(("path", file_name.to_string()))
            .await?;

        Ok(())
    }
}
