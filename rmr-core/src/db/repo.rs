use super::Database;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Spool {
    pub id: String,
    pub material: String,
    pub color: String,
    pub weight: f64,
}

pub struct SurrealRepository {
    db: Database,
}

#[derive(Debug)]
pub enum DatabaseError {
    Surreal(surrealdb::Error),
}

impl From<surrealdb::Error> for DatabaseError {
    fn from(err: surrealdb::Error) -> Self {
        DatabaseError::Surreal(err)
    }
}

impl SurrealRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub async fn save_ams_spool(&self, spool: Spool) -> Result<(), DatabaseError> {
        let _: Option<Spool> = self.db.update(("spools", &spool.id)).content(spool).await?;
        Ok(())
    }

    pub async fn get_ams_inventory(&self) -> Result<Vec<Spool>, DatabaseError> {
        let mut response = self.db.query("SELECT * FROM spools").await?;
        let spools: Vec<Spool> = response.take(0)?;
        Ok(spools)
    }
}