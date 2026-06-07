//! SQLite-backed CRUD for `research_runs`.

use chrono::Utc;
use rbrain_core::error::{BrainError, Result};
use sqlx::{Row, SqlitePool};

use super::model::{ResearchRun, RunStatus, TaskType};

fn db_err<E: std::fmt::Display>(e: E) -> BrainError {
    BrainError::Io(std::io::Error::new(
        std::io::ErrorKind::Other,
        e.to_string(),
    ))
}

pub struct ResearchRunStore<'a> {
    pool: &'a SqlitePool,
}

impl<'a> ResearchRunStore<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new run. If `id` is `None`, a uuid is generated.
    /// Status defaults to `Planned`.
    pub async fn create(
        &self,
        id: Option<&str>,
        slug: &str,
        task_type: TaskType,
        created_by: &str,
    ) -> Result<ResearchRun> {
        let id = id
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO research_runs
             (id, slug, task_type, status, fingerprint, created_by, created_at, updated_at)
             VALUES (?, ?, ?, 'planned', '', ?, ?, ?)",
        )
        .bind(&id)
        .bind(slug)
        .bind(task_type.as_str())
        .bind(created_by)
        .bind(&now)
        .bind(&now)
        .execute(self.pool)
        .await
        .map_err(db_err)?;
        self.get(&id).await
    }

    pub async fn get(&self, id: &str) -> Result<ResearchRun> {
        let row = sqlx::query(
            "SELECT id, slug, task_type, status, fingerprint, created_by,
                    started_at, completed_at, last_validated_at, last_validation_summary,
                    created_at, updated_at
             FROM research_runs WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| BrainError::Conflict(format!("research_run not found: {id}")))?;
        row_to_run(&row)
    }

    pub async fn find_by_slug(&self, slug: &str) -> Result<Option<ResearchRun>> {
        let row = sqlx::query(
            "SELECT id, slug, task_type, status, fingerprint, created_by,
                    started_at, completed_at, last_validated_at, last_validation_summary,
                    created_at, updated_at
             FROM research_runs WHERE slug = ?",
        )
        .bind(slug)
        .fetch_optional(self.pool)
        .await
        .map_err(db_err)?;
        row.as_ref().map(row_to_run).transpose()
    }

    pub async fn set_status(&self, id: &str, next: RunStatus) -> Result<ResearchRun> {
        let current = self.get(id).await?;
        if current.status != next && !current.status.can_transition_to(next) {
            return Err(BrainError::Conflict(format!(
                "illegal transition for run {id}: {} → {}",
                current.status, next
            )));
        }
        let now = Utc::now().to_rfc3339();
        let started_at = if next == RunStatus::Running && current.started_at.is_none() {
            Some(now.clone())
        } else {
            current.started_at.clone()
        };
        let completed_at = if next == RunStatus::Complete {
            Some(now.clone())
        } else {
            current.completed_at.clone()
        };
        sqlx::query(
            "UPDATE research_runs
             SET status = ?, started_at = ?, completed_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(next.as_str())
        .bind(&started_at)
        .bind(&completed_at)
        .bind(&now)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(db_err)?;
        self.get(id).await
    }

    pub async fn record_validation(
        &self,
        id: &str,
        fingerprint: &str,
        summary_json: &str,
    ) -> Result<ResearchRun> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE research_runs
             SET fingerprint = ?, last_validated_at = ?, last_validation_summary = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(fingerprint)
        .bind(&now)
        .bind(summary_json)
        .bind(&now)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(db_err)?;
        self.get(id).await
    }

    pub async fn list(&self, status: Option<RunStatus>) -> Result<Vec<ResearchRun>> {
        let rows = if let Some(s) = status {
            sqlx::query(
                "SELECT id, slug, task_type, status, fingerprint, created_by,
                        started_at, completed_at, last_validated_at, last_validation_summary,
                        created_at, updated_at
                 FROM research_runs WHERE status = ? ORDER BY updated_at DESC",
            )
            .bind(s.as_str())
            .fetch_all(self.pool)
            .await
            .map_err(db_err)?
        } else {
            sqlx::query(
                "SELECT id, slug, task_type, status, fingerprint, created_by,
                        started_at, completed_at, last_validated_at, last_validation_summary,
                        created_at, updated_at
                 FROM research_runs ORDER BY updated_at DESC",
            )
            .fetch_all(self.pool)
            .await
            .map_err(db_err)?
        };
        rows.iter().map(row_to_run).collect()
    }
}

fn row_to_run(row: &sqlx::sqlite::SqliteRow) -> Result<ResearchRun> {
    use std::str::FromStr;
    let task_type_s: String = row.try_get("task_type").map_err(db_err)?;
    let status_s: String = row.try_get("status").map_err(db_err)?;
    Ok(ResearchRun {
        id: row.try_get("id").map_err(db_err)?,
        slug: row.try_get("slug").map_err(db_err)?,
        task_type: TaskType::from_str(&task_type_s).map_err(BrainError::Conflict)?,
        status: RunStatus::from_str(&status_s).map_err(BrainError::Conflict)?,
        fingerprint: row.try_get("fingerprint").map_err(db_err)?,
        created_by: row.try_get("created_by").map_err(db_err)?,
        started_at: row.try_get("started_at").map_err(db_err)?,
        completed_at: row.try_get("completed_at").map_err(db_err)?,
        last_validated_at: row.try_get("last_validated_at").map_err(db_err)?,
        last_validation_summary: row.try_get("last_validation_summary").map_err(db_err)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}
