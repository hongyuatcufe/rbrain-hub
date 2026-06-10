//! End-to-end fixture for the [`TenantView`] wrapper introduced in M4 PR-1b.
//!
//! Verifies that `engine.with_tenant(ctx).method(...)` correctly enforces
//! the tenancy invariants described in plan.md §M4:
//!
//! - **Read isolation**: user A's pages are invisible to user B.
//! - **Global pages**: visible to every user (subject to topic-subscription
//!   filters which arrive in a later PR; for now any user sees any global).
//! - **Write attribution**: `put_page` embeds (ctx.user_id, ctx.project_id)
//!   onto the new row.
//! - **Link guard**: `add_link` from a foreign-tenant source is refused.
//! - **Link scope**: `outlinks`/`backlinks` honour the readable-user-id set.
//! - **Delete guard**: `delete_page` refuses cross-tenant slugs.

mod common;

use common::TestBrain;
use rbrain_core::embedder::Embedder;
use rbrain_core::page::Page;
use rbrain_engine::{Engine, TenantContext};
use rbrain_llm::mock::MockEmbedder;
use rbrain_search::{LanceStore, TantivyIndex};
use std::sync::Arc;

async fn open_mock_engine(tb: &TestBrain) -> Engine {
    let embedder: Arc<dyn Embedder> = Arc::new(MockEmbedder::new(tb.config.embedding_dim));
    let vector_store = Arc::new(
        LanceStore::new(tb.config.lance_dir.clone(), tb.config.embedding_dim)
            .await
            .expect("LanceStore::new"),
    );
    let keyword_index =
        Arc::new(TantivyIndex::new(tb.config.tantivy_dir.clone()).expect("TantivyIndex::new"));
    Engine::open_with_search(tb.config.clone(), embedder, vector_store, keyword_index)
        .await
        .expect("Engine::open_with_search")
}

fn page(slug: &str, page_type: &str, title: &str) -> Page {
    Page::new(
        slug.to_string(),
        page_type.to_string(),
        format!("# {title}\n\n_body_\n"),
    )
}

#[tokio::test]
async fn user_a_cannot_read_user_b_pages_through_view() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let alice = engine.with_tenant(TenantContext::for_user("alice", Some("phd")));
    let bob = engine.with_tenant(TenantContext::for_user("bob", Some("thesis")));

    alice
        .put_page(page("notes/alice-a", "note", "Alice's note"))
        .await
        .unwrap();
    bob.put_page(page("notes/bob-b", "note", "Bob's note"))
        .await
        .unwrap();

    // Each can read their own.
    assert!(alice.get_page("notes/alice-a").await.is_ok());
    assert!(bob.get_page("notes/bob-b").await.is_ok());

    // Cross-reads must fail with "page not found".
    let err_a = alice.get_page("notes/bob-b").await;
    assert!(err_a.is_err(), "alice should not see bob's page");
    let err_b = bob.get_page("notes/alice-a").await;
    assert!(err_b.is_err(), "bob should not see alice's page");
}

#[tokio::test]
async fn global_pages_visible_to_every_user_context() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let pipeline = engine.with_tenant(TenantContext::global());
    pipeline
        .put_page(page("raw/articles/journal-x", "raw", "Journal X article"))
        .await
        .unwrap();

    let alice = engine.with_tenant(TenantContext::for_user("alice", Some("phd")));
    let bob = engine.with_tenant(TenantContext::for_user("bob", Some("thesis")));

    // Both users can read global content.
    assert!(alice.get_page("raw/articles/journal-x").await.is_ok());
    assert!(bob.get_page("raw/articles/journal-x").await.is_ok());
}

#[tokio::test]
async fn list_pages_scoped_to_caller() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let alice = engine.with_tenant(TenantContext::for_user("alice", Some("phd")));
    let bob = engine.with_tenant(TenantContext::for_user("bob", Some("thesis")));

    alice
        .put_page(page("notes/a1", "note", "Alice 1"))
        .await
        .unwrap();
    alice
        .put_page(page("notes/a2", "note", "Alice 2"))
        .await
        .unwrap();
    bob.put_page(page("notes/b1", "note", "Bob 1"))
        .await
        .unwrap();

    let alice_notes = alice
        .list_pages(Some("note"), None, None, None, None)
        .await
        .unwrap();
    let bob_notes = bob
        .list_pages(Some("note"), None, None, None, None)
        .await
        .unwrap();

    assert_eq!(alice_notes.len(), 2);
    assert_eq!(bob_notes.len(), 1);
    let alice_slugs: Vec<&str> = alice_notes.iter().map(|p| p.slug.as_str()).collect();
    assert!(alice_slugs.contains(&"notes/a1"));
    assert!(alice_slugs.contains(&"notes/a2"));
    assert!(!alice_slugs.contains(&"notes/b1"));
}

#[tokio::test]
async fn list_pages_for_user_includes_global() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let global = engine.with_tenant(TenantContext::global());
    global
        .put_page(page("raw/articles/g1", "raw", "Global article"))
        .await
        .unwrap();

    let alice = engine.with_tenant(TenantContext::for_user("alice", Some("phd")));
    alice
        .put_page(page("notes/own", "note", "own note"))
        .await
        .unwrap();

    let all_alice = alice
        .list_pages(None, None, None, None, None)
        .await
        .unwrap();
    let slugs: Vec<&str> = all_alice.iter().map(|p| p.slug.as_str()).collect();
    assert!(slugs.contains(&"raw/articles/g1"), "global reachable");
    assert!(slugs.contains(&"notes/own"), "own reachable");
}

#[tokio::test]
async fn add_link_refused_when_source_belongs_to_other_user() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let alice = engine.with_tenant(TenantContext::for_user("alice", Some("phd")));
    let bob = engine.with_tenant(TenantContext::for_user("bob", Some("thesis")));

    alice
        .put_page(page("notes/alice-claim", "finding", "Alice claim"))
        .await
        .unwrap();
    bob.put_page(page("notes/bob-claim", "finding", "Bob claim"))
        .await
        .unwrap();

    // Bob tries to add a link out of alice's page. Must be refused.
    let cross = bob
        .add_link("notes/alice-claim", "notes/bob-claim", "supports", None, None)
        .await;
    assert!(
        cross.is_err(),
        "add_link from a page belonging to another user must be refused"
    );
}

#[tokio::test]
async fn outlinks_and_backlinks_scoped_to_caller() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let alice = engine.with_tenant(TenantContext::for_user("alice", Some("phd")));
    let bob = engine.with_tenant(TenantContext::for_user("bob", Some("thesis")));

    alice.put_page(page("a/from", "note", "From")).await.unwrap();
    alice.put_page(page("a/to", "note", "To")).await.unwrap();
    bob.put_page(page("b/from", "note", "BFrom")).await.unwrap();
    bob.put_page(page("b/to", "note", "BTo")).await.unwrap();

    alice
        .add_link("a/from", "a/to", "cites", None, None)
        .await
        .unwrap();
    bob.add_link("b/from", "b/to", "cites", None, None)
        .await
        .unwrap();

    let alice_out = alice.outlinks("a/from").await.unwrap();
    assert_eq!(alice_out.len(), 1);
    assert_eq!(alice_out[0].target_slug, "a/to");

    let bob_out = bob.outlinks("b/from").await.unwrap();
    assert_eq!(bob_out.len(), 1);
    assert_eq!(bob_out[0].target_slug, "b/to");

    // Bob asking about a/from sees no outlinks — alice's edges are invisible.
    let bob_sees_alice = bob.outlinks("a/from").await.unwrap();
    assert!(bob_sees_alice.is_empty());
}

#[tokio::test]
async fn delete_page_refuses_cross_tenant_slug() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let alice = engine.with_tenant(TenantContext::for_user("alice", Some("phd")));
    let bob = engine.with_tenant(TenantContext::for_user("bob", Some("thesis")));

    alice
        .put_page(page("notes/secret", "note", "Alice secret"))
        .await
        .unwrap();

    let err = bob.delete_page("notes/secret").await;
    assert!(
        err.is_err(),
        "bob must not be able to delete alice's page even by slug"
    );

    // Alice can still read it.
    assert!(alice.get_page("notes/secret").await.is_ok());
}

#[tokio::test]
async fn put_page_refuses_cross_tenant_slug_takeover() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let alice = engine.with_tenant(TenantContext::for_user("alice", Some("phd")));
    let bob = engine.with_tenant(TenantContext::for_user("bob", Some("thesis")));

    alice
        .put_page(page("notes/contested", "note", "Alice v1"))
        .await
        .unwrap();
    let takeover = bob.put_page(page("notes/contested", "note", "Bob v1")).await;
    assert!(
        takeover.is_err(),
        "cross-tenant put_page must not overwrite an existing slug"
    );

    assert!(alice.get_page("notes/contested").await.is_ok());
    assert!(bob.get_page("notes/contested").await.is_err());
}

#[tokio::test]
async fn user_cannot_delete_or_link_from_global_page() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let global = engine.with_tenant(TenantContext::global());
    let alice = engine.with_tenant(TenantContext::for_user("alice", Some("phd")));

    global
        .put_page(page("raw/articles/global-source", "raw", "Global source"))
        .await
        .unwrap();
    alice
        .put_page(page("notes/alice-target", "note", "Alice target"))
        .await
        .unwrap();

    assert!(
        alice.delete_page("raw/articles/global-source").await.is_err(),
        "readable global page must not be writable by user tenant"
    );
    assert!(global.get_page("raw/articles/global-source").await.is_ok());

    assert!(
        alice
            .add_link(
                "raw/articles/global-source",
                "notes/alice-target",
                "supports",
                None,
                None
            )
            .await
            .is_err(),
        "user tenant must not create outgoing links from a global-owned page"
    );
}

#[tokio::test]
async fn search_with_context_does_not_return_other_tenant_chunks() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let alice_ctx = TenantContext::for_user("alice", Some("phd"));
    let bob_ctx = TenantContext::for_user("bob", Some("thesis"));
    let alice = engine.with_tenant(alice_ctx.clone());
    let bob = engine.with_tenant(bob_ctx.clone());

    alice
        .put_page(Page::new(
            "notes/alice-search".to_string(),
            "note".to_string(),
            "# Alice\n\nalice-only-token\n".to_string(),
        ))
        .await
        .unwrap();
    bob.put_page(Page::new(
        "notes/bob-search".to_string(),
        "note".to_string(),
        "# Bob\n\nbob-only-token\n".to_string(),
    ))
    .await
    .unwrap();

    let alice_page = alice.get_page("notes/alice-search").await.unwrap();
    let bob_page = bob.get_page("notes/bob-search").await.unwrap();
    engine
        .chunk_and_embed_page_with_ctx(&alice_page, &alice_ctx)
        .await
        .unwrap();
    engine
        .chunk_and_embed_page_with_ctx(&bob_page, &bob_ctx)
        .await
        .unwrap();

    let lang = rbrain_core::page::Language::En;
    let alice_hits = alice
        .search_with_context("bob-only-token", &lang, 10, false, 1)
        .await
        .unwrap();
    assert!(
        alice_hits
            .iter()
            .all(|hit| hit.page_slug != "notes/bob-search" && !hit.text.contains("bob-only-token")),
        "alice search must not return bob's private chunk"
    );

    let bob_hits = bob
        .search_with_context("bob-only-token", &lang, 10, false, 1)
        .await
        .unwrap();
    assert_eq!(bob_hits.len(), 1);
    assert_eq!(bob_hits[0].page_slug, "notes/bob-search");
}

#[tokio::test]
async fn same_user_projects_are_scoped_separately() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let phd = engine.with_tenant(TenantContext::for_user("alice", Some("phd")));
    let grant = engine.with_tenant(TenantContext::for_user("alice", Some("grant")));

    phd.put_page(page("notes/phd-only", "note", "PhD")).await.unwrap();
    grant
        .put_page(page("notes/grant-only", "note", "Grant"))
        .await
        .unwrap();

    let phd_slugs: Vec<String> = phd
        .list_pages(Some("note"), None, None, None, None)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.slug)
        .collect();
    assert!(phd_slugs.contains(&"notes/phd-only".to_string()));
    assert!(!phd_slugs.contains(&"notes/grant-only".to_string()));

    let grant_slugs: Vec<String> = grant
        .list_pages(Some("note"), None, None, None, None)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.slug)
        .collect();
    assert!(grant_slugs.contains(&"notes/grant-only".to_string()));
    assert!(!grant_slugs.contains(&"notes/phd-only".to_string()));
}
