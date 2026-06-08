use rbrain_core::page::Page;
use rbrain_engine::Engine;
use rbrain_engine::evidence::{
    ValidatorResult, analysis_plan_exists, artifact_hash_present, dataset_registered,
    finding_has_dataset_lineage, finding_has_supporting_artifact, run_citation_check,
};
use rbrain_engine::pipeline::{InputSpec, OutputMode, PipelineStep, PromptSpec, ResponseFormat};
use rbrain_engine::research::{
    ProtocolState, ResearchRun, ResearchRunStore, RunStatus, TaskType, protocol::derive_state,
};
use rmcp::{
    handler::server::wrapper::{Json, Parameters},
    schemars::{self, JsonSchema},
    tool, tool_router,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Clone)]
pub struct RBrainMcpServer {
    engine: Engine,
}

impl RBrainMcpServer {
    pub fn new(engine: Engine) -> Self {
        Self { engine }
    }
}

// ── Argument types ─────────────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct QueryArgs {
    /// Search query (any language)
    pub query: String,
    /// Max results to return (default 10, max 50)
    pub limit: Option<i32>,
    /// Use LLM query expansion for better recall (default false)
    pub expand: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetArgs {
    /// Page slug (URL-style identifier)
    pub slug: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct PutArgs {
    /// Page slug
    pub slug: String,
    /// Markdown content
    pub content: String,
    /// Page type: "note", "wiki", "book", etc. (default "note")
    pub page_type: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteArgs {
    /// Page slug to delete
    pub slug: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListArgs {
    /// Filter by page type (e.g. "wiki", "note", "book")
    pub page_type: Option<String>,
    /// Filter by tag
    pub tag: Option<String>,
    /// Filter by language code: "zh-hans", "zh-hant", "en", "ja", "ko"
    pub language: Option<String>,
    /// Maximum number of results (default: no limit, clamped to 1–200)
    pub limit: Option<i64>,
    /// Sort field: "updated_at" (default), "created_at", or "title"
    pub sort_by: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GraphArgs {
    /// Starting page slug
    pub slug: String,
    /// Edge type filter (e.g. "references", "mentions")
    pub edge_type: Option<String>,
    /// Traversal depth (default 2, max 5)
    pub depth: Option<i32>,
    /// Direction: "out" (outgoing), "in" (incoming), "both"
    pub direction: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct BacklinksArgs {
    /// Target page slug — find all pages linking to this page
    pub slug: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct LinkArgs {
    /// Source page slug (the page making the claim or reference)
    pub from: String,
    /// Target page slug (the source being cited or related page)
    pub to: String,
    /// Relationship type: evidence, related, person, period, supports, contrasts, develops, mentions, references
    pub link_type: Option<String>,
    /// Chunk ID from brain_query results — auto-captures that passage as evidence context
    pub chunk_id: Option<i64>,
    /// Free-text context note (used when chunk_id is not provided)
    pub context: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct UnlinkArgs {
    pub from: String,
    pub to: String,
    /// Remove only links of this type (omit to remove all types between these pages)
    pub link_type: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ThinkArgs {
    /// Topic to reason about
    pub topic: String,
    /// Number of context chunks (default 12, max 20)
    pub limit: Option<i32>,
    /// Use LLM query expansion for better recall (default false)
    pub expand: Option<bool>,
    /// Override the expected JSON/Markdown output schema injected into the prompt via {response_schema}.
    /// Leave empty to use the prompt file's default structure.
    /// Example: '{"核心论点":"string","最强反驳":"string","回应策略":"string"}'
    pub response_schema: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TimelineArgs {
    /// Page slug to append the timeline entry to
    pub slug: String,
    /// Text description of the event or finding
    pub text: String,
    /// Date in YYYY-MM-DD format (default: today)
    pub date: Option<String>,
    /// Source reference (e.g. "book-slug chunk:150")
    pub source: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TagArgs {
    /// Page slug
    pub slug: String,
    /// Tag to add or remove
    pub tag: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct OutlinksArgs {
    /// Source page slug — find all pages this page links to
    pub slug: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ProcessArgs {
    /// Filter for input pages (page_type required)
    pub page_type: String,
    /// Optional tag filter
    pub tag: Option<String>,
    /// Optional language filter (e.g. "zh-hans")
    pub language: Option<String>,
    /// Specific page slugs to process (overrides page_type/tag/language filter)
    pub slugs: Option<Vec<String>>,
    /// Max number of pages to process (default: all)
    pub limit: Option<i32>,

    /// Task prompt — what to do with each page (inline text or "file:<name>" to load from prompts dir)
    pub task_prompt: String,
    /// Expected response format: "json" (default) or "markdown"
    pub response_format: Option<String>,
    /// JSON schema hint for the LLM (guides output structure and enables retry)
    pub output_schema: Option<String>,

    /// Output mode: "return" (default), "save", or "update_frontmatter"
    pub output_mode: Option<String>,
    /// For "save" mode: page type for output pages
    pub output_page_type: Option<String>,
    /// For "save" mode: slug prefix for output pages (e.g. "research/processed/")
    pub output_slug_prefix: Option<String>,
    /// For "save" mode: whether to embed output pages
    pub embed_output: Option<bool>,
}

#[derive(Serialize, JsonSchema)]
pub struct ProcessResult {
    pub ok: bool,
    pub count: usize,
    pub results: Vec<serde_json::Value>,
    pub message: String,
}

// ── Result types ────────────────────────────────────────────────────────────

#[derive(Serialize, JsonSchema)]
pub struct ChunkResult {
    pub page_slug: String,
    pub chunk_id: i64,
    pub score: f64,
    /// Chunk text (up to ~500 chars preview)
    pub text: String,
}

/// Wrapper for Vec<ChunkResult> — MCP spec requires root type 'object'
#[derive(Serialize, JsonSchema)]
pub struct ChunkList {
    pub results: Vec<ChunkResult>,
}

#[derive(Serialize, JsonSchema)]
pub struct PageResult {
    pub slug: String,
    pub title: String,
    pub page_type: String,
    pub tags: Vec<String>,
    pub language: Option<String>,
    pub compiled_truth: String,
    pub timeline: String,
    pub updated_at: String,
}

#[derive(Serialize, JsonSchema)]
pub struct PageSummary {
    pub slug: String,
    pub title: String,
    pub page_type: String,
    pub language: Option<String>,
    pub updated_at: String,
    /// First ~160 chars of body content, stripped of markdown formatting
    pub snippet: String,
}

/// Wrapper for Vec<PageSummary>
#[derive(Serialize, JsonSchema)]
pub struct PageList {
    pub results: Vec<PageSummary>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GenerateArgs {
    /// Topic to generate a page about
    pub topic: String,
    /// Number of context chunks to use (default 8, max 20)
    pub limit: Option<i32>,
    /// Save the result as a page in the knowledge base (default false)
    pub save: Option<bool>,
    /// Use LLM query expansion for better recall (default false)
    pub expand: Option<bool>,
    /// Prompt template name: loads compose_{template}.md from prompts dir.
    /// Built-in: "wiki". Custom: "policy_brief", "review_article", "thesis_section", etc.
    pub template: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct GenerateResult {
    /// Generated Markdown wiki content
    pub content: String,
    /// Slug of the saved page, if save=true
    pub saved_as: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct GraphEdge {
    pub target: String,
    pub edge_type: String,
    pub depth: usize,
}

/// Wrapper for Vec<GraphEdge>
#[derive(Serialize, JsonSchema)]
pub struct GraphList {
    pub results: Vec<GraphEdge>,
}

#[derive(Serialize, JsonSchema)]
pub struct LinkRef {
    pub page_slug: String,
    pub edge_type: String,
    pub context: Option<String>,
    pub chunk_id: Option<i64>,
}

/// Wrapper for Vec<LinkRef>
#[derive(Serialize, JsonSchema)]
pub struct LinkList {
    pub results: Vec<LinkRef>,
}

/// Wrapper for Vec<String> (orphan slugs)
#[derive(Serialize, JsonSchema)]
pub struct SlugList {
    pub slugs: Vec<String>,
}

/// Wrapper for brain_get (Option<PageResult> is not a valid root object)
#[derive(Serialize, JsonSchema)]
pub struct GetResult {
    /// true if the page was found
    pub found: bool,
    pub page: Option<PageResult>,
}

/// Wrapper for brain_think (String is not a valid root object)
#[derive(Serialize, JsonSchema)]
pub struct ThinkResult {
    pub reasoning: String,
}

#[derive(Serialize, JsonSchema)]
pub struct StatsResult {
    pub total_pages: i64,
    pub total_chunks: i64,
    pub embedding_coverage_pct: f64,
    pub graph_density: f64,
    pub pages_by_type: std::collections::HashMap<String, i64>,
    pub pages_by_language: std::collections::HashMap<String, i64>,
}

#[derive(Serialize, JsonSchema)]
pub struct MutationResult {
    pub ok: bool,
    pub message: String,
}

impl MutationResult {
    fn ok(msg: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: msg.into(),
        }
    }
    fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: msg.into(),
        }
    }
}

// ── Research-run argument & result types (M1) ──────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct CreateResearchRunArgs {
    /// Human-readable title rendered into the research_run page.
    pub title: String,
    /// Page slug for the run (e.g. "research/runs/2026-06-07-cohort"). If
    /// omitted, derived from title.
    pub slug: Option<String>,
    /// `literature_review` | `data_analysis` | `mixed_methods` | `theory_building`.
    pub task_type: String,
    /// Optional caller-provided uuid. Generated if omitted.
    pub run_id: Option<String>,
    /// Optional initial research question to embed in the page body.
    pub research_question: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct ResearchRunResult {
    pub ok: bool,
    pub run_id: String,
    pub slug: String,
    pub task_type: String,
    pub status: String,
    pub message: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct GetProtocolArgs {
    pub run_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct RegisterInputArgs {
    /// `dataset` | `artifact`
    pub kind: String,
    /// Page slug for this input.
    pub slug: String,
    /// Human title.
    pub title: String,
    /// File path (relative to ZeroClaw workspace).
    pub path: String,
    /// Absolute path captured at registration time.
    pub abs_path_snapshot: Option<String>,
    /// sha256 of the file contents.
    pub hash: String,
    /// Size in bytes.
    pub size_bytes: Option<i64>,
    /// For dataset: csv|xlsx|json|parquet|sql|other.
    /// For artifact: script|log|result_table|chart|model_output|report|notebook.
    pub kind_subtype: Option<String>,
    /// Mime type, optional.
    pub mime_type: Option<String>,
    /// research_run slug this input belongs to.
    pub run_slug: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct RecordArgs {
    /// `finding` | `limitation` | `analysis_plan`
    pub kind: String,
    pub slug: String,
    pub title: String,
    /// Markdown body.
    pub content: String,
    /// research_run slug.
    pub run_slug: String,
    /// For finding: draft|claim|validated. Defaults to draft.
    pub status: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ValidateRunArgs {
    pub run_id: String,
}

#[derive(Serialize, JsonSchema)]
pub struct ValidateRunResult {
    pub ok: bool,
    pub run_id: String,
    pub overall: String,
    pub validators: Vec<ValidatorJson>,
    pub protocol: Option<ProtocolJson>,
}

#[derive(Serialize, JsonSchema)]
pub struct ValidatorJson {
    pub validator: String,
    pub status: String,
    pub message: String,
    pub affected_slugs: Vec<String>,
    pub suggested_actions: Vec<serde_json::Value>,
}

#[derive(Serialize, JsonSchema)]
pub struct ProtocolJson {
    pub run_id: String,
    pub task_type: String,
    pub current_step: String,
    pub completed_steps: Vec<String>,
    pub next_actions: Vec<serde_json::Value>,
    pub blocking_validators: Vec<ValidatorJson>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CitationCheckArgs {
    pub slug: String,
}

#[derive(Serialize, JsonSchema)]
pub struct CitationCheckResultJson {
    pub ok: bool,
    pub slug: String,
    pub validator: ValidatorJson,
}

#[derive(Deserialize, JsonSchema)]
pub struct EvidenceCheckArgs {
    pub finding_slug: String,
}

#[derive(Serialize, JsonSchema)]
pub struct EvidenceCheckResultJson {
    pub ok: bool,
    pub finding_slug: String,
    pub validator: ValidatorJson,
    /// EvidenceChain JSON (direct_support, datasets, scripts, literature_sources)
    /// — present when ok=true, null on traversal error.
    pub chain: serde_json::Value,
}

#[derive(Deserialize, JsonSchema)]
pub struct ProvenanceArgs {
    pub slug: String,
}

#[derive(Serialize, JsonSchema)]
pub struct ProvenanceResultJson {
    pub ok: bool,
    pub slug: String,
    pub page_type: String,
    /// Array of ProvenanceEdge entries: { edge_type, neighbour_slug, neighbour_page_type, incoming }.
    pub edges: serde_json::Value,
    pub message: String,
}

fn validator_to_json(v: &ValidatorResult) -> ValidatorJson {
    ValidatorJson {
        validator: v.validator.clone(),
        status: match v.status {
            rbrain_engine::evidence::ValidatorStatus::Pass => "pass".into(),
            rbrain_engine::evidence::ValidatorStatus::Warn => "warn".into(),
            rbrain_engine::evidence::ValidatorStatus::Fail => "fail".into(),
        },
        message: v.message.clone(),
        affected_slugs: v.affected_slugs.clone(),
        suggested_actions: v
            .suggested_actions
            .iter()
            .map(|a| serde_json::to_value(a).unwrap_or(serde_json::Value::Null))
            .collect(),
    }
}

fn protocol_to_json(p: &ProtocolState) -> ProtocolJson {
    ProtocolJson {
        run_id: p.run_id.clone(),
        task_type: p.task_type.to_string(),
        current_step: p.current_step.clone(),
        completed_steps: p.completed_steps.clone(),
        next_actions: p
            .next_actions
            .iter()
            .map(|a| {
                serde_json::json!({
                    "action": serde_json::to_value(&a.action).unwrap_or(serde_json::Value::Null),
                    "reason": a.reason,
                })
            })
            .collect(),
        blocking_validators: p
            .blocking_validators
            .iter()
            .map(validator_to_json)
            .collect(),
    }
}

fn build_run_page_body(title: &str, _task_type: &str, question: Option<&str>) -> String {
    let mut body = format!("# {title}\n\n");
    if let Some(q) = question {
        body.push_str("## Research question\n\n");
        body.push_str(q);
        body.push_str("\n");
    }
    body
}

fn page_with_frontmatter(
    slug: String,
    page_type: &str,
    title: &str,
    frontmatter: serde_json::Map<String, serde_json::Value>,
    body: String,
) -> Page {
    let mut page = Page::new(slug, page_type.to_string(), body);
    page.frontmatter = serde_json::Value::Object(frontmatter);
    page.title = title.to_string();
    page
}

/// Deserialize the snapshot persisted by `validate_research_run`. Reserved for
/// future audit / history tooling. **Not** used by protocol state derivation —
/// see `validators_for_protocol` for why.
#[allow(dead_code)]
fn stored_validator_summary(run: &ResearchRun) -> Option<Vec<ValidatorResult>> {
    run.last_validation_summary
        .as_deref()
        .and_then(|summary| serde_json::from_str(summary).ok())
}

fn push_validator_result<E: std::fmt::Display>(
    name: &'static str,
    validators: &mut Vec<ValidatorResult>,
    result: std::result::Result<ValidatorResult, E>,
) {
    match result {
        Ok(v) => validators.push(v),
        Err(e) => {
            tracing::warn!("validator {name} failed: {e}");
            validators.push(ValidatorResult::fail(name, format!("db error: {e}")));
        }
    }
}

async fn run_research_validators(
    pool: &sqlx::SqlitePool,
    run: &ResearchRun,
) -> Vec<ValidatorResult> {
    let mut validators = Vec::new();
    match run.task_type {
        TaskType::DataAnalysis => {
            push_validator_result("dataset_registered", &mut validators, dataset_registered(pool, &run.slug).await);
            push_validator_result("analysis_plan_exists", &mut validators, analysis_plan_exists(pool, &run.slug).await);
            push_validator_result(
                "artifact_hash_present",
                &mut validators,
                artifact_hash_present(pool, &run.slug).await,
            );
            push_validator_result(
                "finding_has_dataset_lineage",
                &mut validators,
                finding_has_dataset_lineage(pool, &run.slug).await,
            );
            push_validator_result(
                "finding_has_supporting_artifact",
                &mut validators,
                finding_has_supporting_artifact(pool, &run.slug).await,
            );
        }
        TaskType::MixedMethods | TaskType::TheoryBuilding => {
            push_validator_result("analysis_plan_exists", &mut validators, analysis_plan_exists(pool, &run.slug).await);
            push_validator_result(
                "artifact_hash_present",
                &mut validators,
                artifact_hash_present(pool, &run.slug).await,
            );
            push_validator_result(
                "finding_has_supporting_artifact",
                &mut validators,
                finding_has_supporting_artifact(pool, &run.slug).await,
            );
        }
        TaskType::LiteratureReview => {
            validators.push(ValidatorResult::warn(
                "literature_validators",
                "not implemented until M3 — no validators run for this task_type",
            ));
        }
    }
    validators
}

/// Protocol state is derived **live** from the current registered state — never
/// from `research_runs.last_validation_summary`. The stored summary is kept
/// only for audit / history (read it via `stored_validator_summary` when you
/// genuinely want the snapshot at the moment of the last `validate` call).
///
/// Rationale: between a `validate_research_run` call and a subsequent
/// `get_research_protocol` call, ZeroClaw may have called `register_input` /
/// `record` / `add_link`. Returning the cached summary would make the protocol
/// recommend actions ZeroClaw has already completed. Validators are cheap SQL
/// counts (no LLM), so recomputing is correct and inexpensive.
async fn validators_for_protocol(
    pool: &sqlx::SqlitePool,
    run: &ResearchRun,
) -> Vec<ValidatorResult> {
    run_research_validators(pool, run).await
}

async fn ensure_research_run_slug(
    engine: &Engine,
    run_slug: &str,
) -> std::result::Result<(), String> {
    let store = ResearchRunStore::new(engine.get_db());
    match store.find_by_slug(run_slug).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(format!("research_run not found for slug: {run_slug}")),
        Err(e) => Err(format!("research_run lookup failed: {e}")),
    }
}

fn derive_run_slug(title: &str, given: Option<&str>) -> String {
    if let Some(s) = given {
        return s.to_string();
    }
    let mut prev_dash = false;
    let cleaned: String = title
        .chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() {
                prev_dash = false;
                Some(c.to_ascii_lowercase())
            } else if !prev_dash {
                prev_dash = true;
                Some('-')
            } else {
                None
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    format!("research/runs/{}", trimmed)
}

// ── MCP Tools ───────────────────────────────────────────────────────────────

#[tool_router(server_handler)]
impl RBrainMcpServer {
    /// Hybrid search (vector + keyword + RRF). Returns ranked chunks with text.
    /// Use this to find relevant content before writing or synthesising wiki pages.
    #[tool(
        name = "brain_query",
        description = "Hybrid search combining vector similarity and keyword search (RRF fusion). \
            Returns ranked text chunks with source page. Use before writing wiki pages or answering questions."
    )]
    async fn query(&self, Parameters(args): Parameters<QueryArgs>) -> Json<ChunkList> {
        let limit = args.limit.map(|l| l.clamp(1, 50) as usize).unwrap_or(10);
        let expand = args.expand.unwrap_or(false);
        let lang = rbrain_core::page::Language::detect(&args.query);

        match self
            .engine
            .search_with_context(&args.query, &lang, limit, expand)
            .await
        {
            Ok(chunks) => Json(ChunkList {
                results: chunks
                    .into_iter()
                    .map(|c| ChunkResult {
                        page_slug: c.page_slug,
                        chunk_id: c.chunk_id,
                        score: c.score,
                        text: c.text,
                    })
                    .collect(),
            }),
            Err(e) => {
                tracing::error!("brain_query error: {}", e);
                Json(ChunkList { results: vec![] })
            }
        }
    }

    /// Get a page by slug. Returns full content including compiled_truth and timeline.
    #[tool(
        name = "brain_get",
        description = "Retrieve a page by its slug. Returns the full Markdown content, tags, and metadata."
    )]
    async fn get(&self, Parameters(args): Parameters<GetArgs>) -> Json<GetResult> {
        match self.engine.get_page(&args.slug).await {
            Ok(page) => Json(GetResult {
                found: true,
                page: Some(PageResult {
                    slug: page.slug,
                    title: page.title,
                    page_type: page.page_type,
                    tags: page.tags,
                    language: page.language.as_ref().map(|l| l.to_string()),
                    compiled_truth: page.compiled_truth,
                    timeline: page.timeline,
                    updated_at: page.updated_at.to_string(),
                }),
            }),
            Err(_) => Json(GetResult {
                found: false,
                page: None,
            }),
        }
    }

    /// Create or update a page.
    #[tool(
        name = "brain_put",
        description = "Create or update a page in the knowledge base. Use page_type 'wiki' for synthesised summaries, \
            'note' for raw notes, 'book' for imported books."
    )]
    async fn put(&self, Parameters(args): Parameters<PutArgs>) -> Json<MutationResult> {
        let page = Page::new(
            args.slug,
            args.page_type.unwrap_or_else(|| "note".to_string()),
            args.content,
        );
        match self.engine.put_page(page).await {
            Ok(_) => Json(MutationResult::ok("Page saved")),
            Err(e) => Json(MutationResult::err(format!("Failed: {}", e))),
        }
    }

    /// Delete a page and all its chunks/embeddings.
    #[tool(
        name = "brain_delete",
        description = "Delete a page and all its associated chunks and embeddings."
    )]
    async fn delete(&self, Parameters(args): Parameters<DeleteArgs>) -> Json<MutationResult> {
        match self.engine.delete_page(&args.slug).await {
            Ok(_) => Json(MutationResult::ok("Page deleted")),
            Err(e) => Json(MutationResult::err(format!("Failed: {}", e))),
        }
    }

    /// List pages with optional filters.
    #[tool(
        name = "brain_list",
        description = "List pages with optional filters. Supports: page_type, tag, language \
            (zh-hans/zh-hant/en/ja/ko), limit (max 200), sort_by (updated_at/created_at/title). \
            Returns summaries with 160-char snippet."
    )]
    async fn list(&self, Parameters(args): Parameters<ListArgs>) -> Json<PageList> {
        match self
            .engine
            .list_pages(
                args.page_type.as_deref(),
                args.tag.as_deref(),
                args.language.as_deref(),
                args.limit,
                args.sort_by.as_deref(),
            )
            .await
        {
            Ok(pages) => Json(PageList {
                results: pages
                    .into_iter()
                    .map(|p| PageSummary {
                        snippet: rbrain_core::markdown::MarkdownParser::extract_snippet(
                            &p.compiled_truth,
                            160,
                        ),
                        slug: p.slug,
                        title: p.title,
                        page_type: p.page_type,
                        language: p.language.as_ref().map(|l| l.to_string()),
                        updated_at: p.updated_at.to_string(),
                    })
                    .collect(),
            }),
            Err(_) => Json(PageList { results: vec![] }),
        }
    }

    /// Traverse the knowledge graph from a page.
    #[tool(
        name = "brain_graph",
        description = "Traverse the knowledge graph starting from a page. \
            direction='out' follows links this page makes, 'in' finds pages linking here, 'both' does both."
    )]
    async fn graph(&self, Parameters(args): Parameters<GraphArgs>) -> Json<GraphList> {
        let depth = args.depth.map(|d| d.clamp(1, 5) as usize).unwrap_or(2);
        let direction = args.direction.as_deref().unwrap_or("out");

        match self
            .engine
            .graph_query(&args.slug, args.edge_type.as_deref(), depth, direction)
            .await
        {
            Ok(edges) => Json(GraphList {
                results: edges
                    .into_iter()
                    .map(|e| GraphEdge {
                        target: e.target,
                        edge_type: e.edge_type,
                        depth: e.depth,
                    })
                    .collect(),
            }),
            Err(_) => Json(GraphList { results: vec![] }),
        }
    }

    /// Find all pages that link to a given page (backlinks).
    #[tool(
        name = "brain_backlinks",
        description = "Find all pages that link to the given page. Useful for discovering related content and context."
    )]
    async fn backlinks(&self, Parameters(args): Parameters<BacklinksArgs>) -> Json<LinkList> {
        match self.engine.backlinks(&args.slug).await {
            Ok(links) => Json(LinkList {
                results: links
                    .into_iter()
                    .map(|l| LinkRef {
                        page_slug: l.target_slug,
                        edge_type: l.edge_type,
                        context: l.context,
                        chunk_id: l.chunk_id,
                    })
                    .collect(),
            }),
            Err(_) => Json(LinkList { results: vec![] }),
        }
    }

    /// Get knowledge base statistics.
    #[tool(
        name = "brain_stats",
        description = "Get statistics about the knowledge base: total pages/chunks, embedding coverage, graph density."
    )]
    async fn stats(&self) -> Json<StatsResult> {
        match self.engine.get_stats().await {
            Ok(s) => Json(StatsResult {
                total_pages: s.total_pages(),
                total_chunks: s.total_chunks,
                embedding_coverage_pct: s.embedding_coverage,
                graph_density: s.graph_density,
                pages_by_type: s.pages_by_type.into_iter().map(|(k, v)| (k, v)).collect(),
                pages_by_language: s
                    .pages_by_language
                    .into_iter()
                    .map(|(k, v)| (k, v))
                    .collect(),
            }),
            Err(_) => Json(StatsResult {
                total_pages: 0,
                total_chunks: 0,
                embedding_coverage_pct: 0.0,
                graph_density: 0.0,
                pages_by_type: Default::default(),
                pages_by_language: Default::default(),
            }),
        }
    }

    /// Search relevant content and synthesise a wiki page using DeepSeek LLM.
    /// Use this to create or update wiki pages from existing knowledge base content.
    #[tool(
        name = "brain_generate",
        description = "Search the knowledge base for relevant content and synthesise a Markdown wiki page \
            using the DeepSeek LLM. Optionally saves the result back to the brain with save=true. \
            Requires deepseek.api_key to be configured."
    )]
    async fn generate(&self, Parameters(args): Parameters<GenerateArgs>) -> Json<GenerateResult> {
        let limit = args.limit.map(|l| l.clamp(1, 20) as usize).unwrap_or(8);
        let expand = args.expand.unwrap_or(false);
        let lang = rbrain_core::page::Language::detect(&args.topic);

        match self
            .engine
            .generate_wiki(&args.topic, &lang, limit, expand, args.template.as_deref())
            .await
        {
            Ok(wiki) => {
                let saved_as = if args.save.unwrap_or(false) {
                    let slug = args
                        .topic
                        .to_lowercase()
                        .replace(' ', "-")
                        .replace(['/', '\\', '.'], "-");
                    let page = Page::new(slug.clone(), "wiki".to_string(), wiki.clone());
                    match self.engine.put_page(page).await {
                        Ok(_) => Some(slug),
                        Err(e) => {
                            tracing::error!("brain_generate: failed to save page: {}", e);
                            None
                        }
                    }
                } else {
                    None
                };
                Json(GenerateResult {
                    content: wiki,
                    saved_as,
                })
            }
            Err(e) => {
                tracing::error!("brain_generate error: {}", e);
                Json(GenerateResult {
                    content: format!("Error: {}", e),
                    saved_as: None,
                })
            }
        }
    }

    /// Add a typed link between two pages, optionally anchored to a specific chunk as evidence.
    #[tool(
        name = "brain_link",
        description = "Create a typed directed link between two pages. \
            Use chunk_id (from brain_query results) to anchor the link to a specific passage as evidence — \
            the chunk text is automatically captured as context. \
            Generic graph link_type: evidence | related | person | period | contrasts | develops | mentions | references. \
            Research provenance link_type: uses_dataset | uses_variable | uses_method | computed_by | derived_from | \
            supports | contradicts | tests_hypothesis | cites | limits | produces | validates."
    )]
    async fn link(&self, Parameters(args): Parameters<LinkArgs>) -> Json<MutationResult> {
        let link_type = args.link_type.as_deref().unwrap_or("related");

        // Resolve context from chunk_id if provided
        let context: Option<String> = if let Some(chunk_id) = args.chunk_id {
            match self.engine.fetch_chunk_by_id(chunk_id).await {
                Ok(Some((text, _page_slug))) => Some(text),
                Ok(None) => {
                    return Json(MutationResult::err(format!(
                        "Chunk {} not found. Use chunk_id from brain_query results.",
                        chunk_id
                    )));
                }
                Err(e) => {
                    return Json(MutationResult::err(format!("Failed to fetch chunk: {}", e)));
                }
            }
        } else {
            args.context.clone()
        };

        match self
            .engine
            .add_link(
                &args.from,
                &args.to,
                link_type,
                context.as_deref(),
                args.chunk_id,
            )
            .await
        {
            Ok(_) => Json(MutationResult::ok(format!(
                "Link added: {} --[{}]--> {}{}",
                args.from,
                link_type,
                args.to,
                if args.chunk_id.is_some() {
                    format!(" (context from chunk:{})", args.chunk_id.unwrap())
                } else {
                    String::new()
                }
            ))),
            Err(e) => Json(MutationResult::err(format!("Failed: {}", e))),
        }
    }

    /// Remove a link between two pages.
    #[tool(
        name = "brain_unlink",
        description = "Remove a typed link between two pages. \
            Specify link_type to remove only that type; omit to remove all links between these pages."
    )]
    async fn unlink(&self, Parameters(args): Parameters<UnlinkArgs>) -> Json<MutationResult> {
        match self
            .engine
            .remove_link(&args.from, &args.to, args.link_type.as_deref())
            .await
        {
            Ok(n) if n > 0 => Json(MutationResult::ok(format!(
                "Removed {} link(s): {} --> {}",
                n, args.from, args.to
            ))),
            Ok(_) => Json(MutationResult::err(format!(
                "No matching link found: {} --> {}",
                args.from, args.to
            ))),
            Err(e) => Json(MutationResult::err(format!("Failed: {}", e))),
        }
    }

    /// List pages with no incoming links (orphan pages).
    #[tool(
        name = "brain_orphans",
        description = "Find pages that have no incoming links — not referenced by any other page. \
            Use to discover isolated knowledge that needs to be connected to the graph."
    )]
    async fn orphans(&self) -> Json<SlugList> {
        match self.engine.orphan_pages().await {
            Ok(slugs) => Json(SlugList { slugs }),
            Err(_) => Json(SlugList { slugs: vec![] }),
        }
    }

    /// Deep reasoning synthesis on a topic.
    #[tool(
        name = "brain_think",
        description = "Search the knowledge base for relevant chunks and run structured deep reasoning: \
            core claims, tensions & contradictions, working judgment, open questions. \
            Returns a Markdown reasoning artifact with inline citations. Requires deepseek.api_key."
    )]
    async fn think(&self, Parameters(args): Parameters<ThinkArgs>) -> Json<ThinkResult> {
        let lang = rbrain_core::page::Language::detect(&args.topic);
        let limit = args.limit.map(|l| l.clamp(1, 20) as usize).unwrap_or(12);
        let expand = args.expand.unwrap_or(false);
        match self
            .engine
            .think(
                &args.topic,
                &lang,
                limit,
                expand,
                args.response_schema.as_deref(),
            )
            .await
        {
            Ok(result) => Json(ThinkResult { reasoning: result }),
            Err(e) => Json(ThinkResult {
                reasoning: format!("Error: {}", e),
            }),
        }
    }

    /// Append a dated entry to a page's timeline.
    #[tool(
        name = "brain_add_timeline_entry",
        description = "Append a dated event or finding to a page's timeline log. \
            Use this to record when a scholar is mentioned, a claim is encountered, or a source is read. \
            date defaults to today (YYYY-MM-DD) if omitted."
    )]
    async fn add_timeline_entry(
        &self,
        Parameters(args): Parameters<TimelineArgs>,
    ) -> Json<MutationResult> {
        let date = args
            .date
            .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
        match self
            .engine
            .add_timeline_entry(&args.slug, &date, &args.text, args.source.as_deref())
            .await
        {
            Ok(_) => Json(MutationResult::ok(format!(
                "Timeline entry added to '{}'",
                args.slug
            ))),
            Err(e) => Json(MutationResult::err(format!("Failed: {}", e))),
        }
    }

    /// Add a tag to a page.
    #[tool(
        name = "brain_add_tag",
        description = "Add a tag to a page. Tags enable filtering with brain_list and brain_query."
    )]
    async fn add_tag(&self, Parameters(args): Parameters<TagArgs>) -> Json<MutationResult> {
        match self.engine.add_tag(&args.slug, &args.tag).await {
            Ok(_) => Json(MutationResult::ok(format!(
                "Tag '{}' added to '{}'",
                args.tag, args.slug
            ))),
            Err(e) => Json(MutationResult::err(format!("Failed: {}", e))),
        }
    }

    /// Remove a tag from a page.
    #[tool(name = "brain_remove_tag", description = "Remove a tag from a page.")]
    async fn remove_tag(&self, Parameters(args): Parameters<TagArgs>) -> Json<MutationResult> {
        match self.engine.remove_tag(&args.slug, &args.tag).await {
            Ok(_) => Json(MutationResult::ok(format!(
                "Tag '{}' removed from '{}'",
                args.tag, args.slug
            ))),
            Err(e) => Json(MutationResult::err(format!("Failed: {}", e))),
        }
    }

    /// Find all pages this page links to (outgoing links).
    #[tool(
        name = "brain_outlinks",
        description = "Find all pages that this page links to (outgoing links). \
            Complement to brain_backlinks. Shows what a page references or cites."
    )]
    async fn outlinks(&self, Parameters(args): Parameters<OutlinksArgs>) -> Json<LinkList> {
        match self.engine.outlinks(&args.slug).await {
            Ok(links) => Json(LinkList {
                results: links
                    .into_iter()
                    .map(|l| LinkRef {
                        page_slug: l.target_slug,
                        edge_type: l.edge_type,
                        context: l.context,
                        chunk_id: l.chunk_id,
                    })
                    .collect(),
            }),
            Err(_) => Json(LinkList { results: vec![] }),
        }
    }

    /// Batch LLM processing of knowledge base pages with a custom task prompt.
    #[tool(
        name = "brain_process",
        description = "Batch-process knowledge base pages with a custom LLM task. \
            Filters pages by type/tag/language/slugs, sends each through the task_prompt, \
            and returns results or saves them as new pages. \
            Examples: extract key passages, generate exam questions, annotate themes, \
            score evidence quality. Requires deepseek.api_key."
    )]
    async fn process(&self, Parameters(args): Parameters<ProcessArgs>) -> Json<ProcessResult> {
        let limit = args.limit.map(|l| l.clamp(1, 500) as usize);

        let prompt = if let Some(name) = args.task_prompt.strip_prefix("file:") {
            PromptSpec::File(name.to_string())
        } else {
            PromptSpec::Inline(args.task_prompt.clone())
        };

        let response_format = match args.response_format.as_deref().unwrap_or("json") {
            "markdown" => ResponseFormat::Markdown,
            _ => ResponseFormat::Json,
        };

        let output_mode = match args.output_mode.as_deref().unwrap_or("return") {
            "save" => OutputMode::SaveAs {
                page_type: args.output_page_type.unwrap_or_else(|| "note".to_string()),
                slug_prefix: args
                    .output_slug_prefix
                    .unwrap_or_else(|| "processed/".to_string()),
                embed: args.embed_output.unwrap_or(false),
            },
            "update_frontmatter" => OutputMode::UpdateFrontmatter,
            _ => OutputMode::Return,
        };

        let step = PipelineStep {
            id: "brain_process".to_string(),
            input: InputSpec::SelfContent {
                page_type: args.page_type,
                tag: args.tag,
                language: args.language,
                slugs: args.slugs,
            },
            prompt,
            output_schema: args.output_schema,
            response_format,
            output_mode,
            incremental: false,
            batch_size: 1,
            max_inputs: limit,
            inject_existing_titles: None,
        };

        match self.engine.run_pipeline_step(&step).await {
            Ok(results) => {
                let count = results.len();
                Json(ProcessResult {
                    ok: true,
                    count,
                    results,
                    message: format!("Processed {} page(s)", count),
                })
            }
            Err(e) => Json(ProcessResult {
                ok: false,
                count: 0,
                results: vec![],
                message: format!("Error: {}", e),
            }),
        }
    }

    // ── M1 research-run tools ──────────────────────────────────────────────

    #[tool(
        name = "brain_create_research_run",
        description = "Create a research run. Inserts a research_runs row (source of truth) and \
            a Markdown page (rendering layer). task_type ∈ literature_review|data_analysis|mixed_methods|theory_building."
    )]
    async fn create_research_run(
        &self,
        Parameters(args): Parameters<CreateResearchRunArgs>,
    ) -> Json<ResearchRunResult> {
        let task_type = match TaskType::from_str(&args.task_type) {
            Ok(t) => t,
            Err(e) => {
                return Json(ResearchRunResult {
                    ok: false,
                    run_id: String::new(),
                    slug: String::new(),
                    task_type: args.task_type,
                    status: String::new(),
                    message: format!("invalid task_type: {e}"),
                });
            }
        };
        let slug = derive_run_slug(&args.title, args.slug.as_deref());
        // 1. Persist the page first so research_runs.slug FK is satisfied.
        let body = build_run_page_body(
            &args.title,
            task_type.as_str(),
            args.research_question.as_deref(),
        );
        let mut fm = serde_json::Map::new();
        fm.insert("type".into(), serde_json::json!("research_run"));
        fm.insert("title".into(), serde_json::json!(args.title));
        fm.insert("task_type".into(), serde_json::json!(task_type.as_str()));
        fm.insert("created_by".into(), serde_json::json!("zeroclaw"));
        if let Some(q) = &args.research_question {
            fm.insert("research_question".into(), serde_json::json!(q));
        }
        let page = page_with_frontmatter(slug.clone(), "research_run", &args.title, fm, body);
        if let Err(e) = self.engine.put_page(page).await {
            return Json(ResearchRunResult {
                ok: false,
                run_id: String::new(),
                slug,
                task_type: task_type.to_string(),
                status: String::new(),
                message: format!("put_page failed: {e}"),
            });
        }
        // 2. Insert research_runs row.
        let store = ResearchRunStore::new(self.engine.get_db());
        match store
            .create(args.run_id.as_deref(), &slug, task_type, "zeroclaw")
            .await
        {
            Ok(run) => Json(ResearchRunResult {
                ok: true,
                run_id: run.id,
                slug: run.slug,
                task_type: run.task_type.to_string(),
                status: run.status.to_string(),
                message: "research_run created".into(),
            }),
            Err(e) => {
                // Compensate: remove the orphan page so DB and page store stay consistent.
                let _ = self.engine.delete_page(&slug).await;
                Json(ResearchRunResult {
                    ok: false,
                    run_id: String::new(),
                    slug,
                    task_type: task_type.to_string(),
                    status: String::new(),
                    message: format!("store.create failed: {e}"),
                })
            }
        }
    }

    #[tool(
        name = "brain_get_research_protocol",
        description = "Returns the protocol state machine for a research run: current step, completed steps, \
            next actions, and blocking validators. State is derived live from the latest validator pass — \
            ZeroClaw can resume cleanly after session interruption using just the run_id."
    )]
    async fn get_research_protocol(
        &self,
        Parameters(args): Parameters<GetProtocolArgs>,
    ) -> Json<ProtocolJson> {
        let store = ResearchRunStore::new(self.engine.get_db());
        let run = match store.get(&args.run_id).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("get_research_protocol: {e}");
                return Json(ProtocolJson {
                    run_id: args.run_id,
                    task_type: "unknown".into(),
                    current_step: "error".into(),
                    completed_steps: vec![],
                    next_actions: vec![],
                    blocking_validators: vec![],
                });
            }
        };
        let pool = self.engine.get_db();
        let vs = validators_for_protocol(pool, &run).await;
        Json(protocol_to_json(&derive_state(&run, &vs)))
    }

    #[tool(
        name = "brain_register_input",
        description = "Register an input (kind=dataset|artifact) to a research run. Stores hash + path snapshot \
            in page frontmatter, links it to the run via uses_dataset/produces. ZeroClaw calls this \
            immediately after producing or consuming a file."
    )]
    async fn register_input(
        &self,
        Parameters(args): Parameters<RegisterInputArgs>,
    ) -> Json<MutationResult> {
        if let Err(e) = ensure_research_run_slug(&self.engine, &args.run_slug).await {
            return Json(MutationResult::err(e));
        }

        let (page_type, edge_type) = match args.kind.as_str() {
            "dataset" => ("dataset", "uses_dataset"),
            "artifact" => ("artifact", "produces"),
            other => {
                return Json(MutationResult::err(format!(
                    "kind must be dataset|artifact, got {other}"
                )));
            }
        };

        let format_field = if args.kind == "dataset" {
            "format"
        } else {
            "artifact_kind"
        };
        let mut fm = serde_json::Map::new();
        fm.insert("type".into(), serde_json::json!(page_type));
        fm.insert("title".into(), serde_json::json!(args.title));
        fm.insert("path".into(), serde_json::json!(args.path));
        if let Some(abs) = &args.abs_path_snapshot {
            fm.insert("abs_path_snapshot".into(), serde_json::json!(abs));
        }
        fm.insert("hash".into(), serde_json::json!(args.hash));
        if let Some(sz) = args.size_bytes {
            fm.insert("size_bytes".into(), serde_json::json!(sz));
        }
        if let Some(k) = &args.kind_subtype {
            fm.insert(format_field.into(), serde_json::json!(k));
        }
        if let Some(m) = &args.mime_type {
            fm.insert("mime_type".into(), serde_json::json!(m));
        }
        fm.insert("research_run".into(), serde_json::json!(args.run_slug));

        let body = format!("# {}\n\nRegistered via brain_register_input.\n", args.title);
        let page = page_with_frontmatter(args.slug.clone(), page_type, &args.title, fm, body);
        if let Err(e) = self.engine.put_page(page).await {
            return Json(MutationResult::err(format!("put_page failed: {e}")));
        }
        // Link run → input
        if let Err(e) = self
            .engine
            .add_link(&args.run_slug, &args.slug, edge_type, None, None)
            .await
        {
            return Json(MutationResult::err(format!(
                "add_link {edge_type} failed: {e}"
            )));
        }
        Json(MutationResult::ok(format!(
            "{} registered and linked via {edge_type}",
            args.kind
        )))
    }

    #[tool(
        name = "brain_record",
        description = "Record a discrete research fact (kind=finding|limitation|analysis_plan) and link it to a run. \
            For finding, set status=draft|claim|validated to control validator severity (draft → warn only)."
    )]
    async fn record(&self, Parameters(args): Parameters<RecordArgs>) -> Json<MutationResult> {
        if let Err(e) = ensure_research_run_slug(&self.engine, &args.run_slug).await {
            return Json(MutationResult::err(e));
        }

        let (page_type, edge_type) = match args.kind.as_str() {
            "finding" => ("finding", "produces"),
            "limitation" => ("limitation", "produces"),
            "analysis_plan" => ("analysis_plan", "produces"),
            other => {
                return Json(MutationResult::err(format!(
                    "kind must be finding|limitation|analysis_plan, got {other}"
                )));
            }
        };
        let status = args.status.unwrap_or_else(|| "draft".to_string());
        if args.kind == "finding" && !matches!(status.as_str(), "draft" | "claim" | "validated") {
            return Json(MutationResult::err(format!(
                "finding status must be draft|claim|validated, got {status}"
            )));
        }
        let mut fm = serde_json::Map::new();
        fm.insert("type".into(), serde_json::json!(page_type));
        fm.insert("title".into(), serde_json::json!(args.title));
        fm.insert("research_run".into(), serde_json::json!(args.run_slug));
        if args.kind == "finding" {
            fm.insert("status".into(), serde_json::json!(status));
        }
        let body = format!("# {}\n\n{}\n", args.title, args.content);
        let page = page_with_frontmatter(args.slug.clone(), page_type, &args.title, fm, body);
        if let Err(e) = self.engine.put_page(page).await {
            return Json(MutationResult::err(format!("put_page failed: {e}")));
        }
        if let Err(e) = self
            .engine
            .add_link(&args.run_slug, &args.slug, edge_type, None, None)
            .await
        {
            return Json(MutationResult::err(format!("add_link failed: {e}")));
        }
        Json(MutationResult::ok(format!(
            "{} recorded and linked via {edge_type}",
            args.kind
        )))
    }

    #[tool(
        name = "brain_validate_research_run",
        description = "Run validators against a research run; the set depends on task_type. \
            data_analysis: dataset_registered, analysis_plan_exists, artifact_hash_present, \
            finding_has_dataset_lineage, finding_has_supporting_artifact. \
            mixed_methods | theory_building: analysis_plan_exists, artifact_hash_present, \
            finding_has_supporting_artifact. \
            literature_review: validators land in M3 — overall returns 'warn' until then. \
            Returns structured results with controlled suggested_actions enum and the derived \
            protocol state so ZeroClaw knows the next step."
    )]
    async fn validate_research_run(
        &self,
        Parameters(args): Parameters<ValidateRunArgs>,
    ) -> Json<ValidateRunResult> {
        let store = ResearchRunStore::new(self.engine.get_db());
        let run = match store.get(&args.run_id).await {
            Ok(r) => r,
            Err(_e) => {
                return Json(ValidateRunResult {
                    ok: false,
                    run_id: args.run_id,
                    overall: "error".into(),
                    validators: vec![],
                    protocol: None,
                });
            }
        };
        let pool = self.engine.get_db();
        let vs = run_research_validators(pool, &run).await;

        // Empty validator list is **not** "pass" — it means no validators are
        // wired yet for this task_type (currently: literature_review, M3 work).
        // Returning `warn` prevents ZeroClaw from falsely concluding the run
        // has been validated.
        let overall = if vs.is_empty() {
            "warn"
        } else if vs
            .iter()
            .any(|v| v.status == rbrain_engine::evidence::ValidatorStatus::Fail)
        {
            "fail"
        } else if vs
            .iter()
            .any(|v| v.status == rbrain_engine::evidence::ValidatorStatus::Warn)
        {
            "warn"
        } else {
            "pass"
        }
        .to_string();

        // Persist summary for protocol derivation across sessions.
        let mut latest_run = run.clone();
        if let Ok(json) = serde_json::to_string(&vs) {
            let fingerprint = {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut h = DefaultHasher::new();
                json.hash(&mut h);
                format!("{:016x}", h.finish())
            };
            if let Ok(updated) = store
                .record_validation(&run.id, &fingerprint, &json)
                .await
            {
                latest_run = updated;
            }
            // Advance status: planned -> running on first validation; -> validating on all-pass.
            let next = match (latest_run.status, overall.as_str()) {
                (RunStatus::Planned, _) => Some(RunStatus::Running),
                (RunStatus::Running, "pass") => Some(RunStatus::Validating),
                _ => None,
            };
            if let Some(n) = next {
                if let Ok(updated) = store.set_status(&latest_run.id, n).await {
                    latest_run = updated;
                }
            }
        }

        let protocol = protocol_to_json(&derive_state(&latest_run, &vs));

        Json(ValidateRunResult {
            ok: true,
            run_id: latest_run.id,
            overall,
            validators: vs.iter().map(validator_to_json).collect(),
            protocol: Some(protocol),
        })
    }

    #[tool(
        name = "brain_citation_check",
        description = "Verify all wiki citations in a page resolve to source pages, not derived ones (draft/synthesis/wiki). \
            Wraps the existing `rbrain audit` logic into the ValidatorResult vocabulary with suggested_actions."
    )]
    async fn citation_check(
        &self,
        Parameters(args): Parameters<CitationCheckArgs>,
    ) -> Json<CitationCheckResultJson> {
        match run_citation_check(&self.engine, &args.slug).await {
            Ok(report) => Json(CitationCheckResultJson {
                ok: true,
                slug: report.slug,
                validator: validator_to_json(&report.validator),
            }),
            Err(e) => Json(CitationCheckResultJson {
                ok: false,
                slug: args.slug.clone(),
                validator: ValidatorJson {
                    validator: "brain_citation_check".into(),
                    status: "fail".into(),
                    message: format!("audit failed: {e}"),
                    affected_slugs: vec![args.slug],
                    suggested_actions: vec![],
                },
            }),
        }
    }

    #[tool(
        name = "brain_evidence_check",
        description = "Walk the provenance graph from a finding and report whether it terminates in \
            evidence the validators recognise. Supports both shapes: data_analysis \
            (supports → artifact → derived_from → dataset) and literature_review \
            (cites/supports → note|raw). Returns the resolved chain (direct_support, \
            datasets, scripts, literature_sources) plus a validator verdict."
    )]
    async fn evidence_check(
        &self,
        Parameters(args): Parameters<EvidenceCheckArgs>,
    ) -> Json<EvidenceCheckResultJson> {
        let pool = self.engine.get_db();
        match rbrain_engine::evidence::run_evidence_check(pool, &args.finding_slug).await {
            Ok(report) => {
                let chain_json =
                    serde_json::to_value(&report.chain).unwrap_or(serde_json::Value::Null);
                Json(EvidenceCheckResultJson {
                    ok: true,
                    finding_slug: args.finding_slug,
                    validator: validator_to_json(&report.validator),
                    chain: chain_json,
                })
            }
            Err(e) => Json(EvidenceCheckResultJson {
                ok: false,
                finding_slug: args.finding_slug.clone(),
                validator: ValidatorJson {
                    validator: "brain_evidence_check".into(),
                    status: "fail".into(),
                    message: format!("traversal failed: {e}"),
                    affected_slugs: vec![args.finding_slug],
                    suggested_actions: vec![],
                },
                chain: serde_json::Value::Null,
            }),
        }
    }

    #[tool(
        name = "brain_provenance_of",
        description = "Enumerate one-hop incoming and outgoing research-context edges for a slug. \
            This is an adjacency report, not a recursive chain traversal. For a finding's resolved \
            evidence chain, call brain_evidence_check. Returned edges are limited to: derived_from, \
            computed_by, uses_dataset, uses_method, uses_variable, supports, contradicts, produces, \
            cites, tests_hypothesis, validates, limits."
    )]
    async fn provenance_of(
        &self,
        Parameters(args): Parameters<ProvenanceArgs>,
    ) -> Json<ProvenanceResultJson> {
        let pool = self.engine.get_db();
        match rbrain_engine::evidence::provenance_of(pool, &args.slug).await {
            Ok(report) => {
                let edges_json =
                    serde_json::to_value(&report.edges).unwrap_or(serde_json::Value::Array(vec![]));
                Json(ProvenanceResultJson {
                    ok: true,
                    slug: report.slug,
                    page_type: report.page_type,
                    edges: edges_json,
                    message: String::new(),
                })
            }
            Err(e) => Json(ProvenanceResultJson {
                ok: false,
                slug: args.slug,
                page_type: String::new(),
                edges: serde_json::Value::Array(vec![]),
                message: format!("provenance_of failed: {e}"),
            }),
        }
    }
}

pub async fn run_stdio_server(engine: Engine) -> anyhow::Result<()> {
    let server = RBrainMcpServer::new(engine);
    let service = rmcp::serve_server(server, (tokio::io::stdin(), tokio::io::stdout())).await?;
    service.waiting().await?;
    Ok(())
}

pub mod http;
pub use http::run_http_server;
