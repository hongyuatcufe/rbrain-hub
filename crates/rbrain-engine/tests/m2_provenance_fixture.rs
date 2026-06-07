//! M2 acceptance — evidence_check + provenance_of integration tests.
//!
//! Validates the two M2 deliverables:
//!
//!   1. `brain_evidence_check` follows the real graph (no 1-hop stub).
//!   2. `brain_provenance_of` enumerates incoming + outgoing research edges
//!      so ZeroClaw can answer "which dataset/script produced this result?".

mod common;

use common::TestBrain;
use rbrain_core::embedder::Embedder;
use rbrain_core::page::Page;
use rbrain_engine::evidence::{
    EvidenceReport, ProvenanceReport, ValidatorStatus, provenance_of, run_evidence_check,
};
use rbrain_engine::Engine;
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

async fn put_typed_page(
    engine: &Engine,
    slug: &str,
    page_type: &str,
    title: &str,
    fm_extra: serde_json::Map<String, serde_json::Value>,
) {
    let mut fm = serde_json::Map::new();
    fm.insert("type".into(), serde_json::json!(page_type));
    fm.insert("title".into(), serde_json::json!(title));
    for (k, v) in fm_extra {
        fm.insert(k, v);
    }
    let body = format!("# {title}\n\n_fixture_\n");
    let mut page = Page::new(slug.to_string(), page_type.to_string(), body);
    page.frontmatter = serde_json::Value::Object(fm);
    page.title = title.to_string();
    engine.put_page(page).await.expect("put_page");
}

#[tokio::test]
async fn evidence_check_data_analysis_chain_resolves_dataset_and_script() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let dataset = "research/datasets/d";
    let script = "research/scripts/s.py";
    let artifact = "research/artifacts/a";
    let finding = "research/findings/f";
    put_typed_page(&engine, dataset, "dataset", "D", Default::default()).await;
    put_typed_page(&engine, script, "script", "S", Default::default()).await;
    put_typed_page(&engine, artifact, "artifact", "A", Default::default()).await;
    put_typed_page(&engine, finding, "finding", "F", Default::default()).await;

    engine.add_link(finding, artifact, "supports", None, None).await.unwrap();
    engine.add_link(artifact, dataset, "derived_from", None, None).await.unwrap();
    engine.add_link(artifact, script, "computed_by", None, None).await.unwrap();

    let report: EvidenceReport = run_evidence_check(engine.get_db(), finding).await.unwrap();
    assert_eq!(report.validator.status, ValidatorStatus::Pass);
    assert_eq!(report.chain.datasets, vec![dataset.to_string()]);
    assert_eq!(report.chain.scripts, vec![script.to_string()]);
    assert_eq!(report.chain.direct_support.len(), 1);
    assert_eq!(report.chain.direct_support[0].page_type, "artifact");
}

#[tokio::test]
async fn evidence_check_literature_shape_passes_via_cites() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let source = "raw/articles/x";
    let finding = "research/findings/lit-f";
    put_typed_page(&engine, source, "raw", "Source X", Default::default()).await;
    put_typed_page(&engine, finding, "finding", "Lit finding", Default::default()).await;

    engine.add_link(finding, source, "cites", None, None).await.unwrap();

    let report = run_evidence_check(engine.get_db(), finding).await.unwrap();
    assert_eq!(report.validator.status, ValidatorStatus::Pass);
    assert_eq!(report.chain.literature_sources, vec![source.to_string()]);
}

#[tokio::test]
async fn evidence_check_artifact_without_dataset_warns_not_fails() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let artifact = "research/artifacts/orphan";
    let finding = "research/findings/orphan-f";
    put_typed_page(&engine, artifact, "artifact", "A", Default::default()).await;
    put_typed_page(&engine, finding, "finding", "F", Default::default()).await;
    engine.add_link(finding, artifact, "supports", None, None).await.unwrap();

    let report = run_evidence_check(engine.get_db(), finding).await.unwrap();
    assert_eq!(report.validator.status, ValidatorStatus::Warn);
    assert!(report.chain.datasets.is_empty());
}

#[tokio::test]
async fn evidence_check_no_supports_or_cites_fails_with_link_action() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let finding = "research/findings/lonely";
    put_typed_page(&engine, finding, "finding", "Lonely", Default::default()).await;

    let report = run_evidence_check(engine.get_db(), finding).await.unwrap();
    assert_eq!(report.validator.status, ValidatorStatus::Fail);
    assert_eq!(report.validator.suggested_actions.len(), 1);
}

#[tokio::test]
async fn provenance_of_artifact_lists_incoming_and_outgoing_research_edges() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let dataset = "research/datasets/dp";
    let script = "research/scripts/sp.py";
    let artifact = "research/artifacts/ap";
    let finding = "research/findings/fp";
    let run = "research/runs/runp";
    put_typed_page(&engine, dataset, "dataset", "Dp", Default::default()).await;
    put_typed_page(&engine, script, "script", "Sp", Default::default()).await;
    put_typed_page(&engine, artifact, "artifact", "Ap", Default::default()).await;
    put_typed_page(&engine, finding, "finding", "Fp", Default::default()).await;
    put_typed_page(&engine, run, "research_run", "Run", Default::default()).await;

    // Outgoing from artifact:
    engine.add_link(artifact, dataset, "derived_from", None, None).await.unwrap();
    engine.add_link(artifact, script, "computed_by", None, None).await.unwrap();
    // Incoming to artifact:
    engine.add_link(finding, artifact, "supports", None, None).await.unwrap();
    engine.add_link(finding, artifact, "contradicts", None, None).await.unwrap();
    engine.add_link(run, artifact, "produces", None, None).await.unwrap();
    // Non-research edge that must be filtered out:
    engine.add_link(artifact, dataset, "mentions", None, None).await.unwrap();

    let report: ProvenanceReport = provenance_of(engine.get_db(), artifact).await.unwrap();
    assert_eq!(report.page_type, "artifact");

    let outgoing: Vec<_> = report.edges.iter().filter(|e| !e.incoming).collect();
    let incoming: Vec<_> = report.edges.iter().filter(|e| e.incoming).collect();

    // Outgoing: derived_from + computed_by (mentions excluded).
    let out_types: Vec<&str> = outgoing.iter().map(|e| e.edge_type.as_str()).collect();
    assert!(out_types.contains(&"derived_from"), "out_types={:?}", out_types);
    assert!(out_types.contains(&"computed_by"), "out_types={:?}", out_types);
    assert!(!out_types.contains(&"mentions"), "out_types={:?}", out_types);

    // Incoming: supports + contradicts + produces.
    let in_types: Vec<&str> = incoming.iter().map(|e| e.edge_type.as_str()).collect();
    assert!(in_types.contains(&"supports"), "in_types={:?}", in_types);
    assert!(in_types.contains(&"contradicts"), "in_types={:?}", in_types);
    assert!(in_types.contains(&"produces"), "in_types={:?}", in_types);
}

#[tokio::test]
async fn provenance_of_unknown_slug_errors() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let r = provenance_of(engine.get_db(), "research/missing").await;
    assert!(r.is_err());
}

#[tokio::test]
async fn evidence_check_unknown_slug_errors() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let r = run_evidence_check(engine.get_db(), "research/findings/missing").await;
    assert!(r.is_err());
}

#[tokio::test]
async fn evidence_check_non_finding_slug_errors() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;

    let note = "research/notes/not-a-finding";
    put_typed_page(&engine, note, "note", "Not a finding", Default::default()).await;

    let r = run_evidence_check(engine.get_db(), note).await;
    assert!(r.is_err());
}
