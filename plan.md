# rbrain-hub improvement plan

This note records the code-review findings and roadmap for evolving rbrain-hub into the research memory and quality layer for a lightweight academic research agent stack.

> **Status (2026-06-08)**: The top-level execution plan has been revised to v2; see `../rbrain-hub-execution-plan.md`. The gap analysis below remains authoritative as the *inventory* of rbrain-hub-side debt, but milestone sequencing now follows v2. The "Gap → Milestone mapping" and "Locked decisions" sections at the bottom are the source of truth for which work belongs in which milestone.
>
> **M0 + M1 shipped** in commit `0819ae6`: research_runs migration, evidence/research engine modules, 7 consolidated MCP tools, doctor sparse warn, `query --explain` v1, 9-test data_analysis fixture.
>
> **M2 shipped** in the follow-up commit: `ResearchEdge` vocabulary (12 edges), real graph-traversal `brain_evidence_check` returning `EvidenceChain`, new `brain_provenance_of(slug)` tool that enumerates one-hop research-edge adjacency only (filters out `references`/`mentions`/etc.), `SuggestedAction::RecordAnalysisPlan`, 8-test m2_provenance fixture.
>
> **M3 in progress (2026-06-08)**: Literature review quality upgrade. Shipped so far: synthesis quality gates (citation coverage, section limit, thin-section rejection), synthesis retry with feedback injection, dual-model routing (flash for extract/simple, pro for synthesis/compose), compose stage timeout fix (600 s), min_sources pre-filter for LinkedSources anchors. Designed (not yet coded): `pub_metadata` extraction pipeline (`CnkiRefParser` + LLM auto-extract), compose-time pub_metadata injection (`inject_pub_metadata`), and post-hoc citation audit stage (`verify_citations`). See Tasks #32–#41.

## Current assessment

rbrain-hub already has the right core shape:

- Rust single-binary architecture with CLI, MCP server, and background worker surfaces.
- SQLite as system of record, Tantivy for BM25, LanceDB for vectors.
- Academic page model with Markdown repository sync.
- MCP tools for search, read/write, graph traversal, thinking, timeline, and process automation.
- Configurable LLM pipeline profiles for extraction, synthesis, and literature-review composition.
- Academic prompts that require source citations and distinguish original sources from secondary synthesis pages.

The main gap is not the absence of features. The main gap is that retrieval quality, citation quality, pipeline execution, and academic metadata are not yet systematic enough for high-quality research workflows.

## Product positioning with ZeroClaw

rbrain-hub should not become another general agent runtime. In the rbrain + ZeroClaw stack, the clean division is:

- ZeroClaw is the executor and orchestrator: it runs tools, shell commands, Python/R/SQL, browser work, file operations, report export, deployment tasks, and multi-step user-facing workflows under its security, approval, sandbox, and tool-receipt model.
- rbrain-hub is the research assistant's brain, lab notebook, literature base, and method reviewer: it stores research state, retrieves evidence, links claims to sources, records analysis artifacts, preserves provenance, runs citation/method checks, and produces evidence-grounded synthesis.

This means complex execution-heavy work, especially data analysis, should remain with ZeroClaw. rbrain should assist by making the work traceable, reviewable, citable, and reusable.

### rbrain responsibilities

- Maintain the literature corpus and research knowledge graph.
- Provide hybrid retrieval and graph traversal over primary sources, concepts, figures, methods, datasets, findings, and research memos.
- Store research runs as structured records: research question, hypothesis, dataset, codebook, analysis plan, generated artifacts, findings, limitations, and final memo.
- Track provenance from source pages and datasets to scripts, results, findings, and reports.
- Run method and evidence quality checks.
- Generate literature-review synthesis and other evidence-grounded intermediate research products.
- Expose all of the above through MCP so ZeroClaw can call it during research workflows.

### ZeroClaw responsibilities

- Execute data analysis and other complex tasks using shell, Python/R/SQL, files, browser, HTTP, and external APIs.
- Coordinate multi-turn user interaction and task planning.
- Decide when to call rbrain retrieval, rbrain synthesis, or rbrain validators.
- Register datasets, scripts, outputs, and findings back into rbrain.
- Produce final deliverables such as docx/pdf/slides/web reports when needed.

### Literature review division

Literature review should not be removed from rbrain. It is one of rbrain's core strengths because the input is the literature corpus, citation chunks, concept pages, and knowledge graph.

Recommended split:

- rbrain produces evidence-grounded synthesis: extracted concepts, per-concept synthesis pages, literature-review drafts, gap analyses, and citation-aware reasoning artifacts.
- ZeroClaw orchestrates the review process: chooses when to run the rbrain profile, asks follow-up questions, rewrites structure, requests missing evidence, merges with data-analysis results, and exports final deliverables.

Two modes should coexist:

- rbrain-led review: ZeroClaw triggers `rbrain dream --profile literature_review`; rbrain generates concepts, synthesis, and review draft; ZeroClaw reviews, edits, and exports.
- ZeroClaw-led review: ZeroClaw repeatedly calls `brain_query`, `brain_get`, `brain_graph`, and `brain_think`; ZeroClaw writes the final review; rbrain validates citations and stores the final memo.

rbrain can generate academic synthesis, but those generated outputs should be treated as traceable research materials, not as the only possible final authoring surface.

## High-priority gaps

### 1. Make sparse retrieval real or report degradation

The README describes three-way hybrid search: dense vector, sparse vector, and BM25. The engine calls all three branches, but the current LanceDB Rust backend returns an empty result for sparse search because sparse ANN is not exposed by the SDK.

Impact:

- The system currently behaves like dense + BM25, not dense + sparse + BM25.
- Chinese academic terms, proper nouns, and exact phrases lose an important retrieval signal.

Plan:

- Add a health/doctor warning when sparse search is disabled.
- Update docs to distinguish planned vs active retrieval paths.
- Implement a fallback sparse scoring path if LanceDB cannot do it yet:
  - store sparse vectors in LanceDB or SQLite,
  - build a sparse inverted index,
  - score query sparse vector against candidate chunks,
  - fuse with dense and BM25 via RRF.

### 2. Add explainable retrieval and diagnostics

Current search returns ranked chunks, but not why they matched. gbrain treats retrieval as an explainable and testable subsystem with search explain, search diagnose, named-entity benchmarks, and replay evaluation.

Plan:

- Add `rbrain query --explain`.
- Add `rbrain search diagnose "<query>" --target <slug>`.
- Return per-result attribution:
  - dense rank and score,
  - sparse rank and score,
  - BM25 rank and score,
  - RRF contribution,
  - backlink/graph boost,
  - title or alias hit,
  - source-tier boost,
  - final score,
  - evidence type.
- Add structured MCP output for these explanations so ZeroClaw can reason over retrieval failures.

### 3. Add page-level max-pooling before final ranking

Current search is chunk-grain. A long article can occupy several top slots before context assembly deduplicates it. Academic research usually needs source diversity.

Plan:

- After initial candidate retrieval, group candidates by `page_slug`.
- Keep the best candidate chunk per page for page ranking.
- After selecting top pages, expand to 1-3 evidence chunks per page.
- Make per-page chunk cap configurable by profile or search mode.

### 4. Use query cache

There is a `query_cache` migration, but the expanded-search path still calls intent classification and query expansion each time.

Plan:

- Cache normalized query + language + expansion profile.
- Store intent, expansions, created_at, and provider/model version.
- Add TTL and manual invalidation.
- Report cache hit/miss in `--explain`.

### 5. Harden embedding throughput and reliability

The Qwen embedder currently uses a fixed batch size and serial API calls, with no retry/backoff/rate lease.

Plan:

- Make batch size configurable.
- Add bounded concurrency.
- Add retry with jitter and `Retry-After` support.
- Track partial failures at chunk/page level.
- Submit failed embedding work back to the job queue.
- Record provider/model/dimension metadata for migration safety.

## Medium-priority gaps

### 6. Upgrade the job system into the default execution substrate

The job queue exists, but long research tasks still run mostly as foreground CLI/pipeline work. Some handlers are also incomplete; for example, extract-links currently returns extracted links but does not persist them.

Plan:

- Run dream stages and pipeline stages as durable jobs.
- Add stage-level progress events.
- Add cancellation, resume, dead-letter queues, and retry policies.
- Add parent/child jobs for long literature-review and data-analysis workflows.
- Persist pipeline run state and artifacts.

### 7. Add academic metadata as structured fields

The current page model relies heavily on frontmatter plus full text. High-quality academic research needs structured metadata.

Plan:

- Add or standardize fields:
  - authors,
  - year,
  - journal,
  - DOI,
  - citation key,
  - publication type,
  - abstract,
  - keywords,
  - institution,
  - methodology,
  - theory,
  - research question,
  - dataset,
  - region,
  - period.
- Build filters and facets over these fields.
- Add importers that preserve source metadata.

### 8. Extend graph semantics for academic work

Current graph edges are useful but too generic for research analysis.

Plan:

- Add academic edge types:
  - cites,
  - critiques,
  - extends,
  - uses_theory,
  - uses_method,
  - studies_case,
  - defines_concept,
  - operationalizes,
  - finds,
  - contradicts,
  - replicates.
- Distinguish literature citation graphs, concept lineage graphs, evidence graphs, method graphs, and scholar/institution graphs.
- Add graph-aware retrieval boosts per query intent.

### 9. Add citation verification

Prompts ask the LLM to cite `[[slug | chunk:N]]`, but format compliance is not enough.

Plan:

- Verify that cited slugs exist.
- Verify that chunk IDs belong to the cited slug.
- Verify that each citation is attached to a source chunk, not only a synthesis page.
- Check that substantive claims have at least one citation.
- Add optional claim-support verification using retrieved source chunks.
- Reject or repair low-quality synthesis outputs before saving.

### 10. Improve remote MCP security

HTTP MCP is currently forced to loopback because it exposes mutation tools without auth. That is correct for local ZeroClaw integration but insufficient for remote clients.

Plan:

- Keep loopback as the default.
- Add bearer-token or OAuth support for remote MCP.
- Add scopes:
  - read,
  - write,
  - admin.
- Add rate limits and audit logs per token/client.

## Pipeline-specific review

The pipeline abstraction is promising but still oriented around one workflow: literature review. It currently supports:

- `SelfContent`: each page is processed independently.
- `LinkedSources`: each anchor page gathers linked source pages.
- `AggregateContent`: many pages are concatenated into one LLM call.
- `Return`, `SaveAs`, `SaveMulti`, and `UpdateFrontmatter` output modes.
- TOML profiles with custom stages and prompts.

This is enough for extraction -> synthesis -> composition. It is not yet general enough for data analysis, mixed-method research, or reproducible empirical workflows.

### Pipeline gap 1: stages are linear, not a DAG

Profiles are ordered lists of stages. A stage cannot declare named dependencies, fan-out/fan-in rules, conditional branches, or reusable intermediate artifacts.

Plan:

- Add optional `depends_on = [...]`.
- Add `when` conditions based on page type, artifact existence, metadata, or prior stage output.
- Add fan-out/fan-in primitives:
  - per-document extraction,
  - per-variable profiling,
  - per-cluster synthesis,
  - final report composition.
- Persist each stage run with input hash, output hash, model, prompt version, and status.

### Pipeline gap 2: processing is hard-wired to LLM calls

Every pipeline step currently assumes a prompt and a DeepSeek chat call. For data analysis workflows, rbrain should not become the place where arbitrary generated code is executed. Instead, rbrain should support protocol, provenance, validation, and synthesis steps around analysis that ZeroClaw executes.

Plan:

- Add processor types:
  - `llm`,
  - `search`,
  - `graph_query`,
  - `protocol`,
  - `artifact_register`,
  - `validation`,
  - `compose`.
- Keep LLM steps for interpretation, literature synthesis, method critique, and narrative synthesis.
- Let ZeroClaw handle execution processors such as script, SQL, statistics, and chart generation.
- Let rbrain validate and store the resulting artifacts, metrics, and claims.

Example future TOML shape:

```toml
[[stages]]
id = "register_analysis_result"
processor = "artifact_register"
input_mode = "external_artifact"
artifact = "outputs/descriptive_stats.json"
output_mode = "save_artifact"

[[stages]]
id = "interpret_results"
processor = "llm"
depends_on = ["register_analysis_result"]
prompt = "interpret_statistical_profile"
output_mode = "save"
```

### Pipeline gap 3: inputs are page-centric

The current input modes assume Markdown pages. Data analysis needs CSV, TSV, XLSX, JSON, SQL extracts, codebooks, and possibly images/figures.

Plan:

- Introduce an `Artifact` model:
  - path,
  - type,
  - media type,
  - schema,
  - hash,
  - provenance,
  - created_by_stage,
  - related_page_slug.
- Add input modes:
  - `artifact`,
  - `dataset`,
  - `query_results`,
  - `previous_stage`,
  - `external_artifact`.
- Keep pages as narrative and knowledge outputs, not the only data carrier.
- Treat executable data processing outputs from ZeroClaw as external artifacts that rbrain registers, hashes, links, and validates.

### Pipeline gap 4: outputs are mostly pages

Data analysis needs tables, charts, logs, model outputs, cleaned datasets, and reproducible scripts.

Plan:

- Add output modes:
  - `save_artifact`,
  - `save_table`,
  - `save_dataset`,
  - `save_chart_spec`,
  - `append_evidence`,
  - `emit_metrics`,
  - `fail_if`.
- Store artifacts under `.rbrain/artifacts/` with content hashes.
- Link artifacts back to pages with graph edges such as `derived_from`, `analyzes`, and `supports`.

### Pipeline gap 5: no explicit data contracts between stages

`output_schema` is currently a prompt hint and JSON parse helper, not a strongly enforced pipeline contract.

Plan:

- Use JSON Schema as a real validation contract.
- Save schema versions with artifacts and pages.
- Add validation failures as first-class pipeline results.
- Add schema migrations for reusable research profiles.

### Pipeline gap 6a: compose is single-topic only (multi-topic library not supported)

Current `compose` stage uses `input_mode = "aggregate"` which merges **all** synthesis pages into one wiki document. This works well when a user keeps one research topic per data directory (the current intended usage), but breaks down for users who maintain a large mixed-topic literature corpus.

The problem: concepts extracted from different research topics are structurally identical — there is no topic-level signal in the pipeline that would cause compose to split them. LLM receives all synthesis pages at once and produces one merged narrative, which is typically incoherent across topics.

Two candidate solutions:
- **Tag-based filtering** (low effort): convention that each note is tagged with a topic tag; pipeline is run with `--tag <topic>` to produce per-topic wiki pages. Already works today with minor convention discipline.
- **`group_by_tag` compose** (medium effort): extend the pipeline engine so an `aggregate` stage can have `group_by_tag = true` — it splits all matching synthesis pages into tag groups, runs one compose call per group, and saves each to `research/wiki/<tag>.md`. No per-run invocation needed. Requires a new pipeline engine capability.

**Milestone assignment**: M4 or later (not blocking M3). Implement tag-based convention as the interim guidance; build `group_by_tag` when multi-topic corpus users are confirmed.

### Pipeline gap 6: incremental logic is heuristic

Incremental behavior currently checks output page timestamps or existing links. This works for early literature-review stages but is fragile for data analysis.

Plan:

- Compute input fingerprints:
  - page content hash,
  - artifact hash,
  - prompt hash,
  - model/version,
  - stage config hash,
  - dependency output hashes.
- Skip only when the full stage fingerprint matches.
- Persist stage-run records instead of relying on output timestamps.

### Pipeline gap 7: context assembly is not token-budget aware enough

`AggregateContent` truncates each page by character count. `LinkedSources` can include all chunks from each source. This is simple but can produce poor source coverage or context overflow.

Plan:

- Add token-budget-aware context packing.
- Support source diversity constraints.
- Support top-k chunks per source by relevance to anchor.
- Support recency, citation, and method/theory filters.
- Emit a manifest of included and excluded sources.

### Pipeline gap 8: quality gates are hard-coded

Synthesis quality validation is currently specialized to synthesis pages. Different research tasks need different validators.

Plan:

- Add configurable validators:
  - citation coverage,
  - source diversity,
  - minimum primary-source ratio,
  - no unsupported claims,
  - numeric consistency,
  - table schema validity,
  - statistical test assumptions,
  - missing-data threshold,
  - reproducibility check.
- Allow validators per stage in TOML.

Example:

```toml
[[stages.validators]]
type = "citation_coverage"
min_citations_per_section = 1

[[stages.validators]]
type = "numeric_consistency"
source_artifact = "analysis_results"
```

### Pipeline gap 9: no first-class provenance trail

High-quality research needs to answer: which input, prompt, model, code, and dataset produced this claim?

Plan:

- Add pipeline run tables:
  - runs,
  - stage_runs,
  - stage_inputs,
  - stage_outputs,
  - artifacts,
  - metrics.
- Record:
  - model/provider,
  - prompt name and hash,
  - profile name and hash,
  - input hashes,
  - output hashes,
  - cost/latency,
  - validation status.

### Pipeline gap 10: data analysis needs an execution handoff

For quantitative research, the system must run code or SQL safely. LLM-only analysis is not acceptable. The execution sandbox should live in ZeroClaw, because ZeroClaw already owns shell execution, file operations, approvals, tool receipts, and runtime sandboxing.

Plan:

- Add an rbrain-side handoff contract for ZeroClaw-executed analysis.
- ZeroClaw runs Python/R/SQL and produces scripts, logs, tables, charts, and result JSON.
- rbrain registers those outputs as artifacts with hashes, metadata, and provenance links.
- rbrain validates that findings cite registered result artifacts and datasets.
- rbrain stores notebooks/scripts as versioned artifacts, but does not need to execute them itself.
- Use ZeroClaw's approval gates and tool receipts for generated code execution.

## Proposed data-analysis profile architecture

A future `data_analysis` profile should be a research protocol and validation workflow around ZeroClaw execution:

```text
create_research_run
  -> register_dataset
  -> record_analysis_plan
  -> handoff_to_zeroclaw_for_execution
  -> register_analysis_artifacts
  -> validate_result_provenance
  -> validate_findings
  -> compose_research_memo
```

Execution-heavy stages such as profiling columns, assessing data quality, running descriptive statistics, modeling, and charting should be done by ZeroClaw. rbrain should record their inputs and outputs, then validate and synthesize them.

Recommended page/artifact split:

- datasets and computed results live as artifacts.
- research questions, assumptions, findings, and interpretations live as pages.
- graph edges connect findings to source tables, variables, code, and cited literature.

Recommended MCP additions for this profile:

- `brain_create_research_run`
- `brain_register_dataset`
- `brain_register_artifact`
- `brain_record_analysis_plan`
- `brain_record_finding`
- `brain_validate_research_run`
- `brain_get_research_protocol`

## Suggested implementation order (superseded — see Gap → Milestone mapping below)

The original 7-step ordering is kept here for historical reference, but the live sequencing is the M0–M5 milestone plan in `../rbrain-hub-execution-plan.md`.

1. Add pipeline run/stage-run persistence and fingerprint-based incremental checks. *(→ M5)*
2. Add artifact model and `save_artifact` output mode. *(→ M5; M1 uses frontmatter + `research_runs` table only)*
3. Add research-run, dataset, artifact, analysis-plan, and finding page/artifact types. *(→ M1, with `research_runs` table from day one)*
4. Add configurable validators. *(→ M1 minimal set; M2 full enum; M3 literature-review-specific)*
5. Add token-budget-aware context packing for `LinkedSources` and `AggregateContent`. *(→ M3)*
6. Add `data_analysis.toml` as a protocol/validation profile, not as a local execution runner. *(→ M1 as a protocol state machine, not a TOML profile yet)*
7. Add eval fixtures for literature review and data analysis workflows. *(→ M1 fixtures; reused as regression baseline through M5)*

## Gap → Milestone mapping

Each gap above maps to a milestone in the v2 execution plan:

### Top gaps

| Gap | Milestone | Note |
|---|---|---|
| #1 Sparse retrieval real or report degradation | **M0** | First action: doctor warning + `--explain` marks sparse=disabled. Real sparse fallback can slip to M4. |
| #2 Explainable retrieval and diagnostics | **M0** (`query --explain` v1) → **M4** (`search diagnose`, full attribution) | M0 ships dense/BM25/RRF attribution; sparse + advanced diagnostics in M4. |
| #3 Page-level max-pooling | **M3** | Required for literature-review quality. |
| #4 Use query cache | **M4** | Cache hit/miss visible in `--explain`. |
| #5 Embedding throughput/reliability | **M5** (or as needed earlier if fixtures hit reliability issues) | Not blocking for M1 correctness. |
| #6 Job system as default execution substrate | **M5** | Tied to stage-run persistence. |
| #7 Academic metadata as structured fields | Cross-cutting; partial in **M1** frontmatter (`dataset`, `artifact`, `finding`). Full structured-field expansion in **M5**. |
| #8 Extended graph semantics | **M2** (the research provenance edge set is the M2 deliverable) |
| #9 Citation verification | **M0** minimal (`brain_citation_check` reusing `rbrain audit`) → **M3** (synthesis-aware extensions). |
| #10 Remote MCP security | **out of scope for M0–M3**; loopback-only remains. Schedule alongside or after M5. |

### Pipeline gaps

| Gap | Milestone | Note |
|---|---|---|
| #1 Stages linear not DAG | **M5** (with stage_runs table) |
| #2 Hard-wired to LLM calls | **M2** introduces `protocol`-type processor via the research_run state machine; full processor types in **M5**. |
| #3 Inputs page-centric | **M1** `brain_register_input(kind=dataset\|artifact)` handles the external-artifact case. Schema-typed artifacts in **M5**. |
| #4 Outputs mostly pages | **M1** for artifact registration; **M5** for `save_artifact` output mode in pipelines. |
| #5 No explicit data contracts | **M3** (JSON Schema for synthesis quality outputs) → **M5** (full schema enforcement). |
| #6a Compose single-topic only | **M4** (`group_by_tag` compose); interim: tag-based convention (M3). |
| #6 Heuristic incremental logic | **M5** (fingerprint-based). |
| #7 Token-budget-aware context packing | **M3** |
| #8 Hard-coded quality gates | **M2** (validator enum), **M3** (literature-specific gates) |
| #9 No first-class provenance trail | **M2** edges; **M5** stage_runs/artifacts tables |
| #10 Data-analysis execution handoff | **M1** (protocol + register_input + record + validate establish the handoff contract) |

## Locked decisions (from execution plan review)

1. **M1 ships the `research_runs` table** (migration `0013`). Page is the rendering layer; the table is the source of truth for run identity, status, and fingerprint.
2. **MCP tool surface is consolidated to 5–6 tools** in M1:
   - `brain_create_research_run`
   - `brain_register_input(kind=dataset|artifact)`
   - `brain_record(kind=finding|limitation|analysis_plan)`
   - `brain_validate_research_run`
   - `brain_citation_check`
   - `brain_evidence_check`
3. **M0 runs in parallel with M1** to give M3 an observable retrieval/citation foundation.
4. **Protocol is a state machine bound to `run_id`**, derived from validators — not a static checklist, not in frontmatter.
5. **Artifact storage policy is determined by `artifact_kind`** (not "optional"); see execution plan Phase 1.
6. **Validators differentiate `finding.status = draft` (warn) from `claim/validated` (fail)** to avoid noisy feedback during exploratory work.
7. **`suggested_actions` uses a controlled enum with payload schemas**, enabling ZeroClaw "auto-fix where safe" loops.
8. **Evidence / citation / source-diversity / gap-analysis primitives live in `rbrain-engine::evidence/`**, shared between literature-review and data-analysis profiles.

## Near-term ZeroClaw integration

For a lightweight academic agent, ZeroClaw should call rbrain through MCP while rbrain owns the research state.

Initial deployment shape:

```text
ZeroClaw agent runtime
  -> rbrain MCP server over stdio or loopback HTTP
  -> rbrain SQLite/Tantivy/LanceDB storage
```

Keep rbrain HTTP MCP on loopback until auth/scopes are implemented.

Operational loop:

```text
1. ZeroClaw receives a research request.
2. ZeroClaw creates or retrieves a research run in rbrain.
3. rbrain returns the relevant protocol, prior literature, and evidence constraints.
4. ZeroClaw executes analysis or writing tasks.
5. ZeroClaw registers artifacts, scripts, logs, and findings back into rbrain.
6. rbrain validates provenance, citation coverage, and method completeness.
7. ZeroClaw revises or reruns work if rbrain reports gaps.
8. rbrain stores the final research memo and links it to all supporting evidence.
```
