//! `fixtures/data_analysis_demo` — end-to-end M1 acceptance test.
//!
//! Simulates ZeroClaw walking the `data_analysis` protocol:
//!
//!   1. `brain_create_research_run`           — run row + research_run page.
//!   2. `brain_register_input(kind=dataset)`  — registers a CSV-like dataset.
//!   3. `brain_register_input(kind=artifact)` — registers a result table.
//!   4. `brain_record(kind=finding)`          — records a finding (status=claim)
//!                                              and links it to the artifact.
//!   5. `brain_validate_research_run`         — all three validators pass.
//!   6. `brain_get_research_protocol`         — protocol advances to
//!                                              `compose_research_memo`.
//!
//! Uses MockEmbedder — no API keys required.

mod common;

use common::TestBrain;
use rbrain_core::embedder::Embedder;
use rbrain_core::page::Page;
use rbrain_engine::Engine;
use rbrain_engine::evidence::{
    SuggestedAction, ValidatorStatus, analysis_plan_exists, artifact_hash_present,
    dataset_registered, finding_has_dataset_lineage, finding_has_supporting_artifact,
};
use rbrain_engine::research::{ProtocolState, ResearchRunStore, RunStatus, TaskType, protocol};
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

/// Helper: persist a page with a typed frontmatter so validators can read
/// `hash`, `status`, etc.
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
    let body = format!("---\ntype: {page_type}\ntitle: {title}\n---\n\n# {title}\n\n_fixture_\n");
    let mut page = Page::new(slug.to_string(), page_type.to_string(), body);
    page.frontmatter = serde_json::Value::Object(fm);
    page.title = title.to_string();
    engine.put_page(page).await.expect("put_page");
}

#[tokio::test]
async fn data_analysis_protocol_runs_end_to_end() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let store = ResearchRunStore::new(engine.get_db());

    // ── Step 1: create_research_run ─────────────────────────────────────────
    let run_slug = "research/runs/cohort-2026q2";
    // The page must exist first because research_runs.slug references pages(slug).
    put_typed_page(
        &engine,
        run_slug,
        "research_run",
        "Cohort 2026 Q2 baseline",
        Default::default(),
    )
    .await;
    let run = store
        .create(None, run_slug, TaskType::DataAnalysis, "fixture")
        .await
        .expect("create run");
    assert_eq!(run.status, RunStatus::Planned);

    // ── Step 2: register_input(dataset) ─────────────────────────────────────
    let dataset_slug = "research/datasets/cohort-baseline-csv";
    let mut ds_fm = serde_json::Map::new();
    ds_fm.insert("path".into(), serde_json::json!("data/cohort_baseline.csv"));
    ds_fm.insert(
        "abs_path_snapshot".into(),
        serde_json::json!("/tmp/data/cohort_baseline.csv"),
    );
    ds_fm.insert("hash".into(), serde_json::json!("sha256:deadbeef"));
    ds_fm.insert("size_bytes".into(), serde_json::json!(1024));
    ds_fm.insert("format".into(), serde_json::json!("csv"));
    ds_fm.insert("research_run".into(), serde_json::json!(run_slug));
    put_typed_page(
        &engine,
        dataset_slug,
        "dataset",
        "Cohort baseline CSV",
        ds_fm,
    )
    .await;
    engine
        .add_link(run_slug, dataset_slug, "uses_dataset", None, None)
        .await
        .expect("link run→dataset");

    // ── Step 3: record an analysis plan ─────────────────────────────────────
    let plan_slug = "research/plans/cohort-analysis-plan";
    let mut plan_fm = serde_json::Map::new();
    plan_fm.insert("research_run".into(), serde_json::json!(run_slug));
    put_typed_page(
        &engine,
        plan_slug,
        "analysis_plan",
        "Cohort descriptive analysis plan",
        plan_fm,
    )
    .await;
    engine
        .add_link(run_slug, plan_slug, "tests_hypothesis", None, None)
        .await
        .expect("link run→analysis_plan");

    // ── Step 4: register_input(artifact) — a result table ──────────────────
    let artifact_slug = "research/artifacts/cohort-descriptive-stats";
    let mut art_fm = serde_json::Map::new();
    art_fm.insert("path".into(), serde_json::json!("outputs/desc_stats.csv"));
    art_fm.insert("hash".into(), serde_json::json!("sha256:cafef00d"));
    art_fm.insert("artifact_kind".into(), serde_json::json!("result_table"));
    art_fm.insert("research_run".into(), serde_json::json!(run_slug));
    art_fm.insert("stored_copy".into(), serde_json::json!(true));
    put_typed_page(
        &engine,
        artifact_slug,
        "artifact",
        "Cohort descriptive stats table",
        art_fm,
    )
    .await;
    engine
        .add_link(run_slug, artifact_slug, "produces", None, None)
        .await
        .expect("link run→artifact");
    // Also link artifact back to dataset for evidence_check's 2-hop walk.
    engine
        .add_link(artifact_slug, dataset_slug, "derived_from", None, None)
        .await
        .expect("link artifact→dataset");

    // ── Step 5: record finding (status = claim) linked to the artifact ─────
    let finding_slug = "research/findings/cohort-attrition-rises";
    let mut find_fm = serde_json::Map::new();
    find_fm.insert("status".into(), serde_json::json!("claim"));
    find_fm.insert("research_run".into(), serde_json::json!(run_slug));
    put_typed_page(
        &engine,
        finding_slug,
        "finding",
        "Attrition rises in Q2 cohort",
        find_fm,
    )
    .await;
    engine
        .add_link(run_slug, finding_slug, "produces", None, None)
        .await
        .expect("link run→finding");
    engine
        .add_link(finding_slug, artifact_slug, "supports", None, None)
        .await
        .expect("link finding→artifact");

    // ── Step 6: validators — all protocol-backed data-analysis checks pass ──
    let pool = engine.get_db();
    let v1 = dataset_registered(pool, run_slug).await.expect("v1");
    let v2 = analysis_plan_exists(pool, run_slug).await.expect("v2");
    let v3 = artifact_hash_present(pool, run_slug).await.expect("v3");
    let v4 = finding_has_dataset_lineage(pool, run_slug)
        .await
        .expect("v4");
    let v5 = finding_has_supporting_artifact(pool, run_slug)
        .await
        .expect("v5");
    assert_eq!(
        v1.status,
        ValidatorStatus::Pass,
        "{}: {}",
        v1.validator,
        v1.message
    );
    assert_eq!(
        v2.status,
        ValidatorStatus::Pass,
        "{}: {}",
        v2.validator,
        v2.message
    );
    assert_eq!(
        v3.status,
        ValidatorStatus::Pass,
        "{}: {}",
        v3.validator,
        v3.message
    );
    assert_eq!(
        v4.status,
        ValidatorStatus::Pass,
        "{}: {}",
        v4.validator,
        v4.message
    );
    assert_eq!(
        v5.status,
        ValidatorStatus::Pass,
        "{}: {}",
        v5.validator,
        v5.message
    );

    // ── Step 7: protocol state advances ─────────────────────────────────────
    // Manually flip the status as validate_research_run would.
    let run = store
        .set_status(&run.id, RunStatus::Running)
        .await
        .expect("→running");
    let state: ProtocolState = protocol::derive_state(&run, &[v1, v2, v3, v4, v5]);

    assert!(
        state
            .completed_steps
            .contains(&"create_research_run".to_string()),
        "completed={:?}",
        state.completed_steps
    );
    assert!(
        state
            .completed_steps
            .contains(&"register_dataset".to_string()),
        "completed={:?}",
        state.completed_steps
    );
    assert_eq!(
        state.current_step, "compose_research_memo",
        "expected final handoff step; got {}",
        state.current_step
    );
    assert!(
        state.blocking_validators.is_empty(),
        "no validator should be in fail state; got {:?}",
        state.blocking_validators
    );
}

#[tokio::test]
async fn finding_support_must_target_artifact() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let store = ResearchRunStore::new(engine.get_db());

    let run_slug = "research/runs/non-artifact-support";
    put_typed_page(
        &engine,
        run_slug,
        "research_run",
        "Non-artifact support run",
        Default::default(),
    )
    .await;
    store
        .create(None, run_slug, TaskType::DataAnalysis, "fixture")
        .await
        .expect("create");

    let finding_slug = "research/findings/claim-supports-note";
    let mut find_fm = serde_json::Map::new();
    find_fm.insert("status".into(), serde_json::json!("claim"));
    find_fm.insert("research_run".into(), serde_json::json!(run_slug));
    put_typed_page(
        &engine,
        finding_slug,
        "finding",
        "Claim supports a note",
        find_fm,
    )
    .await;
    engine
        .add_link(run_slug, finding_slug, "produces", None, None)
        .await
        .expect("link run→finding");

    let note_slug = "research/notes/not-an-artifact";
    put_typed_page(
        &engine,
        note_slug,
        "note",
        "Not an artifact",
        Default::default(),
    )
    .await;
    engine
        .add_link(finding_slug, note_slug, "supports", None, None)
        .await
        .expect("link finding→note");

    let v = finding_has_supporting_artifact(engine.get_db(), run_slug)
        .await
        .expect("v");
    assert_eq!(v.status, ValidatorStatus::Fail);
}

#[tokio::test]
async fn validators_emit_typed_actions_for_missing_dataset() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let store = ResearchRunStore::new(engine.get_db());

    let run_slug = "research/runs/empty";
    put_typed_page(
        &engine,
        run_slug,
        "research_run",
        "Empty run",
        Default::default(),
    )
    .await;
    store
        .create(None, run_slug, TaskType::DataAnalysis, "fixture")
        .await
        .expect("create");

    let v = dataset_registered(engine.get_db(), run_slug)
        .await
        .expect("v");
    assert_eq!(v.status, ValidatorStatus::Fail);
    assert_eq!(v.suggested_actions.len(), 1);
    matches!(
        v.suggested_actions[0],
        SuggestedAction::RegisterDataset { .. }
    );
}

#[tokio::test]
async fn draft_finding_only_warns_not_fails() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let store = ResearchRunStore::new(engine.get_db());

    let run_slug = "research/runs/drafting";
    put_typed_page(
        &engine,
        run_slug,
        "research_run",
        "Drafting run",
        Default::default(),
    )
    .await;
    store
        .create(None, run_slug, TaskType::DataAnalysis, "fixture")
        .await
        .expect("create");

    // Draft finding with no supports outlink.
    let finding_slug = "research/findings/draft-hunch";
    let mut fm = serde_json::Map::new();
    fm.insert("status".into(), serde_json::json!("draft"));
    fm.insert("research_run".into(), serde_json::json!(run_slug));
    put_typed_page(&engine, finding_slug, "finding", "Draft hunch", fm).await;
    engine
        .add_link(run_slug, finding_slug, "produces", None, None)
        .await
        .expect("link");

    let v = finding_has_supporting_artifact(engine.get_db(), run_slug)
        .await
        .expect("v");
    assert_eq!(
        v.status,
        ValidatorStatus::Warn,
        "draft findings without supports must be warn, not fail: {}",
        v.message
    );
}

#[tokio::test]
async fn analysis_plan_exists_fails_without_plan_and_emits_typed_action() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let store = ResearchRunStore::new(engine.get_db());

    let run_slug = "research/runs/no-plan";
    put_typed_page(
        &engine,
        run_slug,
        "research_run",
        "No-plan run",
        Default::default(),
    )
    .await;
    store
        .create(None, run_slug, TaskType::DataAnalysis, "fixture")
        .await
        .expect("create");

    let v = analysis_plan_exists(engine.get_db(), run_slug)
        .await
        .expect("v");
    assert_eq!(v.status, ValidatorStatus::Fail);
    assert_eq!(v.suggested_actions.len(), 1);
    assert!(matches!(
        &v.suggested_actions[0],
        SuggestedAction::RecordAnalysisPlan { run_slug: s } if s == run_slug
    ));
}

#[tokio::test]
async fn analysis_plan_exists_passes_after_tests_hypothesis_link() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let store = ResearchRunStore::new(engine.get_db());

    let run_slug = "research/runs/with-plan";
    put_typed_page(
        &engine,
        run_slug,
        "research_run",
        "With-plan run",
        Default::default(),
    )
    .await;
    store
        .create(None, run_slug, TaskType::DataAnalysis, "fixture")
        .await
        .expect("create");

    let plan_slug = "research/plans/sample";
    put_typed_page(
        &engine,
        plan_slug,
        "analysis_plan",
        "Sample plan",
        Default::default(),
    )
    .await;
    engine
        .add_link(run_slug, plan_slug, "tests_hypothesis", None, None)
        .await
        .expect("link");

    let v = analysis_plan_exists(engine.get_db(), run_slug)
        .await
        .expect("v");
    assert_eq!(v.status, ValidatorStatus::Pass);
    assert!(v.suggested_actions.is_empty());
}

#[tokio::test]
async fn finding_has_dataset_lineage_rejects_uses_dataset_on_artifact_edge() {
    // Phase 3 contract: only `derived_from` is the artifact→dataset edge.
    // If the test graph mis-uses `uses_dataset` from artifact to dataset, the
    // validator must NOT count it as lineage.
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let store = ResearchRunStore::new(engine.get_db());

    let run_slug = "research/runs/bad-lineage-edge";
    put_typed_page(
        &engine,
        run_slug,
        "research_run",
        "Bad-lineage-edge run",
        Default::default(),
    )
    .await;
    store
        .create(None, run_slug, TaskType::DataAnalysis, "fixture")
        .await
        .expect("create");

    let dataset_slug = "research/datasets/d1";
    let mut ds_fm = serde_json::Map::new();
    ds_fm.insert("hash".into(), serde_json::json!("sha256:1"));
    put_typed_page(&engine, dataset_slug, "dataset", "D1", ds_fm).await;

    let artifact_slug = "research/artifacts/a1";
    let mut art_fm = serde_json::Map::new();
    art_fm.insert("hash".into(), serde_json::json!("sha256:2"));
    art_fm.insert("artifact_kind".into(), serde_json::json!("result_table"));
    put_typed_page(&engine, artifact_slug, "artifact", "A1", art_fm).await;

    let finding_slug = "research/findings/f1";
    let mut find_fm = serde_json::Map::new();
    find_fm.insert("status".into(), serde_json::json!("claim"));
    put_typed_page(&engine, finding_slug, "finding", "F1", find_fm).await;

    engine
        .add_link(run_slug, finding_slug, "produces", None, None)
        .await
        .expect("run→finding");
    engine
        .add_link(run_slug, dataset_slug, "uses_dataset", None, None)
        .await
        .expect("run→dataset");
    engine
        .add_link(finding_slug, artifact_slug, "supports", None, None)
        .await
        .expect("finding→artifact");
    // Wrong edge type — semantically uses_dataset is run→dataset, not artifact→dataset.
    engine
        .add_link(artifact_slug, dataset_slug, "uses_dataset", None, None)
        .await
        .expect("artifact-(wrong)→dataset");

    let v = finding_has_dataset_lineage(engine.get_db(), run_slug)
        .await
        .expect("v");
    assert_eq!(
        v.status,
        ValidatorStatus::Fail,
        "uses_dataset on artifact edge must NOT count as dataset lineage: {}",
        v.message
    );
}

#[tokio::test]
async fn finding_has_dataset_lineage_passes_with_derived_from() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let store = ResearchRunStore::new(engine.get_db());

    let run_slug = "research/runs/good-lineage";
    put_typed_page(
        &engine,
        run_slug,
        "research_run",
        "Good-lineage run",
        Default::default(),
    )
    .await;
    store
        .create(None, run_slug, TaskType::DataAnalysis, "fixture")
        .await
        .expect("create");

    let dataset_slug = "research/datasets/d2";
    let mut ds_fm = serde_json::Map::new();
    ds_fm.insert("hash".into(), serde_json::json!("sha256:3"));
    put_typed_page(&engine, dataset_slug, "dataset", "D2", ds_fm).await;

    let artifact_slug = "research/artifacts/a2";
    let mut art_fm = serde_json::Map::new();
    art_fm.insert("hash".into(), serde_json::json!("sha256:4"));
    art_fm.insert("artifact_kind".into(), serde_json::json!("result_table"));
    put_typed_page(&engine, artifact_slug, "artifact", "A2", art_fm).await;

    let finding_slug = "research/findings/f2";
    let mut find_fm = serde_json::Map::new();
    find_fm.insert("status".into(), serde_json::json!("claim"));
    put_typed_page(&engine, finding_slug, "finding", "F2", find_fm).await;

    engine
        .add_link(run_slug, finding_slug, "produces", None, None)
        .await
        .expect("run→finding");
    engine
        .add_link(run_slug, dataset_slug, "uses_dataset", None, None)
        .await
        .expect("run→dataset");
    engine
        .add_link(finding_slug, artifact_slug, "supports", None, None)
        .await
        .expect("finding→artifact");
    engine
        .add_link(artifact_slug, dataset_slug, "derived_from", None, None)
        .await
        .expect("artifact→dataset");

    let v = finding_has_dataset_lineage(engine.get_db(), run_slug)
        .await
        .expect("v");
    assert_eq!(v.status, ValidatorStatus::Pass);
}

#[tokio::test]
async fn finding_has_dataset_lineage_rejects_dataset_not_registered_on_run() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let store = ResearchRunStore::new(engine.get_db());

    let run_slug = "research/runs/wrong-dataset";
    put_typed_page(
        &engine,
        run_slug,
        "research_run",
        "Wrong-dataset run",
        Default::default(),
    )
    .await;
    store
        .create(None, run_slug, TaskType::DataAnalysis, "fixture")
        .await
        .expect("create");

    let registered_dataset = "research/datasets/registered";
    let mut reg_fm = serde_json::Map::new();
    reg_fm.insert("hash".into(), serde_json::json!("sha256:registered"));
    put_typed_page(&engine, registered_dataset, "dataset", "Registered", reg_fm).await;

    let other_dataset = "research/datasets/other";
    let mut other_fm = serde_json::Map::new();
    other_fm.insert("hash".into(), serde_json::json!("sha256:other"));
    put_typed_page(&engine, other_dataset, "dataset", "Other", other_fm).await;

    let artifact_slug = "research/artifacts/wrong-dataset-result";
    let mut art_fm = serde_json::Map::new();
    art_fm.insert("hash".into(), serde_json::json!("sha256:result"));
    art_fm.insert("artifact_kind".into(), serde_json::json!("result_table"));
    put_typed_page(&engine, artifact_slug, "artifact", "Wrong dataset result", art_fm).await;

    let finding_slug = "research/findings/wrong-dataset-finding";
    let mut find_fm = serde_json::Map::new();
    find_fm.insert("status".into(), serde_json::json!("claim"));
    put_typed_page(&engine, finding_slug, "finding", "Wrong dataset finding", find_fm).await;

    engine
        .add_link(run_slug, registered_dataset, "uses_dataset", None, None)
        .await
        .expect("run→registered dataset");
    engine
        .add_link(run_slug, finding_slug, "produces", None, None)
        .await
        .expect("run→finding");
    engine
        .add_link(finding_slug, artifact_slug, "supports", None, None)
        .await
        .expect("finding→artifact");
    engine
        .add_link(artifact_slug, other_dataset, "derived_from", None, None)
        .await
        .expect("artifact→other dataset");

    let v = finding_has_dataset_lineage(engine.get_db(), run_slug)
        .await
        .expect("v");
    assert_eq!(
        v.status,
        ValidatorStatus::Fail,
        "artifact lineage must use a dataset registered on the run: {}",
        v.message
    );
}

#[tokio::test]
async fn finding_has_dataset_lineage_draft_only_warns() {
    let tb = TestBrain::new().await;
    let engine = open_mock_engine(&tb).await;
    let store = ResearchRunStore::new(engine.get_db());

    let run_slug = "research/runs/draft-lineage";
    put_typed_page(
        &engine,
        run_slug,
        "research_run",
        "Draft-lineage run",
        Default::default(),
    )
    .await;
    store
        .create(None, run_slug, TaskType::DataAnalysis, "fixture")
        .await
        .expect("create");

    // Draft finding, no artifact/dataset lineage at all.
    let finding_slug = "research/findings/draft-no-lineage";
    let mut find_fm = serde_json::Map::new();
    find_fm.insert("status".into(), serde_json::json!("draft"));
    put_typed_page(
        &engine,
        finding_slug,
        "finding",
        "Draft no-lineage",
        find_fm,
    )
    .await;
    engine
        .add_link(run_slug, finding_slug, "produces", None, None)
        .await
        .expect("link");

    let v = finding_has_dataset_lineage(engine.get_db(), run_slug)
        .await
        .expect("v");
    assert_eq!(
        v.status,
        ValidatorStatus::Warn,
        "draft finding without lineage must be warn, not fail: {}",
        v.message
    );
}
