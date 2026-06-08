use async_trait::async_trait;
use rbrain_core::error::Result;
use rbrain_core::page::Page;
use rbrain_core::prompt_loader::PromptLoader;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Input specification ───────────────────────────────────────────────────────

/// How to gather input pages for a pipeline step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum InputSpec {
    /// Use each page's `compiled_truth` directly (EXTRACT, ANNOTATE, EVALUATE, TRANSFORM).
    SelfContent {
        page_type: String,
        #[serde(default)]
        tag: Option<String>,
        #[serde(default)]
        language: Option<String>,
        /// If set, only process these specific slugs.
        #[serde(default)]
        slugs: Option<Vec<String>>,
    },

    /// Anchor-based: collect linked source pages for each anchor (AGGREGATE).
    LinkedSources {
        anchor_page_type: String,
        source_page_type: String,
        #[serde(default = "default_min_sources")]
        min_sources: usize,
        /// true = pull DB chunks for high-fidelity context; false = compiled_truth snippet
        #[serde(default = "default_true")]
        use_chunks: bool,
        /// Jaccard similarity threshold for source-set deduplication.
        /// When two anchors share ≥ this fraction of source articles, the one with fewer
        /// sources is skipped. 0.0 = disabled (default). Typical useful value: 0.8.
        #[serde(default)]
        dedup_sources_threshold: f32,
        /// M3 Slice 5: cap total prompt context at this many estimated tokens
        /// across all source blocks for one anchor. None = no token cap
        /// (legacy char-only behavior).
        #[serde(default)]
        token_budget: Option<usize>,
    },

    /// Aggregate all pages of a given type into a single LLM call → one output page (COMPOSE).
    /// Unlike SelfContent (one call per page), this concatenates all pages as one context block.
    AggregateContent {
        page_type: String,
        #[serde(default)]
        tag: Option<String>,
        /// Maximum number of pages to include (most recently updated first).
        /// Prevents context window overflow on large corpora.
        #[serde(default = "default_aggregate_max")]
        max_pages: usize,
        /// Max chars of compiled_truth to include per page (truncates long pages).
        #[serde(default = "default_chars_per_page")]
        chars_per_page: usize,
        /// M3 Slice 5: cap total prompt context at this many estimated tokens.
        /// Pages are packed greedily until the budget is reached. None = no
        /// token cap (legacy chars_per_page × max_pages behavior).
        #[serde(default)]
        token_budget: Option<usize>,
    },
}

fn default_min_sources() -> usize { 3 }
fn default_true() -> bool { true }
fn default_aggregate_max() -> usize { 30 }
fn default_chars_per_page() -> usize { 2000 }

// ─── Prompt specification ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PromptSpec {
    /// Load from `$prompts_dir/{name}.md`, falling back to built-in.
    File(String),
    /// Literal prompt text supplied by the caller.
    Inline(String),
}

// ─── Response format ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseFormat {
    /// LLM returns JSON; validate against output_schema and retry on failure.
    Json,
    /// LLM returns Markdown; pass through normalize_llm_output.
    Markdown,
}

// ─── Output mode ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum OutputMode {
    /// Return results to the caller — do not write to the knowledge base.
    Return,

    /// Save each result as a new page (one-to-one).
    SaveAs {
        page_type: String,
        slug_prefix: String,
        #[serde(default)]
        embed: bool,
    },

    /// Save multiple page types from a single LLM response (one-to-many).
    /// The key is the JSON array field name in the LLM response.
    SaveMulti {
        type_map: HashMap<String, SaveTypeConfig>,
    },

    /// Merge result fields back into the input page's frontmatter.
    UpdateFrontmatter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveTypeConfig {
    pub page_type: String,
    pub slug_prefix: String,
    #[serde(default)]
    pub embed: bool,
    /// When true, append new description to existing page instead of overwriting.
    /// Useful for concept enrichment across multiple source articles.
    #[serde(default)]
    pub enrich_existing: bool,
}

// ─── PipelineStep ──────────────────────────────────────────────────────────────

/// Unified parameter block for all LLM-driven pipeline stages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStep {
    pub id: String,

    // Input
    pub input: InputSpec,

    // LLM processing
    pub prompt: PromptSpec,
    /// Optional JSON Schema hint sent to the LLM; also used by RetryParser.
    #[serde(default)]
    pub output_schema: Option<String>,
    pub response_format: ResponseFormat,

    // Output
    pub output_mode: OutputMode,

    // Execution control
    #[serde(default = "default_true")]
    pub incremental: bool,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default)]
    pub max_inputs: Option<usize>,

    // Context injection
    #[serde(default)]
    pub inject_existing_titles: Option<String>,

    // Model selection — "none" bypasses LLM (e.g. CnkiRefParser), "flash" or "pro" overrides default routing.
    #[serde(default)]
    pub model_tier: Option<String>,

    // When true and output_mode = SaveMulti: skip a source page if its expected target slug already
    // exists in the DB. Used by extract_pub_metadata_auto so CNKI-derived pages take priority.
    #[serde(default)]
    pub skip_if_target_exists: bool,

    // When true: query all pub_metadata pages and prepend a citation table to the system prompt
    // before the LLM call. Used by the compose stage to inject correct author/year/journal data.
    #[serde(default)]
    pub inject_pub_metadata: bool,
}

fn default_batch_size() -> usize { 1 }

// ─── Runnable trait ────────────────────────────────────────────────────────────

/// Unified interface for all pipeline components (borrowing from LangChain LCEL).
#[async_trait]
pub trait Runnable: Send + Sync {
    type Input: Send;
    type Output: Send;

    async fn invoke(&self, input: Self::Input) -> Result<Self::Output>;

    async fn batch(&self, inputs: Vec<Self::Input>) -> Result<Vec<Self::Output>> {
        let mut out = Vec::with_capacity(inputs.len());
        for i in inputs {
            out.push(self.invoke(i).await?);
        }
        Ok(out)
    }
}

/// Sequential composition: runs A then feeds its output to B.
#[derive(Debug)]
pub struct Sequential<A, B> {
    pub a: A,
    pub b: B,
}

#[async_trait]
impl<A, B> Runnable for Sequential<A, B>
where
    A: Runnable,
    B: Runnable<Input = A::Output>,
{
    type Input = A::Input;
    type Output = B::Output;

    async fn invoke(&self, input: A::Input) -> Result<B::Output> {
        let mid = self.a.invoke(input).await?;
        self.b.invoke(mid).await
    }
}

// ─── PromptTemplate ────────────────────────────────────────────────────────────

/// A prompt string that supports `{key}` variable substitution.
#[derive(Debug, Clone)]
pub struct PromptTemplate(pub String);

impl PromptTemplate {
    pub fn from_loader(loader: &PromptLoader, name: &str) -> Self {
        Self(loader.load(name))
    }

    pub fn from_str(s: &str) -> Self {
        Self(s.to_string())
    }

    pub fn render(&self, vars: &HashMap<&str, &str>) -> String {
        PromptLoader::render(&self.0, vars)
    }
}

// ─── RetryParser ───────────────────────────────────────────────────────────────

/// Parse JSON from an LLM response, retrying with error feedback on failure.
#[derive(Debug, Clone)]
pub struct RetryParser {
    pub max_retries: usize,
}

impl Default for RetryParser {
    fn default() -> Self { Self { max_retries: 2 } }
}

impl RetryParser {
    /// Try to parse `response` as `T`. On failure, ask the LLM to fix it.
    pub async fn parse_with_retry<T, F, Fut>(
        &self,
        response: &str,
        schema_hint: Option<&str>,
        retry_fn: F,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = Result<String>>,
    {
        let cleaned = clean_json(response);
        if let Ok(v) = serde_json::from_str::<T>(cleaned) {
            return Ok(v);
        }

        let mut last_resp = response.to_string();
        for attempt in 1..=self.max_retries {
            let hint = schema_hint.unwrap_or("valid JSON");
            let fix_prompt = format!(
                "Your previous JSON response failed to parse. Required schema: {hint}\n\
                 Your response was:\n{last_resp}\n\
                 Please return only valid JSON matching the schema."
            );
            let fixed = retry_fn(fix_prompt).await?;
            let cleaned_fixed = clean_json(&fixed);
            if let Ok(v) = serde_json::from_str::<T>(cleaned_fixed) {
                return Ok(v);
            }
            eprintln!("  [RetryParser] attempt {}/{} still invalid JSON", attempt, self.max_retries);
            last_resp = fixed;
        }

        Err(rbrain_core::error::BrainError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("failed to parse LLM output after {} retries", self.max_retries),
        )))
    }
}

// ─── PageBatch ─────────────────────────────────────────────────────────────────

/// One unit of work fed into a PipelineStep — either a single page or
/// an anchor page plus its linked source pages.
#[derive(Debug, Clone)]
pub enum PageBatch {
    /// SelfContent mode: one (or more, if batch_size > 1) pages processed together.
    Single(Vec<Page>),
    /// LinkedSources mode: anchor page + its linked source pages.
    Anchored {
        anchor: Page,
        sources: Vec<Page>,
    },
}

impl PageBatch {
    pub fn pages(&self) -> Vec<&Page> {
        match self {
            PageBatch::Single(pages) => pages.iter().collect(),
            PageBatch::Anchored { anchor, sources } => {
                let mut v = vec![anchor];
                v.extend(sources.iter());
                v
            }
        }
    }
}

// ─── Helpers (reused from engine.rs) ──────────────────────────────────────────

pub fn clean_json(s: &str) -> &str {
    let mut s = s.trim();
    if s.starts_with("```") {
        if let Some(end) = s.rfind("```").filter(|&e| e > 0) {
            s = &s[3..end];
            if s.starts_with("json") {
                s = &s[4..];
            }
        }
    }
    s.trim()
}

// ─── Profile / Workflow TOML structures (Phase 4) ─────────────────────────────

/// A named pipeline profile loaded from `$profiles_dir/{name}.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineProfile {
    pub profile: ProfileMeta,
    #[serde(default)]
    pub stages: Vec<StageConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileMeta {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// TOML representation of a single stage (flat for human-friendliness).
/// Deserialized and converted to `PipelineStep` by `StageConfig::into_step()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageConfig {
    pub id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,

    // ── Input ──
    #[serde(default = "default_input_mode")]
    pub input_mode: String,   // "self" | "linked_sources"
    #[serde(default)]
    pub page_type: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub slugs: Option<Vec<String>>,
    #[serde(default)]
    pub anchor_type: Option<String>,
    #[serde(default)]
    pub source_type: Option<String>,
    #[serde(default = "default_min_sources")]
    pub min_sources: usize,
    #[serde(default = "default_true")]
    pub use_chunks: bool,
    #[serde(default)]
    pub dedup_sources_threshold: f32,
    // aggregate mode
    #[serde(default = "default_aggregate_max")]
    pub max_pages: usize,
    #[serde(default = "default_chars_per_page")]
    pub chars_per_page: usize,
    /// M3 Slice 5: optional token-budget for AggregateContent / LinkedSources.
    /// When set, prefer this over chars × max_pages as the prompt size cap.
    #[serde(default)]
    pub token_budget: Option<usize>,

    // ── Processing ──
    pub prompt: String,
    #[serde(default = "default_response_format")]
    pub response_format: String,  // "json" | "markdown"
    #[serde(default)]
    pub output_schema: Option<String>,

    // ── Output ──
    #[serde(default = "default_output_mode")]
    pub output_mode: String,  // "return" | "save" | "save_multi" | "update_frontmatter"
    #[serde(default)]
    pub output_page_type: Option<String>,
    #[serde(default)]
    pub output_slug_prefix: Option<String>,
    #[serde(default)]
    pub embed_output: bool,
    #[serde(default)]
    pub type_map: HashMap<String, SaveTypeConfig>,

    // ── Execution ──
    #[serde(default = "default_true")]
    pub incremental: bool,
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    #[serde(default)]
    pub max_inputs: Option<usize>,

    // ── Context injection ──
    /// If set, fetch all existing page titles of this page_type and inject as
    /// "already-known" context so the LLM normalises to existing names.
    /// Example: "concept" — prevents synonym proliferation in extract stages.
    #[serde(default)]
    pub inject_existing_titles: Option<String>,

    // ── Model / bypass ──
    /// "none" = bypass LLM entirely (use CnkiRefParser for ref_entry pages).
    /// "flash" | "pro" = explicit model tier override.
    /// Absent = default routing heuristic (pro for synthesis, flash otherwise).
    #[serde(default)]
    pub model_tier: Option<String>,

    /// When true and output_mode = save_multi: skip source pages whose expected
    /// target slug already exists in DB (slug_prefix + slugify(source_basename)).
    #[serde(default)]
    pub skip_if_target_exists: bool,

    /// When true: prepend a pub_metadata citation table to the compose system prompt.
    #[serde(default)]
    pub inject_pub_metadata: bool,
}

fn default_input_mode() -> String { "self".to_string() }
fn default_response_format() -> String { "json".to_string() }
fn default_output_mode() -> String { "return".to_string() }

impl StageConfig {
    pub fn into_step(self) -> Result<PipelineStep> {
        let input = match self.input_mode.as_str() {
            "self" => InputSpec::SelfContent {
                page_type: self.page_type.unwrap_or_default(),
                tag: self.tag,
                language: self.language,
                slugs: self.slugs,
            },
            "linked_sources" => InputSpec::LinkedSources {
                anchor_page_type: self.anchor_type.unwrap_or_default(),
                source_page_type: self.source_type.unwrap_or_default(),
                min_sources: self.min_sources,
                use_chunks: self.use_chunks,
                dedup_sources_threshold: self.dedup_sources_threshold,
                token_budget: self.token_budget,
            },
            "aggregate" => InputSpec::AggregateContent {
                page_type: self.page_type.unwrap_or_default(),
                tag: self.tag,
                max_pages: self.max_pages,
                chars_per_page: self.chars_per_page,
                token_budget: self.token_budget,
            },
            other => return Err(rbrain_core::error::BrainError::Conflict(
                format!("unknown input_mode '{other}'")
            )),
        };

        let response_format = match self.response_format.as_str() {
            "json" => ResponseFormat::Json,
            "markdown" => ResponseFormat::Markdown,
            other => return Err(rbrain_core::error::BrainError::Conflict(
                format!("unknown response_format '{other}'")
            )),
        };

        let output_mode = match self.output_mode.as_str() {
            "return" => OutputMode::Return,
            "save" => OutputMode::SaveAs {
                page_type: self.output_page_type.unwrap_or_default(),
                slug_prefix: self.output_slug_prefix.unwrap_or_default(),
                embed: self.embed_output,
            },
            "save_multi" => OutputMode::SaveMulti { type_map: self.type_map },
            "update_frontmatter" => OutputMode::UpdateFrontmatter,
            other => return Err(rbrain_core::error::BrainError::Conflict(
                format!("unknown output_mode '{other}'")
            )),
        };

        Ok(PipelineStep {
            id: self.id,
            input,
            prompt: PromptSpec::File(self.prompt),
            output_schema: self.output_schema,
            response_format,
            output_mode,
            incremental: self.incremental,
            batch_size: self.batch_size,
            max_inputs: self.max_inputs,
            inject_existing_titles: self.inject_existing_titles,
            model_tier: self.model_tier,
            skip_if_target_exists: self.skip_if_target_exists,
            inject_pub_metadata: self.inject_pub_metadata,
        })
    }
}

#[cfg(test)]
mod token_budget_wiring_tests {
    //! Verify that TOML `token_budget = N` reaches InputSpec correctly
    //! for both AggregateContent and LinkedSources stages.

    use super::*;

    #[test]
    fn aggregate_token_budget_threads_through_from_toml() {
        let toml = r#"
            id = "compose"
            input_mode = "aggregate"
            page_type = "synthesis"
            max_pages = 25
            chars_per_page = 2500
            token_budget = 50000
            prompt = "compose_lit"
            response_format = "markdown"
            output_mode = "save"
            output_page_type = "wiki"
            output_slug_prefix = "research/wiki/"
        "#;
        let cfg: StageConfig = toml::from_str(toml).expect("parse");
        assert_eq!(cfg.token_budget, Some(50000));
        let step = cfg.into_step().expect("to step");
        match step.input {
            InputSpec::AggregateContent { token_budget, .. } => {
                assert_eq!(token_budget, Some(50000));
            }
            other => panic!("expected AggregateContent, got {other:?}"),
        }
    }

    #[test]
    fn linked_sources_token_budget_threads_through_from_toml() {
        let toml = r#"
            id = "synthesize"
            input_mode = "linked_sources"
            anchor_type = "concept"
            source_type = "note"
            min_sources = 2
            token_budget = 30000
            prompt = "synth"
            response_format = "markdown"
            output_mode = "save"
            output_page_type = "synthesis"
            output_slug_prefix = "research/synthesis/"
        "#;
        let cfg: StageConfig = toml::from_str(toml).expect("parse");
        assert_eq!(cfg.token_budget, Some(30000));
        let step = cfg.into_step().expect("to step");
        match step.input {
            InputSpec::LinkedSources { token_budget, .. } => {
                assert_eq!(token_budget, Some(30000));
            }
            other => panic!("expected LinkedSources, got {other:?}"),
        }
    }

    #[test]
    fn missing_token_budget_is_none_for_backward_compat() {
        let toml = r#"
            id = "compose"
            input_mode = "aggregate"
            page_type = "synthesis"
            max_pages = 10
            chars_per_page = 1000
            prompt = "compose"
            response_format = "markdown"
            output_mode = "save"
            output_page_type = "wiki"
            output_slug_prefix = "research/wiki/"
        "#;
        let cfg: StageConfig = toml::from_str(toml).expect("parse");
        assert_eq!(cfg.token_budget, None);
        let step = cfg.into_step().expect("to step");
        match step.input {
            InputSpec::AggregateContent { token_budget, .. } => {
                assert_eq!(token_budget, None);
            }
            other => panic!("expected AggregateContent, got {other:?}"),
        }
    }
}
