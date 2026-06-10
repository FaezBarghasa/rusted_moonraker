use serde::{Deserialize, Serialize};
use crate::db::Database;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PrintRecoveryCheckpoint {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub extruder: f64,
    pub hotend_temp: f64,
    pub bed_temp: f64,
    pub file_offset: u64,
}

#[derive(Debug)]
pub enum RecoveryError {
    DbError(surrealdb::Error),
    NoCheckpoint,
}

impl From<surrealdb::Error> for RecoveryError {
    fn from(err: surrealdb::Error) -> Self {
        RecoveryError::DbError(err)
    }
}

pub struct StateMachine {
    db: Database,
}

impl StateMachine {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub async fn save_checkpoint(&self, checkpoint: PrintRecoveryCheckpoint) -> Result<(), RecoveryError> {
        let _: Option<PrintRecoveryCheckpoint> = self.db.update(("checkpoints", &checkpoint.id))
            .content(checkpoint)
            .await?;
        Ok(())
    }

    pub async fn resume_print_job(&self, job_id: &str) -> Result<PrintRecoveryCheckpoint, RecoveryError> {
        let checkpoint: Option<PrintRecoveryCheckpoint> = self.db.select(("checkpoints", job_id)).await?;
        checkpoint.ok_or(RecoveryError::NoCheckpoint)
    }
}