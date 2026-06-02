use std::path::Path;
use surrealdb::engine::local::{Db, RocksDb, Mem};
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrintRecord {
    pub file_path: String,
    pub status: String,
    pub print_time: f64,
    pub timestamp: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebLayoutPreset {
    pub name: String,
    pub layout_data: String,
    pub timestamp: i64,
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
        let db = Surreal::new::<RocksDb>(&path_str).await?;
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

    // PrintRecord CRUD
    pub async fn save_print_record(&self, record: &PrintRecord) -> Result<(), DatabaseError> {
        self.inner
            .query("INSERT INTO print_records (file_path, status, print_time, timestamp)
                    VALUES ($file_path, $status, $print_time, $timestamp);")
            .bind(("file_path", record.file_path.clone()))
            .bind(("status", record.status.clone()))
            .bind(("print_time", record.print_time))
            .bind(("timestamp", record.timestamp))
            .await?;
        Ok(())
    }

    pub async fn get_print_records(&self) -> Result<Vec<PrintRecord>, DatabaseError> {
        let mut response = self.inner
            .query("SELECT * FROM print_records ORDER BY timestamp DESC;")
            .await?;
        let records: Vec<PrintRecord> = response.take(0)?;
        Ok(records)
    }

    // WebLayoutPreset CRUD
    pub async fn save_web_layout_preset(&self, preset: &WebLayoutPreset) -> Result<(), DatabaseError> {
        self.inner
            .query("INSERT INTO web_layout_presets (name, layout_data, timestamp)
                    VALUES ($name, $layout_data, $timestamp)
                    ON DUPLICATE KEY UPDATE
                    layout_data = $layout_data, timestamp = $timestamp;")
            .bind(("name", preset.name.clone()))
            .bind(("layout_data", preset.layout_data.clone()))
            .bind(("timestamp", preset.timestamp))
            .await?;
        Ok(())
    }

    pub async fn get_web_layout_presets(&self) -> Result<Vec<WebLayoutPreset>, DatabaseError> {
        let mut response = self.inner
            .query("SELECT * FROM web_layout_presets ORDER BY timestamp DESC;")
            .await?;
        let presets: Vec<WebLayoutPreset> = response.take(0)?;
        Ok(presets)
    }

    pub async fn get_web_layout_preset(&self, name: &str) -> Result<Option<WebLayoutPreset>, DatabaseError> {
        let mut response = self.inner
            .query("SELECT * FROM web_layout_presets WHERE name = $name LIMIT 1;")
            .bind(("name", name.to_string()))
            .await?;
        let mut presets: Vec<WebLayoutPreset> = response.take(0)?;
        Ok(presets.pop())
    }

    pub async fn delete_web_layout_preset(&self, name: &str) -> Result<(), DatabaseError> {
        self.inner
            .query("DELETE web_layout_presets WHERE name = $name;")
            .bind(("name", name.to_string()))
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

    #[tokio::test]
    async fn test_concurrent_reads_writes() {
        // Create a temporary directory for the RocksDB instance
        let temp_dir = std::env::temp_dir().join(format!("rmr_db_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let db = DatabaseManager::initialize(&temp_dir).await.unwrap();

        let db_arc = std::sync::Arc::new(db);
        let mut handles = vec![];

        for i in 0..10 {
            let db_clone = db_arc.clone();
            let handle = tokio::spawn(async move {
                let preset = WebLayoutPreset {
                    name: format!("preset_{}", i),
                    layout_data: format!("data_{}", i),
                    timestamp: i,
                };
                db_clone.save_web_layout_preset(&preset).await.unwrap();
                let retrieved = db_clone.get_web_layout_preset(&format!("preset_{}", i)).await.unwrap();
                assert!(retrieved.is_some());
                assert_eq!(retrieved.unwrap().layout_data, format!("data_{}", i));
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Clean up temporary database files
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
