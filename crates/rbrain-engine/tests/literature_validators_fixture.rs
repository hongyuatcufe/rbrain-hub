//! M3 Slice 1 acceptance — literature-review validators.
//!
//! Covers the five validators wired into `TaskType::LiteratureReview`:
//! `source_count_minimum`, `citation_chunks_exist`,
//! `citation_chunk_matches_slug`, `synthesis_sections_have_citations`,
//! `review_links_to_synthesis_pages`.

mod common;

use common::TestBrain;
use rbrain_core::embedder::Embedder;
use rbrain_core::page::Page;
use rbrain_engine::Engine;
use rbrain_engine::evidence::{
    ValidatorStatus, citation_chunk_matches_slug, citation_chunks_exist,
    review_links_to_synthesis_pages, source_count_minimum, synthesis_sections_have_citations,
};
use rbrain_llm::mock::MockEmbedder;
use rbrain_search::{LanceStore, TantivyIndex};
use std::sync::Arc;

const RUN_SLUG: &str = "research/runs/lr-test";

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

async fn put_typed_page(engine: &Engine, slug: &str, page_type: &str, title: &str, body: String) {
    let mut fm = serde_json::Map::new();
    fm.insert("type".into(), serde_json::json!(page_type));
    fm.insert("title".into(), serde_json::json!(title));
    let mut page = Page::new(slug.to_string(), page_type.to_string(), body);
    page.frontmatter = serde_json::Value::Object(fm);
    page.title = title.to_string();
    engine.put_page(page).await.expect("put_page");
}

/// Insert a chunk row directly. `put_page` enqueues an embed job but does
/// not run it synchronously, so chunks must be inserted manually for tests.
async fn insert_chunk(engine: &Engine, page_slug: &str, chunk_idx: i64, text: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "INSERT INTO chunks
         (page_slug, chunk_idx, text, is_compiled_truth, language, has_embedding, indexed_in_vectors, created_at)
         VALUES (?, ?, ?, 1, 'en', 0, 0, datetime('now'))
         RETURNING id",
    )
    .bind(page_slug)
    .bind(chunk_idx)
    .bind(text)
    .fetch_one(engine.get_db())
    .await
    .expect("INSERT chunk")
}

// ─── source_count_minimum ───────────────────────────────────────────────────

#[tokio::test]
async fn source_count_minimum_fails_below_threshold() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let r = source_count_minimum(engine.get_db(), RUN_SLUG).await.unwrap();
    assert_eq!(r.status, ValidatorStatus::Fail);
}

#[tokio::test]
async fn source_count_minimum_passes_at_threshold() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    put_typed_page(&engine, "raw/articles/a", "note", "A", "alpha".into()).await;
    put_typed_page(&engine, "raw/articles/b", "note", "B", "beta".into()).await;
    put_typed_page(&engine, "raw/articles/c", "note", "C", "gamma".into()).await;

    let r = source_count_minimum(engine.get_db(), RUN_SLUG).await.unwrap();
    assert_eq!(r.status, ValidatorStatus::Pass);
}

// ─── citation_chunks_exist ──────────────────────────────────────────────────

#[tokio::test]
async fn citation_chunks_exist_warns_when_no_synthesis_pages_yet() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let r = citation_chunks_exist(engine.get_db(), RUN_SLUG).await.unwrap();
    assert_eq!(r.status, ValidatorStatus::Warn);
}

#[tokio::test]
async fn citation_chunks_exist_passes_when_all_refs_resolve() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    put_typed_page(&engine, "raw/articles/a", "note", "A", "alpha".into()).await;
    let chunk_id = insert_chunk(&engine, "raw/articles/a", 0, "alpha chunk").await;

    let body = format!(
        "# 综合\n\n## 主题\n\n这是引用证据。[[raw/articles/a | chunk:{chunk_id}]]\n"
    );
    put_typed_page(&engine, "research/synthesis/x", "synthesis", "X", body).await;

    let r = citation_chunks_exist(engine.get_db(), RUN_SLUG).await.unwrap();
    assert_eq!(r.status, ValidatorStatus::Pass);
}

#[tokio::test]
async fn citation_chunks_exist_fails_when_chunk_id_invented() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    put_typed_page(&engine, "raw/articles/a", "note", "A", "alpha".into()).await;
    let _ = insert_chunk(&engine, "raw/articles/a", 0, "alpha").await;

    let body = "# 综合\n\n## 主题\n\n虚假引用 [[raw/articles/a | chunk:9999]]\n".to_string();
    put_typed_page(&engine, "research/synthesis/x", "synthesis", "X", body).await;

    let r = citation_chunks_exist(engine.get_db(), RUN_SLUG).await.unwrap();
    assert_eq!(r.status, ValidatorStatus::Fail);
    assert_eq!(r.affected_slugs, vec!["research/synthesis/x".to_string()]);
}

// ─── citation_chunk_matches_slug ────────────────────────────────────────────

#[tokio::test]
async fn citation_chunk_matches_slug_passes_when_slug_matches() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    put_typed_page(&engine, "raw/articles/a", "note", "A", "alpha".into()).await;
    let chunk_id = insert_chunk(&engine, "raw/articles/a", 0, "alpha").await;

    let body = format!(
        "# 综合\n\n## 主题\n\n证据 [[raw/articles/a | chunk:{chunk_id}]]\n"
    );
    put_typed_page(&engine, "research/synthesis/x", "synthesis", "X", body).await;

    let r = citation_chunk_matches_slug(engine.get_db(), RUN_SLUG)
        .await
        .unwrap();
    assert_eq!(r.status, ValidatorStatus::Pass);
}

#[tokio::test]
async fn citation_chunk_matches_slug_fails_on_cross_slug_citation() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    put_typed_page(&engine, "raw/articles/a", "note", "A", "alpha".into()).await;
    put_typed_page(&engine, "raw/articles/b", "note", "B", "beta".into()).await;
    let chunk_a = insert_chunk(&engine, "raw/articles/a", 0, "alpha").await;

    // chunk_a belongs to /a but citation attributes it to /b.
    let body = format!(
        "# 综合\n\n## 主题\n\n错配 [[raw/articles/b | chunk:{chunk_a}]]\n"
    );
    put_typed_page(&engine, "research/synthesis/x", "synthesis", "X", body).await;

    let r = citation_chunk_matches_slug(engine.get_db(), RUN_SLUG)
        .await
        .unwrap();
    assert_eq!(r.status, ValidatorStatus::Fail);
}

// ─── synthesis_sections_have_citations ──────────────────────────────────────

#[tokio::test]
async fn synthesis_sections_have_citations_warns_when_no_synthesis_yet() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let r = synthesis_sections_have_citations(engine.get_db(), RUN_SLUG)
        .await
        .unwrap();
    assert_eq!(r.status, ValidatorStatus::Warn);
}

#[tokio::test]
async fn synthesis_sections_have_citations_fails_on_uncited_sprawl() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let mut body = String::from("# 综合\n\n");
    body.push_str("## 主题一\n\n讨论但没有引用证据。这是一个比较长的段落以避免被判定为过薄。\n\n");
    body.push_str("## 主题二\n\n讨论但也没有引用证据。这是一个比较长的段落以避免被判定为过薄。\n\n");
    body.push_str("## 主题三\n\n第三段没有引用证据。这是一个比较长的段落以避免被判定为过薄。\n\n");
    put_typed_page(&engine, "research/synthesis/x", "synthesis", "X", body).await;

    let r = synthesis_sections_have_citations(engine.get_db(), RUN_SLUG)
        .await
        .unwrap();
    assert_eq!(r.status, ValidatorStatus::Fail);
}

#[tokio::test]
async fn synthesis_sections_have_citations_passes_on_cited_compact() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let body = "# 综合\n\n## 主题一\n\n这是有引用的段落，足够长以满足正文长度要求。[[raw/a | chunk:1]]\n\n## 主题二\n\n第二段有引用，足够长以满足正文长度要求。[[raw/b | chunk:2]]\n".to_string();
    put_typed_page(&engine, "research/synthesis/x", "synthesis", "X", body).await;

    let r = synthesis_sections_have_citations(engine.get_db(), RUN_SLUG)
        .await
        .unwrap();
    assert_eq!(r.status, ValidatorStatus::Pass);
}

// ─── review_links_to_synthesis_pages ────────────────────────────────────────

#[tokio::test]
async fn review_links_to_synthesis_warns_when_no_wiki() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let r = review_links_to_synthesis_pages(engine.get_db(), RUN_SLUG)
        .await
        .unwrap();
    assert_eq!(r.status, ValidatorStatus::Warn);
}

#[tokio::test]
async fn review_links_to_synthesis_fails_when_wiki_has_no_synthesis_outlinks() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    put_typed_page(
        &engine,
        "research/wiki/r",
        "wiki",
        "Review",
        "# Review\n\n空综述\n".into(),
    )
    .await;
    let r = review_links_to_synthesis_pages(engine.get_db(), RUN_SLUG)
        .await
        .unwrap();
    assert_eq!(r.status, ValidatorStatus::Fail);
}

#[tokio::test]
async fn review_links_to_synthesis_passes_when_wiki_cites_synthesis() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    put_typed_page(
        &engine,
        "research/synthesis/s",
        "synthesis",
        "S",
        "# S\n\n讨论\n".into(),
    )
    .await;
    put_typed_page(
        &engine,
        "research/wiki/r",
        "wiki",
        "Review",
        "# Review\n\n[[research/synthesis/s]]\n".into(),
    )
    .await;
    engine
        .add_link("research/wiki/r", "research/synthesis/s", "cites", None, None)
        .await
        .unwrap();

    let r = review_links_to_synthesis_pages(engine.get_db(), RUN_SLUG)
        .await
        .unwrap();
    assert_eq!(r.status, ValidatorStatus::Pass);
}
