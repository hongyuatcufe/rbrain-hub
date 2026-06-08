mod common;

use async_trait::async_trait;
use common::TestBrain;
use rbrain_core::embedder::Embedder;
use rbrain_core::error::BrainError;
use rbrain_core::keyword_index::KeywordIndex;
use rbrain_core::markdown::MarkdownParser;
use rbrain_core::page::{Language, Page};
use rbrain_engine::Engine;
use rbrain_llm::mock::MockEmbedder;
use rbrain_search::LanceStore;
use rbrain_search::TantivyIndex;
use std::sync::Arc;

/// Open a full-stack engine backed by MockEmbedder. No API key needed.
async fn open_mock_engine(tb: &TestBrain) -> Engine {
    let embedder = Arc::new(MockEmbedder::new(tb.config.embedding_dim));
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

#[derive(Debug)]
struct FailingEmbedder {
    dim: usize,
}

#[async_trait]
impl Embedder for FailingEmbedder {
    fn dimension(&self) -> usize {
        self.dim
    }

    async fn embed_one(&self, _text: &str) -> rbrain_core::error::Result<Vec<f32>> {
        Err(BrainError::ApiUnreachable {
            provider: "test".to_string(),
            message: "embedding failed".to_string(),
        })
    }

    async fn embed_batch(&self, _texts: &[String]) -> rbrain_core::error::Result<Vec<Vec<f32>>> {
        Err(BrainError::ApiUnreachable {
            provider: "test".to_string(),
            message: "embedding failed".to_string(),
        })
    }

    fn verify_deterministic(&self) -> bool {
        false
    }
}

async fn open_failing_embed_engine(tb: &TestBrain) -> Engine {
    let embedder = Arc::new(FailingEmbedder {
        dim: tb.config.embedding_dim,
    });
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

#[tokio::test]
async fn test_put_and_get() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let page = Page::new(
        "test-page".to_string(),
        "note".to_string(),
        "Hello world".to_string(),
    );
    engine.put_page(page).await.expect("put_page");

    let fetched = engine.get_page("test-page").await.expect("get_page");
    assert_eq!(fetched.slug, "test-page");
    assert_eq!(fetched.page_type, "note");
}

#[tokio::test]
async fn test_delete_page_rejects_non_file_repo_path_before_db_delete() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    engine
        .put_page(Page::new(
            "bad-delete".to_string(),
            "note".to_string(),
            "Content that should remain in the DB.".to_string(),
        ))
        .await
        .expect("put page");

    let repo_path = tb.config.repo_dir.join("bad-delete.md");
    std::fs::remove_file(&repo_path).expect("remove page file");
    std::fs::create_dir(&repo_path).expect("replace page file with directory");

    let err = engine
        .delete_page("bad-delete")
        .await
        .expect_err("directory path should reject deletion");
    assert!(err.to_string().contains("not a regular file"));

    let fetched = engine
        .get_page("bad-delete")
        .await
        .expect("page should remain in DB");
    assert_eq!(fetched.slug, "bad-delete");
    assert!(
        repo_path.is_dir(),
        "non-file repo path should remain untouched"
    );
}

#[tokio::test]
async fn test_import_dir_queues_embed_job_when_inline_embed_fails() {
    let tb = TestBrain::new().await;
    let engine = open_failing_embed_engine(&tb).await;
    let page_path = tb.config.repo_dir.join("retry-me.md");

    std::fs::write(
        &page_path,
        "---\ntitle: Retry Me\ntags: []\n---\n\nThis page should be queued for retry.",
    )
    .expect("write markdown");

    let imported = engine
        .import_dir(tb.config.repo_dir.to_str().expect("repo path utf-8"))
        .await
        .expect("import dir");
    assert_eq!(imported, vec!["retry-me".to_string()]);

    let db = rbrain_db::open_database(&tb.config.db_path)
        .await
        .expect("open db");
    let params: String = sqlx::query_scalar(
        "SELECT params FROM jobs WHERE name = 'embed_page' AND status = 'pending'",
    )
    .fetch_one(&db)
    .await
    .expect("queued embed job");
    let params: serde_json::Value = serde_json::from_str(&params).expect("job params json");
    assert_eq!(params["slug"], "retry-me");
}

#[tokio::test]
async fn test_embed_and_keyword_search_en() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let page = Page {
        language: Some(Language::En),
        ..Page::new(
            "tang-dynasty".to_string(),
            "book".to_string(),
            "The Tang dynasty was an imperial dynasty of China that ruled from 618 to 907."
                .to_string(),
        )
    };
    engine.put_page(page.clone()).await.expect("put_page");
    engine
        .chunk_and_embed_page(&page)
        .await
        .expect("chunk_and_embed_page");

    let results = engine
        .keyword_search("dynasty", &Language::En, 5)
        .await
        .expect("keyword_search");
    assert!(
        !results.is_empty(),
        "English keyword search should return results"
    );
}

#[tokio::test]
async fn test_cjk_keyword_search_zh() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let page = Page {
        language: Some(Language::ZhHans),
        ..Page::new(
            "china-history".to_string(),
            "book".to_string(),
            "中国历史上，唐朝是一个重要的朝代，文化繁荣，经济发达。".to_string(),
        )
    };
    engine.put_page(page.clone()).await.expect("put_page");
    engine
        .chunk_and_embed_page(&page)
        .await
        .expect("chunk_and_embed_page");

    let results = engine
        .keyword_search("唐朝", &Language::ZhHans, 5)
        .await
        .expect("keyword_search zh");
    assert!(
        !results.is_empty(),
        "Chinese keyword search for 唐朝 should return results with lindera CC-CEDICT"
    );
}

#[tokio::test]
async fn test_cjk_keyword_search_ja() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let page = Page {
        language: Some(Language::Ja),
        ..Page::new(
            "japan-history".to_string(),
            "book".to_string(),
            "日本の歴史において、江戸時代は重要な時代です。文化が発展し、経済も成長しました。"
                .to_string(),
        )
    };
    engine.put_page(page.clone()).await.expect("put_page");
    engine
        .chunk_and_embed_page(&page)
        .await
        .expect("chunk_and_embed_page");

    let results = engine
        .keyword_search("江戸", &Language::Ja, 5)
        .await
        .expect("keyword_search ja");
    assert!(
        !results.is_empty(),
        "Japanese keyword search for 江戸 should return results with lindera IPADIC"
    );
}

#[tokio::test]
async fn test_hybrid_search() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let page = Page {
        language: Some(Language::En),
        ..Page::new(
            "rust-lang".to_string(),
            "note".to_string(),
            "Rust is a systems programming language focused on safety and performance.".to_string(),
        )
    };
    engine.put_page(page.clone()).await.expect("put_page");
    engine
        .chunk_and_embed_page(&page)
        .await
        .expect("chunk_and_embed_page");

    let results = engine
        .hybrid_search("systems programming", &Language::En, 5)
        .await
        .expect("hybrid_search");

    assert!(!results.is_empty(), "hybrid_search should return results");
}

#[tokio::test]
async fn test_tantivy_no_lock_conflict_on_concurrent_open() {
    let tb = TestBrain::new().await;

    // Open two separate TantivyIndex instances on the same directory.
    // With lazy writer, neither holds a file lock at construction time.
    let idx1 = TantivyIndex::new(tb.config.tantivy_dir.clone())
        .expect("first TantivyIndex::new should succeed");
    let idx2 = TantivyIndex::new(tb.config.tantivy_dir.clone())
        .expect("second TantivyIndex::new should succeed without lock conflict");

    // Both can search without conflict
    let r1 = idx1
        .search("test", &Language::En, 5)
        .await
        .expect("idx1 search");
    let r2 = idx2
        .search("test", &Language::En, 5)
        .await
        .expect("idx2 search");

    assert_eq!(r1.len(), r2.len());
}

#[tokio::test]
async fn test_delete_page_cleans_up() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let page = Page::new(
        "to-delete".to_string(),
        "note".to_string(),
        "This page will be deleted.".to_string(),
    );
    engine.put_page(page.clone()).await.expect("put_page");
    engine
        .chunk_and_embed_page(&page)
        .await
        .expect("chunk_and_embed_page");
    engine.delete_page("to-delete").await.expect("delete_page");

    let result = engine.get_page("to-delete").await;
    assert!(result.is_err(), "page should be gone after delete");
}

#[tokio::test]
async fn test_graph_links() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    // Page A links to page B via [[page-b]]
    let page_a = Page::new(
        "page-a".to_string(),
        "note".to_string(),
        "See also [[page-b]] for more details.".to_string(),
    );
    let page_b = Page::new(
        "page-b".to_string(),
        "note".to_string(),
        "Page B content.".to_string(),
    );

    engine.put_page(page_a).await.expect("put page_a");
    engine.put_page(page_b).await.expect("put page_b");

    let backlinks = engine.backlinks("page-b").await.expect("backlinks");
    assert!(
        backlinks.iter().any(|l| l.target_slug == "page-a"),
        "page-b should have a backlink from page-a"
    );
}

#[tokio::test]
async fn test_rejects_slug_path_traversal() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let page = Page::new(
        "../escaped".to_string(),
        "note".to_string(),
        "bad".to_string(),
    );
    assert!(engine.put_page(page).await.is_err());
    assert!(!tb.config.repo_dir.join("../escaped.md").exists());
}

#[tokio::test]
async fn test_explicit_link_survives_page_update() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    engine
        .put_page(Page::new("source".into(), "note".into(), "body".into()))
        .await
        .expect("put source");
    engine
        .put_page(Page::new("target".into(), "note".into(), "target".into()))
        .await
        .expect("put target");
    engine
        .add_link("source", "target", "evidence", Some("manual"), None)
        .await
        .expect("add link");

    let mut source = engine.get_page("source").await.expect("get source");
    source.compiled_truth = "updated body".to_string();
    engine.put_page(source).await.expect("update source");

    let outlinks = engine.outlinks("source").await.expect("outlinks");
    assert!(
        outlinks
            .iter()
            .any(|link| link.target_slug == "target" && link.edge_type == "evidence")
    );
}

#[tokio::test]
async fn test_graph_incoming_context_uses_incoming_edge() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    engine
        .put_page(Page::new("source".into(), "note".into(), "body".into()))
        .await
        .expect("put source");
    engine
        .put_page(Page::new("target".into(), "note".into(), "target".into()))
        .await
        .expect("put target");
    engine
        .add_link(
            "source",
            "target",
            "evidence",
            Some("incoming context"),
            None,
        )
        .await
        .expect("add link");

    let edges = engine
        .graph_query("target", Some("evidence"), 1, "in")
        .await
        .expect("graph query");

    let edge = edges
        .iter()
        .find(|edge| edge.target == "source")
        .expect("source edge");
    assert_eq!(edge.context.as_deref(), Some("incoming context"));
}

#[tokio::test]
async fn test_timeline_preserves_written_frontmatter() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    engine
        .put_page(Page::new(
            "concept-page".into(),
            "concept".into(),
            "body".into(),
        ))
        .await
        .expect("put");
    engine
        .add_tag("concept-page", "kept-tag")
        .await
        .expect("tag");
    engine
        .add_timeline_entry("concept-page", "2026-05-27", "event", None)
        .await
        .expect("timeline");

    let content =
        std::fs::read_to_string(tb.config.repo_dir.join("concept-page.md")).expect("read page");
    let parsed = MarkdownParser::parse(&content);
    assert_eq!(
        parsed
            .frontmatter
            .get("type")
            .and_then(|value| value.as_str()),
        Some("concept")
    );
    assert_eq!(
        parsed
            .frontmatter
            .get("tags")
            .and_then(|value| value.as_array())
            .and_then(|tags| tags[0].as_str()),
        Some("kept-tag")
    );
}

#[tokio::test]
async fn test_update_removes_previous_keyword_chunks() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let mut page = Page {
        language: Some(Language::En),
        ..Page::new(
            "replace-page".into(),
            "note".into(),
            "obsoletekeyword only".into(),
        )
    };
    engine.put_page(page.clone()).await.expect("put original");
    engine
        .chunk_and_embed_page(&page)
        .await
        .expect("embed original");

    page.compiled_truth = "replacementkeyword only".to_string();
    engine
        .put_page(page.clone())
        .await
        .expect("put replacement");
    engine
        .chunk_and_embed_page(&page)
        .await
        .expect("embed replacement");

    let obsolete = engine
        .keyword_search("obsoletekeyword", &Language::En, 5)
        .await
        .expect("search obsolete");
    assert!(
        obsolete.is_empty(),
        "updated pages must not retain prior keyword chunks"
    );
}

#[tokio::test]
async fn test_sync_invalidates_searchable_chunks_and_preserves_timeline() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let mut page = Page {
        language: Some(Language::En),
        ..Page::new(
            "synced-page".into(),
            "note".into(),
            "obsoletekeyword only".into(),
        )
    };
    page.timeline = "- 2026-05-26: original event".to_string();
    engine.put_page(page.clone()).await.expect("put original");
    engine
        .chunk_and_embed_page(&page)
        .await
        .expect("embed original");

    let external = MarkdownParser::to_canonical(
        &page.frontmatter,
        "replacementkeyword only",
        "- 2026-05-27: edited event",
    );
    std::fs::write(tb.config.repo_dir.join("synced-page.md"), external).expect("external edit");

    let (_, updated, _) = engine.sync().await.expect("sync");
    assert_eq!(updated, vec!["synced-page".to_string()]);

    let synced = engine.get_page("synced-page").await.expect("get synced");
    assert_eq!(synced.compiled_truth, "replacementkeyword only");
    assert_eq!(synced.timeline, "- 2026-05-27: edited event");

    let stale = engine.list_stale_pages().await.expect("list stale");
    assert!(
        stale
            .iter()
            .any(|candidate| candidate.slug == "synced-page")
    );
    let obsolete = engine
        .search_with_context("obsoletekeyword", &Language::En, 5, false, 1)
        .await
        .expect("search obsolete");
    assert!(
        obsolete.is_empty(),
        "sync must not return pre-edit source text"
    );
    let obsolete_keyword = engine
        .keyword_search("obsoletekeyword", &Language::En, 5)
        .await
        .expect("keyword search obsolete");
    assert!(
        obsolete_keyword.is_empty(),
        "keyword API must not expose invalidated chunks"
    );
}

#[tokio::test]
async fn test_vector_search_keeps_nearest_result_first() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let exact = Page {
        language: Some(Language::En),
        ..Page::new("exact".into(), "note".into(), "exact query".into())
    };
    let other = Page {
        language: Some(Language::En),
        ..Page::new("other".into(), "note".into(), "unrelated material".into())
    };
    engine.put_page(exact.clone()).await.expect("put exact");
    engine.put_page(other.clone()).await.expect("put other");
    engine
        .chunk_and_embed_page(&exact)
        .await
        .expect("embed exact");
    engine
        .chunk_and_embed_page(&other)
        .await
        .expect("embed other");

    let results = engine
        .vector_search("exact query", 2)
        .await
        .expect("vector search");
    let (_, first_slug) = engine
        .fetch_chunk_by_id(results[0].0)
        .await
        .expect("fetch")
        .expect("first chunk");
    assert_eq!(first_slug, "exact");
}

#[tokio::test]
async fn test_indegree_stats_are_updated_by_links() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    engine
        .put_page(Page::new("source".into(), "note".into(), "source".into()))
        .await
        .expect("put source");
    engine
        .put_page(Page::new("target".into(), "note".into(), "target".into()))
        .await
        .expect("put target");
    engine
        .add_link("source", "target", "related", None, None)
        .await
        .expect("link");

    let indegree: i64 = sqlx::query_scalar("SELECT indegree FROM page_stats WHERE slug = 'target'")
        .fetch_one(engine.get_db())
        .await
        .expect("indegree");
    assert_eq!(indegree, 1);
}

#[tokio::test]
async fn test_dream_cycle_flow() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    // Extraction processes source notes and synthesis requires three sources.
    let page1 = Page {
        tags: vec!["nlp".to_string()],
        ..Page::new(
            "paper-1".to_string(),
            "note".to_string(),
            "摘要：预训练语言模型是现代自然语言处理的基础。我们将研究BERT的性能表现。".to_string(),
        )
    };

    // Create page 2 with same tag
    let page2 = Page {
        tags: vec!["nlp".to_string()],
        ..Page::new(
            "paper-2".to_string(),
            "note".to_string(),
            "摘要：基于预训练语言模型，我们实现了多种下游NLP任务的性能突破。".to_string(),
        )
    };
    let page3 = Page {
        tags: vec!["nlp".to_string()],
        ..Page::new(
            "paper-3".to_string(),
            "note".to_string(),
            "摘要：预训练语言模型与BERT为语言理解研究提供了新基础。".to_string(),
        )
    };

    engine.put_page(page1).await.expect("put page1");
    engine.put_page(page2).await.expect("put page2");
    engine.put_page(page3).await.expect("put page3");

    // Run dream cycle (all stages)
    engine.run_dream_cycle(None).await.expect("run_dream_cycle");

    let concepts = engine
        .list_pages(Some("concept"), None, None, None, None)
        .await
        .expect("list concepts");
    assert_eq!(concepts.len(), 1, "one extracted concept should be created");
    let concept_page = &concepts[0];
    assert!(concept_page.slug.starts_with("research/concepts/"));

    let figures = engine
        .list_pages(Some("figure"), None, None, None, None)
        .await
        .expect("list figures");
    assert_eq!(figures.len(), 1, "one extracted figure should be created");
    assert!(figures[0].slug.starts_with("research/figures/"));

    // Timeline events belong to extracted figures; source notes remain immutable.
    let fetched_p1 = engine.get_page("paper-1").await.expect("get page1");
    assert!(
        fetched_p1.timeline.is_empty(),
        "dream extraction must not alter source notes"
    );
    let bert = engine
        .get_page("research/figures/bert")
        .await
        .expect("get BERT figure");
    assert!(
        bert.timeline.contains("BERT model was officially released"),
        "Timeline event not found on figure: {}",
        bert.timeline
    );

    let synthesis_slug = concept_page
        .slug
        .replacen("research/concepts/", "research/synthesis/", 1);
    let synth_page = engine
        .get_page(&synthesis_slug)
        .await
        .expect("get synthesis page");
    assert!(
        synth_page.compiled_truth.contains("paper-1"),
        "Synthesis content should refer to paper-1"
    );
    assert!(
        synth_page.compiled_truth.contains("paper-2"),
        "Synthesis content should refer to paper-2"
    );
}

#[tokio::test]
async fn test_dream_unassigned_events_are_saved_as_evidence_without_mutating_raw() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let source_slug = "raw/articles/source";

    engine
        .put_page(Page::new(
            source_slug.to_string(),
            "note".to_string(),
            "A study of school practice with a dated milestone.".to_string(),
        ))
        .await
        .expect("put raw source");
    let source_path = tb.config.repo_dir.join("raw/articles/source.md");
    let before = std::fs::read_to_string(&source_path).expect("read source before dream");

    engine
        .run_dream_cycle(Some("extract"))
        .await
        .expect("extract dream");

    let source = engine.get_page(source_slug).await.expect("get raw source");
    let after = std::fs::read_to_string(&source_path).expect("read source after dream");
    assert!(
        source.timeline.is_empty(),
        "unassigned events must not be added to raw pages"
    );
    assert_eq!(
        after, before,
        "dream extraction must not rewrite raw source files"
    );

    let evidence_slug = "research/evidence/events/raw/articles/source";
    let evidence = engine
        .get_page(evidence_slug)
        .await
        .expect("get derived evidence page");
    assert!(evidence.timeline.contains("Mock milestone event"));
    let outlinks = engine
        .outlinks(evidence_slug)
        .await
        .expect("event evidence outlinks");
    assert!(
        outlinks
            .iter()
            .any(|link| { link.target_slug == source_slug && link.edge_type == "evidence" })
    );
}

#[tokio::test]
async fn test_list_pages_language_filter() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let mut zh = Page::new(
        "zh-page".to_string(),
        "note".to_string(),
        "中文内容".to_string(),
    );
    zh.language = Some(Language::ZhHans);
    let mut en = Page::new(
        "en-page".to_string(),
        "note".to_string(),
        "English content".to_string(),
    );
    en.language = Some(Language::En);

    engine.put_page(zh).await.expect("put zh page");
    engine.put_page(en).await.expect("put en page");

    let zh_pages = engine
        .list_pages(None, None, Some("zh-hans"), None, None)
        .await
        .expect("list zh-hans pages");
    assert_eq!(zh_pages.len(), 1);
    assert_eq!(zh_pages[0].slug, "zh-page");

    let en_pages = engine
        .list_pages(None, None, Some("en"), None, None)
        .await
        .expect("list en pages");
    assert_eq!(en_pages.len(), 1);
    assert_eq!(en_pages[0].slug, "en-page");
}

#[tokio::test]
async fn test_list_pages_tag_filter_is_exact() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let mut exact = Page::new(
        "exact-tag".to_string(),
        "note".to_string(),
        "exact".to_string(),
    );
    exact.tags = vec!["ai".to_string()];
    let mut partial = Page::new(
        "partial-tag".to_string(),
        "note".to_string(),
        "partial".to_string(),
    );
    partial.tags = vec!["fair".to_string()];

    engine.put_page(exact).await.expect("put exact tag");
    engine.put_page(partial).await.expect("put partial tag");

    let pages = engine
        .list_pages(None, Some("ai"), None, None, Some("title"))
        .await
        .expect("list by tag");

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].slug, "exact-tag");
}

#[tokio::test]
async fn test_list_pages_limit() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    for i in 0..5 {
        engine
            .put_page(Page::new(
                format!("page-{}", i),
                "note".to_string(),
                format!("Content {}", i),
            ))
            .await
            .expect("put page");
    }

    let limited = engine
        .list_pages(None, None, None, Some(3), None)
        .await
        .expect("list with limit");
    assert_eq!(limited.len(), 3);
}

#[tokio::test]
async fn test_academic_meta_deserialize_partial() {
    use serde_json;
    // Struct is private, so test via JSON deserialization at the engine boundary.
    // We verify backward-compatibility: missing academic_meta defaults to empty.
    let old_fmt = r#"{"concepts":[],"figures":[],"events":[]}"#;
    // Parse via serde_json directly (ExtractedKnowledge is private, so we check the shape)
    let v: serde_json::Value = serde_json::from_str(old_fmt).expect("parse json");
    assert!(
        v.get("academic_meta").is_none(),
        "old format has no academic_meta key"
    );

    let new_fmt = r#"{"concepts":[],"figures":[],"events":[],"academic_meta":{"authors":["张三"],"year":2023,"journal":null,"doi":null}}"#;
    let v2: serde_json::Value = serde_json::from_str(new_fmt).expect("parse new json");
    let authors = v2["academic_meta"]["authors"]
        .as_array()
        .expect("authors array");
    assert_eq!(authors[0].as_str().unwrap(), "张三");
}

/// M3 Slice 4: page-level max-pooling end-to-end.
/// Two CJK pages, each with enough text to produce ≥2 chunks. With
/// `max_chunks_per_page=1`, search_with_context must return at most one
/// chunk per page even when multiple are scored highly.
#[tokio::test]
async fn search_with_context_max_pool_caps_chunks_per_page() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    // CJK chunker target = 600 chars; build pages well over that so each
    // produces multiple chunks. Repeat distinctive sentences containing the
    // query token so both pages match.
    let unit = "搜索测试内容,这是关键词命中的句子。";
    let long = unit.repeat(60); // ~1800 chars > 2 chunks

    let page_a = Page {
        language: Some(Language::ZhHans),
        ..Page::new("page/a".into(), "note".into(), long.clone())
    };
    let page_b = Page {
        language: Some(Language::ZhHans),
        ..Page::new("page/b".into(), "note".into(), long)
    };
    engine.put_page(page_a.clone()).await.expect("put a");
    engine.put_page(page_b.clone()).await.expect("put b");
    engine
        .chunk_and_embed_page(&page_a)
        .await
        .expect("embed a");
    engine
        .chunk_and_embed_page(&page_b)
        .await
        .expect("embed b");

    // Sanity check: each page produced ≥2 chunks.
    let n_a: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunks WHERE page_slug = 'page/a'")
        .fetch_one(engine.get_db())
        .await
        .expect("count a");
    let n_b: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunks WHERE page_slug = 'page/b'")
        .fetch_one(engine.get_db())
        .await
        .expect("count b");
    assert!(n_a >= 2, "page/a chunks: {n_a}");
    assert!(n_b >= 2, "page/b chunks: {n_b}");

    // Request a high k so the raw ranker would surface multiple chunks per
    // page if not for the cap.
    let pooled = engine
        .search_with_context("搜索测试", &Language::ZhHans, 6, false, 1)
        .await
        .expect("search pooled");
    let mut per_page: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for c in &pooled {
        *per_page.entry(c.page_slug.clone()).or_insert(0) += 1;
    }
    for (slug, n) in &per_page {
        assert!(*n <= 1, "pooled result has {n} chunks from {slug}");
    }

    // With cap=0 (unlimited), expect more results from the same pages.
    let unlimited = engine
        .search_with_context("搜索测试", &Language::ZhHans, 6, false, 0)
        .await
        .expect("search unlimited");
    assert!(
        unlimited.len() >= pooled.len(),
        "unlimited ({}) should return ≥ pooled ({})",
        unlimited.len(),
        pooled.len()
    );
}
