//! Projects — first-class container for a user's research work (M4).
//!
//! A project belongs to exactly one `owner_user_id`. One project hosts
//! many `research_runs` (lit_review, data_analysis, etc.). The pair
//! `(owner_user_id, slug)` is unique. See plan.md §M4.
//!
//! Status transitions: `active → archived` (soft delete) and
//! `active → complete` (research wrapped). `archived → active` restores.

use std::fmt;
use std::str::FromStr;

use chrono::Utc;
use rbrain_core::error::{BrainError, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

fn db_err<E: std::fmt::Display>(e: E) -> BrainError {
    BrainError::Io(std::io::Error::new(
        std::io::ErrorKind::Other,
        e.to_string(),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Active,
    Archived,
    Complete,
}

impl ProjectStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectStatus::Active => "active",
            ProjectStatus::Archived => "archived",
            ProjectStatus::Complete => "complete",
        }
    }
}

impl fmt::Display for ProjectStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProjectStatus {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "active" => Ok(ProjectStatus::Active),
            "archived" => Ok(ProjectStatus::Archived),
            "complete" => Ok(ProjectStatus::Complete),
            other => Err(format!("unknown project status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub slug: String,
    pub owner_user_id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: ProjectStatus,
    pub created_at: String,
    pub updated_at: String,
}

pub struct ProjectStore<'a> {
    pool: &'a SqlitePool,
}

impl<'a> ProjectStore<'a> {
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Create a new project. `id` is generated if `None`. Status defaults to
    /// `Active`. Returns a `Conflict` error if `(owner_user_id, slug)` already
    /// exists.
    pub async fn create(
        &self,
        id: Option<&str>,
        slug: &str,
        owner_user_id: &str,
        title: &str,
        description: Option<&str>,
    ) -> Result<Project> {
        let id = id
            .map(|s| s.to_string())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO projects
             (id, slug, owner_user_id, title, description, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 'active', ?, ?)",
        )
        .bind(&id)
        .bind(slug)
        .bind(owner_user_id)
        .bind(title)
        .bind(description)
        .bind(&now)
        .bind(&now)
        .execute(self.pool)
        .await
        .map_err(db_err)?;
        self.get(&id).await
    }

    pub async fn get(&self, id: &str) -> Result<Project> {
        let row = sqlx::query(
            "SELECT id, slug, owner_user_id, title, description, status,
                    created_at, updated_at
             FROM projects WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| BrainError::Conflict(format!("project not found: {id}")))?;
        row_to_project(&row)
    }

    pub async fn find_by_slug(&self, owner_user_id: &str, slug: &str) -> Result<Option<Project>> {
        let row = sqlx::query(
            "SELECT id, slug, owner_user_id, title, description, status,
                    created_at, updated_at
             FROM projects WHERE owner_user_id = ? AND slug = ?",
        )
        .bind(owner_user_id)
        .bind(slug)
        .fetch_optional(self.pool)
        .await
        .map_err(db_err)?;
        row.as_ref().map(row_to_project).transpose()
    }

    pub async fn list_by_user(
        &self,
        owner_user_id: &str,
        status: Option<ProjectStatus>,
    ) -> Result<Vec<Project>> {
        let rows = if let Some(s) = status {
            sqlx::query(
                "SELECT id, slug, owner_user_id, title, description, status,
                        created_at, updated_at
                 FROM projects WHERE owner_user_id = ? AND status = ?
                 ORDER BY updated_at DESC",
            )
            .bind(owner_user_id)
            .bind(s.as_str())
            .fetch_all(self.pool)
            .await
            .map_err(db_err)?
        } else {
            sqlx::query(
                "SELECT id, slug, owner_user_id, title, description, status,
                        created_at, updated_at
                 FROM projects WHERE owner_user_id = ?
                 ORDER BY updated_at DESC",
            )
            .bind(owner_user_id)
            .fetch_all(self.pool)
            .await
            .map_err(db_err)?
        };
        rows.iter().map(row_to_project).collect()
    }

    pub async fn set_status(&self, id: &str, status: ProjectStatus) -> Result<Project> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE projects SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status.as_str())
            .bind(&now)
            .bind(id)
            .execute(self.pool)
            .await
            .map_err(db_err)?;
        self.get(id).await
    }

    /// Convenience: archive a project (soft delete). Use `set_status(id, Active)`
    /// to restore.
    pub async fn archive(&self, id: &str) -> Result<Project> {
        self.set_status(id, ProjectStatus::Archived).await
    }

    pub async fn update_metadata(
        &self,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
    ) -> Result<Project> {
        let now = Utc::now().to_rfc3339();
        let current = self.get(id).await?;
        let new_title = title.unwrap_or(&current.title);
        let new_desc = description.or(current.description.as_deref());
        sqlx::query(
            "UPDATE projects SET title = ?, description = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(new_title)
        .bind(new_desc)
        .bind(&now)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(db_err)?;
        self.get(id).await
    }
}

fn row_to_project(row: &sqlx::sqlite::SqliteRow) -> Result<Project> {
    let status_s: String = row.try_get("status").map_err(db_err)?;
    Ok(Project {
        id: row.try_get("id").map_err(db_err)?,
        slug: row.try_get("slug").map_err(db_err)?,
        owner_user_id: row.try_get("owner_user_id").map_err(db_err)?,
        title: row.try_get("title").map_err(db_err)?,
        description: row.try_get("description").map_err(db_err)?,
        status: ProjectStatus::from_str(&status_s).map_err(BrainError::Conflict)?,
        created_at: row.try_get("created_at").map_err(db_err)?,
        updated_at: row.try_get("updated_at").map_err(db_err)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trips() {
        for s in [
            ProjectStatus::Active,
            ProjectStatus::Archived,
            ProjectStatus::Complete,
        ] {
            assert_eq!(ProjectStatus::from_str(s.as_str()).unwrap(), s);
        }
    }
}
