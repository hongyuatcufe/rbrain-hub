//! Project lifecycle fixture (M4 PR-1).
//!
//! Verifies ProjectStore CRUD + status transitions + per-user isolation.
//! Uses raw SQLite (no Engine) because Project storage is independent of
//! the page/chunk subsystem.

mod common;

use common::TestBrain;
use rbrain_engine::engine::Engine;
use rbrain_engine::research::{ProjectStatus, ProjectStore};
use sqlx::sqlite::SqlitePoolOptions;

async fn open_pool(tb: &TestBrain) -> sqlx::SqlitePool {
    // Touch the engine once so it runs migrations against this temp DB.
    let _ = Engine::open(tb.config.clone()).await.expect("Engine::open");
    let url = format!(
        "sqlite://{}?mode=rwc",
        tb.config.db_path.to_string_lossy()
    );
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("connect")
}

#[tokio::test]
async fn create_get_find_by_slug_round_trip() {
    let tb = TestBrain::new().await;
    let pool = open_pool(&tb).await;
    let store = ProjectStore::new(&pool);

    let p = store
        .create(None, "phd", "alice", "PhD dissertation", Some("教育公平方向"))
        .await
        .expect("create");
    assert_eq!(p.slug, "phd");
    assert_eq!(p.owner_user_id, "alice");
    assert_eq!(p.status, ProjectStatus::Active);

    let fetched = store.get(&p.id).await.expect("get");
    assert_eq!(fetched.id, p.id);

    let by_slug = store
        .find_by_slug("alice", "phd")
        .await
        .expect("find")
        .expect("project present");
    assert_eq!(by_slug.id, p.id);
}

#[tokio::test]
async fn duplicate_slug_for_same_user_rejected() {
    let tb = TestBrain::new().await;
    let pool = open_pool(&tb).await;
    let store = ProjectStore::new(&pool);

    store
        .create(None, "dup", "alice", "First", None)
        .await
        .expect("first");
    let second = store.create(None, "dup", "alice", "Second", None).await;
    assert!(
        second.is_err(),
        "(owner_user_id, slug) UNIQUE must reject duplicate"
    );
}

#[tokio::test]
async fn different_users_can_share_a_slug() {
    let tb = TestBrain::new().await;
    let pool = open_pool(&tb).await;
    let store = ProjectStore::new(&pool);

    store
        .create(None, "thesis", "alice", "Alice", None)
        .await
        .expect("alice");
    store
        .create(None, "thesis", "bob", "Bob", None)
        .await
        .expect("bob — same slug under different owner is fine");

    let alices = store.list_by_user("alice", None).await.expect("alice list");
    let bobs = store.list_by_user("bob", None).await.expect("bob list");
    assert_eq!(alices.len(), 1);
    assert_eq!(bobs.len(), 1);
    assert_eq!(alices[0].owner_user_id, "alice");
    assert_eq!(bobs[0].owner_user_id, "bob");
}

#[tokio::test]
async fn list_filters_by_status() {
    let tb = TestBrain::new().await;
    let pool = open_pool(&tb).await;
    let store = ProjectStore::new(&pool);

    let active = store.create(None, "a", "u", "A", None).await.unwrap();
    let archived = store.create(None, "b", "u", "B", None).await.unwrap();
    store.archive(&archived.id).await.unwrap();

    let all = store.list_by_user("u", None).await.unwrap();
    assert_eq!(all.len(), 2);

    let actives = store
        .list_by_user("u", Some(ProjectStatus::Active))
        .await
        .unwrap();
    assert_eq!(actives.len(), 1);
    assert_eq!(actives[0].id, active.id);

    let archives = store
        .list_by_user("u", Some(ProjectStatus::Archived))
        .await
        .unwrap();
    assert_eq!(archives.len(), 1);
    assert_eq!(archives[0].id, archived.id);
}

#[tokio::test]
async fn archive_then_restore_round_trip() {
    let tb = TestBrain::new().await;
    let pool = open_pool(&tb).await;
    let store = ProjectStore::new(&pool);

    let p = store.create(None, "r", "u", "Project", None).await.unwrap();
    assert_eq!(p.status, ProjectStatus::Active);

    let archived = store.archive(&p.id).await.unwrap();
    assert_eq!(archived.status, ProjectStatus::Archived);

    let restored = store
        .set_status(&p.id, ProjectStatus::Active)
        .await
        .unwrap();
    assert_eq!(restored.status, ProjectStatus::Active);
}

#[tokio::test]
async fn complete_status_is_terminal_but_set_status_allows_revert() {
    // M4 doesn't enforce a status DAG (validators do that); set_status is
    // a pure write. Verify both transitions work.
    let tb = TestBrain::new().await;
    let pool = open_pool(&tb).await;
    let store = ProjectStore::new(&pool);

    let p = store.create(None, "c", "u", "Project", None).await.unwrap();
    let done = store
        .set_status(&p.id, ProjectStatus::Complete)
        .await
        .unwrap();
    assert_eq!(done.status, ProjectStatus::Complete);
    let revived = store
        .set_status(&p.id, ProjectStatus::Active)
        .await
        .unwrap();
    assert_eq!(revived.status, ProjectStatus::Active);
}

#[tokio::test]
async fn update_metadata_changes_title_and_description() {
    let tb = TestBrain::new().await;
    let pool = open_pool(&tb).await;
    let store = ProjectStore::new(&pool);

    let p = store
        .create(None, "m", "u", "Old title", Some("old desc"))
        .await
        .unwrap();
    let updated = store
        .update_metadata(&p.id, Some("New title"), Some("new desc"))
        .await
        .unwrap();
    assert_eq!(updated.title, "New title");
    assert_eq!(updated.description.as_deref(), Some("new desc"));
}

#[tokio::test]
async fn get_unknown_id_returns_error() {
    let tb = TestBrain::new().await;
    let pool = open_pool(&tb).await;
    let store = ProjectStore::new(&pool);
    let r = store.get("does-not-exist").await;
    assert!(r.is_err());
}

#[tokio::test]
async fn find_by_slug_returns_none_when_absent() {
    let tb = TestBrain::new().await;
    let pool = open_pool(&tb).await;
    let store = ProjectStore::new(&pool);
    let r = store.find_by_slug("alice", "ghost").await.unwrap();
    assert!(r.is_none());
}
