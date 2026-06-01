use std::path::Path;
use surrealdb::engine::local::{Db, SurrealKv, Mem};
use surrealdb::Surreal;

pub mod migration;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GCodeMetadata {
    pub estimated_time: Option<f64>,
    pub layer_height: Option<f64>,
    pub slicer_type: Option<String>,
    pub thumbnail_path: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrintStats {
    pub total_prints: u64,
    pub successful_prints: u64,
    pub failed_prints: u64,
    pub total_print_time: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("SurrealDB error: {0}")]
    Surreal(#[from] surrealdb::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone)]
pub struct DatabaseManager {
    pub inner: Surreal<Db>,
}

fn check_and_clear_lockfile(db_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(db_dir)?;
    let lock_path = db_dir.join("surreal.lock");
    if lock_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&lock_path) {
            if let Ok(pid) = content.trim().parse::<i32>() {
                // Check if PID is alive on Linux by reading /proc
                let proc_path = format!("/proc/{}", pid);
                let is_alive = Path::new(&proc_path).exists();
                if !is_alive {
                    let _ = std::fs::remove_file(&lock_path);
                }
            } else {
                let _ = std::fs::remove_file(&lock_path);
            }
        } else {
            let _ = std::fs::remove_file(&lock_path);
        }
    }
    std::fs::write(&lock_path, std::process::id().to_string())?;
    Ok(())
}

impl DatabaseManager {
    pub async fn initialize(db_dir: &Path) -> Result<Self, DatabaseError> {
        check_and_clear_lockfile(db_dir)?;
        let path_str = db_dir.to_string_lossy().to_string();
        let db = Surreal::new::<SurrealKv>(&path_str).await?;
        db.use_ns("moonraker").use_db("printer").await?;
        migration::run_migrations(&db).await?;
        Ok(DatabaseManager { inner: db })
    }

    pub async fn initialize_mem() -> Result<Self, DatabaseError> {
        let db = Surreal::new::<Mem>(()).await?;
        db.use_ns("moonraker").use_db("printer").await?;
        migration::run_migrations(&db).await?;
        Ok(DatabaseManager { inner: db })
    }

    pub async fn get_print_statistics(&self) -> Result<PrintStats, DatabaseError> {
        let mut response = self.inner
            .query("SELECT status, print_time FROM print_history;")
            .await?;
        
        let records: Vec<serde_json::Value> = response.take(0)?;
        let mut total_prints = 0;
        let mut successful_prints = 0;
        let mut failed_prints = 0;
        let mut total_print_time = 0.0;

        for rec in records {
            if let Some(status) = rec.get("status").and_then(|v| v.as_str()) {
                total_prints += 1;
                match status {
                    "success" => successful_prints += 1,
                    "failed" => failed_prints += 1,
                    _ => {}
                }
            }
            if let Some(time) = rec.get("print_time").and_then(|v| v.as_f64()) {
                total_print_time += time;
            }
        }

        Ok(PrintStats {
            total_prints,
            successful_prints,
            failed_prints,
            total_print_time,
        })
    }

    pub async fn save_gcode_metadata(&self, file_path: &str, metadata: &GCodeMetadata) -> Result<(), DatabaseError> {
        self.inner
            .query("INSERT INTO gcode_files (file_path, estimated_time, layer_height, slicer_type, thumbnail_path)
                    VALUES ($file_path, $estimated_time, $layer_height, $slicer_type, $thumbnail_path)
                    ON DUPLICATE KEY UPDATE
                    estimated_time = $estimated_time, layer_height = $layer_height, slicer_type = $slicer_type, thumbnail_path = $thumbnail_path;")
            .bind(("file_path", file_path.to_string()))
            .bind(("estimated_time", metadata.estimated_time))
            .bind(("layer_height", metadata.layer_height))
            .bind(("slicer_type", metadata.slicer_type.clone()))
            .bind(("thumbnail_path", metadata.thumbnail_path.clone()))
            .await?;
        Ok(())
    }

    pub async fn add_print_history_record(&self, file_path: &str, status: &str, print_time: f64) -> Result<(), DatabaseError> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.inner
            .query("INSERT INTO print_history (file_path, status, print_time, timestamp) VALUES ($file_path, $status, $print_time, $timestamp);")
            .bind(("file_path", file_path.to_string()))
            .bind(("status", status.to_string()))
            .bind(("print_time", print_time))
            .bind(("timestamp", timestamp))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_db_setup_and_operations() {
        let db = DatabaseManager::initialize_mem().await.unwrap();

        // Save gcode metadata
        let meta = GCodeMetadata {
            estimated_time: Some(1234.5),
            layer_height: Some(0.2),
            slicer_type: Some("PrusaSlicer".to_string()),
            thumbnail_path: Some("thumb.png".to_string()),
        };
        db.save_gcode_metadata("test_file.gcode", &meta).await.unwrap();

        // Get print stats (should be empty initially)
        let stats = db.get_print_statistics().await.unwrap();
        assert_eq!(stats.total_prints, 0);

        // Add history records
        db.add_print_history_record("test_file.gcode", "success", 1000.0).await.unwrap();
        db.add_print_history_record("test_file.gcode", "failed", 500.0).await.unwrap();

        let stats2 = db.get_print_statistics().await.unwrap();
        assert_eq!(stats2.total_prints, 2);
        assert_eq!(stats2.successful_prints, 1);
        assert_eq!(stats2.failed_prints, 1);
        assert_eq!(stats2.total_print_time, 1500.0);
    }
}
