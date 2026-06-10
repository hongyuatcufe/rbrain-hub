# rbrain-hub improvement plan

This note records the code-review findings and roadmap for evolving rbrain-hub into the research memory and quality layer for a lightweight academic research agent stack.

> **Status (2026-06-10)**: M0–M3 全部上线（commit `8c98000`），105 测试全绿。下一阶段 M4+ 路线图已锁定，详见本文档底部 "M4+ 路线图：多租户 SaaS 学术研究记忆层" 章节。早期 gap analysis 仍作为 inventory 参考，但 milestone 顺序按 M4+ 路线图执行。
>
> **M0 + M1 shipped** in commit `0819ae6`: research_runs migration, evidence/research engine modules, 7 consolidated MCP tools, doctor sparse warn, `query --explain` v1, 9-test data_analysis fixture.
>
> **M2 shipped**: `ResearchEdge` vocabulary (12 edges), real graph-traversal `brain_evidence_check` returning `EvidenceChain`, new `brain_provenance_of(slug)` tool, `SuggestedAction::RecordAnalysisPlan`, 8-test m2_provenance fixture.
>
> **M3 shipped (commit `8c98000`)**: 5 个 slice 全部完成 — lit-review validators + synthesis quality core (S1)、`primary_source_ratio` + `bibliography_consistency` + snippet enrichment (S2)、`gap_analysis` + `contradiction` pipeline stages 与 validators (S3)、page-level max-pooling (S4)、token-budget aware context packing (S5)。新工具 `brain_verify_citations`（解耦 citation verification + `CnkiRefParser` + `pub_metadata` pipeline + 4 个 verify prompt 文件）。28 个 literature_validators fixture。
>
> **M3 后审查修复**（同 commit）：5 个 review-found issue 全部修复（CLAUDE.md 工具表同步、`primary_source_ratio` 的 N+1 SQL 改批量、`bibliography_consistency` SuggestedAction 字段误用、lit-review validator 注释更新、execution plan M3 ✅）。
>
> **下一阶段 M4+**：从单租户单语料 brain 演化为 **多用户多项目 SaaS 学术研究记忆层**，配合 ZeroClaw（admin 工作台）+ 新写轻量 agent runtime（用户端），支撑 200+ 期刊监控 + 学术观点推送 + 文献综述协助。详见本文档底部章节。
>
> **M4 PR-1 进行中（2026-06-10）**：
> - **PR-1a shipped (`2e024df`)**：migration 0014 (tenancy columns) + 0015 (projects 表) + `TenantContext` / `ProjectStore` 模块 + 9-test `project_lifecycle_fixture`。
> - **PR-1b shipped (`3065389`)**：Engine 8 个核心方法 (`put_page` / `get_page` / `delete_page` / `list_pages` / `add_link` / `outlinks` / `backlinks` / `put_page_force`) 新增 `_with_ctx` tenant-aware 变体；新增 `TenantView<'a>` 包装器 + `Engine::with_tenant()` 入口；8-test `tenant_view_fixture` 覆盖 read/write isolation、global 可见、跨 tenant 拒绝、delete guard 等不变式。
> - **PR-1c security hardening shipped (2026-06-11)**：核心检索/graph/stats/chunk 回取新增 `_with_ctx`；MCP stdio + HTTP 核心工具支持可选 `user_id` / `project_id`；worker embed job 继承 tenant；`brain_evidence_check` / `brain_provenance_of` 切 scoped 版本；新增 migration 0016 修复 tenant-aware `page_stats` trigger；`tenant_view_fixture` 扩到 11 测试。
> - **测试**：131/131 ✅（65 lib + 10 data_analysis + 8 m2 + 28 literature + 9 project + 11 tenant_view）。`cargo check --workspace` / `rbrain-mcp --no-default-features --lib` / `rbrain-worker --lib` / `git diff --check` 均通过。
> - **PR-1c 剩余后置**：lit-review validators tenant scope、pipeline/dream cycle tenant 注入、CLI 显式 tenant/project context、维护命令（remove_link/orphan/stale/fix）tenant 收口。这些继续后置，不阻塞 PR-2。

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

---


---

# M4+ 路线图：多租户 SaaS 学术研究记忆层

本章节是 2026-06-10 批准的新路线图全文。本地工作的 plan 文件副本在 `.claude/plans/zeroclaw-rbrain-hub-codex-fancy-dolphin.md`（不入 git）；以下内容是同一文档的完整版本，托管在仓库里以便跨机器共享、对齐 codex / Claude Code / 团队成员。

## Context

M0–M3 已经全部上线（commit `8c98000`）：研究运行表、validator 体系、provenance 图、文献综述质量门、引文验证工具。105 个测试全绿。

**rbrain-hub 当前定位**：单租户单语料的研究记忆层，假设"一个 brain = 一个研究员 = 一个语料"。

**下一个产品形态**：把 rbrain-hub 升级成**多用户多项目的学术研究记忆层**，配合两个 agent 端（ZeroClaw 当 admin 后台、新写的轻量 agent runtime 给 SaaS 用户），支撑一个监控 200+ 期刊、给研究员推送学术观点、协助文献综述的产品。

## 锁定的设计决策（按讨论顺序）

1. **存储不迁 Postgres**：SQLite + WAL 足够支撑 200 期刊 × 几百用户。等真出现并发瓶颈再独立 milestone 评估迁移。
2. **`projects` 表是一等公民**：一个 project = 多个 research_run（lit_review + data_analysis 可共存）。
3. **数据分两层 tenancy**：
   - 源数据 `raw|note`：可以是 `user_id='global'`（期刊）或 `(user_id, project_id)`（用户笔记）。
   - 派生物 `concept|synthesis|wiki|gap_analysis|contradiction_note|finding|limitation|memo|draft`：**强制 `(user_id, project_id)`**，**没有 global 派生物**（避免英文期刊概念污染中国语境项目）。
4. **项目源范围 = 主题订阅**：项目声明 topic/keyword。Ingestion 给文章打 topic 标签。dream cycle 只读 "用户自己笔记 + 匹配 topic 的全局期刊"。
5. **保留项目文件夹模型**：`/users/{user}/{project}/` 仍是 markdown 文件夹，`rbrain sync` / `rbrain export` 在文件夹 ↔ 单 DB 之间双向同步。
6. **三层 tenant 模型**：
   - `user_id='global'`：pipeline 写入，所有用户按 topic 订阅可读
   - `user_id='admin'`：ZeroClaw 管理员写入，admin 自己 + 通过 publish/push 对用户可见
   - `user_id=<user>`：用户自己写入，互不可见
7. **ZeroClaw 是 admin/平台运营工作台**，**不是对外的 user agent**：
   - 跑 pipeline（期刊抓取、topic 标签、定期 topic_digest 生成）
   - 编辑工作（编辑推荐、topic 分类维护）
   - 平台数据分析（匿名聚合）
   - **永远不直写**用户 project 内容，所有对用户的影响走 push/subscribe
8. **对外 SaaS agent runtime 重新写**（不 fork ZeroClaw 的 gateway）。多租户从 day 1 支持，每个 session 带 `(user_id, project_id)`。
9. **研究日志层（个人空间）**：加 `daily|meeting|person|idea|reading` page types，**`user_id` 绑定、`project_id=NULL`**，dream cycle 默认不消费（避免污染项目语境），用户可显式挂载到 project（建 `inspired_by` / `discussed_in` 边）。
10. **学术观点推送 = topic_digest 两层架构**：
    - **Layer 1（ZeroClaw 跑）**：周期性给每个活跃 topic 生成 `topic_digest`（复用 M3 lit_review profile，迷你配置），落到 global 空间。每 topic 一份，订阅者共享。
    - **Layer 2（agent runtime 跑）**：用户访问时读 project state + 近期 topic_digest，做轻量个性化排序，写 push_inbox。
11. **push_inbox 是用户和 admin/pipeline 之间唯一的通道**：所有推送都 opt-in，**永远不直接改用户内容**。

---

## M4 — 多租户基础 + 三层 tenant + 项目文件夹

**进度**（2026-06-10）：

| Sub-PR | 内容 | 状态 |
|---|---|---|
| PR-1a | migration 0014/0015 + TenantContext/ProjectStore modules + project_lifecycle_fixture | ✅ shipped `2e024df` |
| PR-1b | Engine 8 个 `_with_ctx` 变体 + `TenantView` wrapper + tenant_view_fixture | ✅ shipped `3065389` |
| PR-1c | Tenant safety hardening：search/graph/stats/chunk 回取、MCP stdio+HTTP、worker embed job、provenance/evidence_walk、page_stats trigger | ✅ partial shipped 2026-06-11 |
| PR-1c-remain | Lit-review validators tenant scope + pipeline/dream cycle tenant 注入 + CLI context + 维护命令收口 + 既有测试 wrapper 迁移 | ⏸️ 后置（不阻塞 PR-2） |
| PR-2 | migration 0016–0020 (project_topics / page_topics / push_inbox / topic_digests / research_runs.project_id) + store 模块 | 🚧 下一步 |
| PR-3 | 7 新 MCP 工具（5 project 管理 + 3 push inbox）+ `brain_create_research_run` 加 project_id | 待办 |
| PR-4 | CLI `rbrain project` 子命令 + context 注入 + `.rbrain/context.toml` | 待办 |
| PR-5 | `rbrain sync` / `rbrain export` 文件夹 ↔ 单 DB 双向同步改造 | 待办 |

### 4.1 Schema 改造（migration 0014–0020）

- **0014_tenancy_columns.sql** ✅ shipped (PR-1a)
  ```sql
  ALTER TABLE pages         ADD COLUMN user_id TEXT NOT NULL DEFAULT 'default';
  ALTER TABLE pages         ADD COLUMN project_id TEXT;
  -- 同上为 chunks / links / page_stats / jobs
  ```
  - `user_id='default'` 是迁移兼容值；新数据强制非空且非 default
  - 所有索引重建为 `(user_id, ...)` 前缀
  - 保留 `user_id='global'` / `'admin'` 作为系统专用 tenant

- **0015_projects.sql** ✅ shipped (PR-1a)
  ```sql
  CREATE TABLE projects (
      id TEXT PRIMARY KEY,
      slug TEXT NOT NULL,
      owner_user_id TEXT NOT NULL,
      title TEXT NOT NULL,
      description TEXT,
      status TEXT NOT NULL CHECK(status IN ('active','archived','complete')) DEFAULT 'active',
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      UNIQUE(owner_user_id, slug)
  ) STRICT;
  ```

- **0016_project_topics.sql** — 项目订阅哪些 topic
  ```sql
  CREATE TABLE project_topics (
      project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
      topic TEXT NOT NULL,
      weight REAL NOT NULL DEFAULT 1.0,
      created_at TEXT NOT NULL,
      PRIMARY KEY(project_id, topic)
  ) STRICT;
  ```

- **0017_page_topics.sql** — 文章被打的 topic 标签
  ```sql
  CREATE TABLE page_topics (
      page_slug TEXT NOT NULL,
      user_id TEXT NOT NULL,
      topic TEXT NOT NULL,
      confidence REAL,
      created_at TEXT NOT NULL,
      PRIMARY KEY(user_id, page_slug, topic)
  ) STRICT;
  CREATE INDEX idx_page_topics_topic ON page_topics(topic, user_id);
  ```

- **0018_research_runs_project.sql**
  ```sql
  ALTER TABLE research_runs ADD COLUMN project_id TEXT NOT NULL DEFAULT 'unassigned';
  ```

- **0019_push_inbox.sql**
  ```sql
  CREATE TABLE push_inbox (
      id TEXT PRIMARY KEY,
      user_id TEXT NOT NULL,
      project_id TEXT NOT NULL,
      source_slug TEXT NOT NULL,
      source_user_id TEXT NOT NULL,
      push_kind TEXT NOT NULL CHECK(push_kind IN (
          'subscription','agent_recommendation','editorial','topic_digest'
      )),
      reason TEXT,
      status TEXT NOT NULL CHECK(status IN ('pending','accepted','dismissed')) DEFAULT 'pending',
      created_at TEXT NOT NULL,
      actioned_at TEXT
  ) STRICT;
  CREATE INDEX idx_push_inbox_user ON push_inbox(user_id, project_id, status, created_at DESC);
  ```

- **0020_topic_digests.sql**
  ```sql
  CREATE TABLE topic_digests (
      id TEXT PRIMARY KEY,
      topic TEXT NOT NULL,
      period_start TEXT NOT NULL,
      period_end TEXT NOT NULL,
      page_slug TEXT NOT NULL,           -- 对应 global 空间里的 page
      source_count INTEGER NOT NULL,
      generated_at TEXT NOT NULL,
      UNIQUE(topic, period_start, period_end)
  ) STRICT;
  ```

### 4.2 Engine — TenantContext

新模块 `crates/rbrain-engine/src/research/tenant.rs`：

```rust
#[derive(Debug, Clone)]
pub struct TenantContext {
    pub user_id: String,
    pub project_id: Option<String>,
}

impl TenantContext {
    pub fn global() -> Self { ... }
    pub fn admin() -> Self { ... }
    pub fn for_user(user_id: &str, project_id: Option<&str>) -> Self { ... }
    /// 检索时可见的 user_id 池：自己 + 'global'（不含 'admin' 除非显式订阅）
    pub fn readable_user_ids(&self) -> Vec<&str> { ... }
    pub fn is_admin(&self) -> bool { ... }
}
```

所有 Engine query 函数加 `&TenantContext` 参数。主要改造（按文件）：

- `crates/rbrain-engine/src/engine.rs` — `put_page` / `get_page` / `list_pages` / `keyword_search` / `vector_search` / `hybrid_search` / `expanded_search` / `add_link` / `outlinks` / `backlinks` / `graph_query` 全部加 tenant 上下文 + WHERE 子句改写
- `crates/rbrain-engine/src/evidence/validators.rs` — 所有 lit-review validator 改 `(user_id, project_id)` 切片，去掉现在的全库扫描（关掉 M3 留下的 B3）
- `crates/rbrain-engine/src/evidence/{provenance,evidence_walk}.rs` — 加 tenant scope
- `crates/rbrain-engine/src/pipeline.rs` — `InputSpec::SelfContent` / `AggregateContent` / `LinkedSources` 加 tenant filter。dream cycle 绑定到 `(user_id, project_id)`，源集计算：
  ```sql
  -- 项目可见源数据 = 自己 + topic 匹配的 global
  SELECT * FROM pages p
  WHERE p.page_type IN ('note', 'raw')
    AND (
        (p.user_id = :user_id AND p.project_id = :project_id)
        OR (p.user_id = 'global' AND EXISTS (
              SELECT 1 FROM page_topics pt
              JOIN project_topics ppt ON ppt.topic = pt.topic
              WHERE pt.page_slug = p.slug
                AND pt.user_id = 'global'
                AND ppt.project_id = :project_id
        ))
    )
  ```

### 4.3 Research 模块扩展

- 新模块 `crates/rbrain-engine/src/research/projects.rs`：
  - `Project` struct
  - `ProjectStore::create / get / find_by_slug / list_by_user / set_status / set_topics`
- `ResearchRunStore::create` 加 `project_id` 必填参数

### 4.4 MCP 工具改造

**新工具（项目管理）**：

| 工具 | 作用 |
|---|---|
| `brain_create_project` | `{ slug, title, description?, topics: [...] }` → `project_id` |
| `brain_list_projects` | `{}` → 当前 user 的所有 project |
| `brain_get_project` | `{ project_id }` → 详情 + topics + 关联 run |
| `brain_archive_project` | `{ project_id }` |
| `brain_set_project_topics` | `{ project_id, topics: [...] }` |

**新工具（push inbox）**：

| 工具 | 作用 |
|---|---|
| `brain_list_inbox` | `{ project_id?, status? }` → 用户的待办推送 |
| `brain_accept_push` | `{ push_id }` → 在 project 里建引用边，标记 accepted |
| `brain_dismiss_push` | `{ push_id }` |

**修改既有工具**：

- `brain_create_research_run` 加必填 `project_id` 参数
- 所有 read 类工具（`brain_query` / `brain_get` / `brain_list` / `brain_graph` / `brain_think` / `brain_citation_check` / `brain_evidence_check` / `brain_provenance_of` / `brain_verify_citations`）从调用上下文取 `user_id` 和 `project_id`（不在参数里暴露，避免客户端伪造）

**Tenant 注入**：M4 阶段先做 dev-mode（环境变量 / config 读默认 user/project）。完整 auth 在 M10 production gateway 做。

### 4.5 CLI 改造

- 新增 `rbrain project create/list/show/archive/set-topics`
- 既有命令加上下文参数：`rbrain --user-id alice --project-id phd-dissertation <cmd>`
- 默认 context：`rbrain context set ...` 写到 `.rbrain/context.toml`，后续命令默认读

### 4.6 文件夹 ↔ 单 DB 双向同步

```
/users/
└── alice/
    └── phd-dissertation/
        ├── research/
        │   ├── findings/*.md
        │   ├── synthesis/*.md
        │   └── runs/*.md
        ├── notes/*.md
        ├── .rbrain/
        │   └── context.toml          ← (user_id, project_id) 绑定
        └── README.md
```

- 单 DB 在 `~/.rbrain/global.db`（或服务端的 data 目录）
- `rbrain sync /users/alice/phd-dissertation` → 读 context → 扫 markdown → 写共享 DB（带 tenant 标签）
- `rbrain export /users/alice/phd-dissertation` → DB 中筛选该 tenant 行 → 写 markdown

### 4.7 测试 / fixture

- 现有 105 测试改造：默认 `TenantContext::for_user("default", Some("test"))`，向后兼容
- 新增 `multi_tenant_fixture`：2 user × 2 project × 5 global page，验证：
  - 用户互相不可见
  - 同用户跨 project 派生物不可见
  - global raw 按 topic 订阅对项目可见
  - dream cycle source set 严格按 topic 过滤
  - cross-tenant citation 被拒绝
- 新增 `project_lifecycle_fixture`：create → list → archive → restore
- 新增 `push_inbox_fixture`：写 push → list → accept（建引用边）/ dismiss

### 4.8 关键文件清单（M4）

| 文件 | 工作 |
|---|---|
| `migrations/0014–0020_*.sql` | 7 个新 migration |
| `crates/rbrain-engine/src/research/tenant.rs` | 新增 |
| `crates/rbrain-engine/src/research/projects.rs` | 新增 |
| `crates/rbrain-engine/src/research/push_inbox.rs` | 新增 |
| `crates/rbrain-engine/src/research/topic_digests.rs` | 新增（store 部分，generator 在 M5） |
| `crates/rbrain-engine/src/research/mod.rs` | 导出新模块 |
| `crates/rbrain-engine/src/research/store.rs` | `ResearchRunStore::create` 加 project_id |
| `crates/rbrain-engine/src/engine.rs` | 所有 query 加 `&TenantContext` |
| `crates/rbrain-engine/src/evidence/validators.rs` | tenant scope + 删 B3 stale comment |
| `crates/rbrain-engine/src/evidence/{provenance,evidence_walk}.rs` | tenant scope |
| `crates/rbrain-engine/src/pipeline.rs` | InputSpec topic-aware source set |
| `crates/rbrain-mcp/src/lib.rs` | 7 新工具 + 既有工具改写 |
| `crates/rbrain-cli/src/main.rs` | `project` 子命令 + context 注入 |
| `crates/rbrain-engine/tests/multi_tenant_fixture.rs` | 新 fixture |
| `crates/rbrain-engine/tests/project_lifecycle_fixture.rs` | 新 fixture |
| `crates/rbrain-engine/tests/push_inbox_fixture.rs` | 新 fixture |
| `CLAUDE.md`（仓库根 + rbrain-hub） | 同步 tenancy 设计、三层模型、push 规则 |
| `rbrain-hub-execution-plan.md` | M4 ✅ + 后续 milestone 重排 |

**复用（CLAUDE.md §7）**：

- `Engine::add_link` 不动签名，sqlx 加 tenant 检查防跨 tenant 建边
- `extract_links` / `audit_citations` / `brain_verify_citations` 不动核心算法
- `ResearchEdge` 12 个边类型不变；M4.5 加新边

---

## M4.5（小） — 研究日志层（简化版 gbrain）

### 4.5.1 新 page types

- `daily` — 日记，slug 模板 `daily/YYYY-MM-DD`
- `meeting` — 会议笔记
- `person` — 研究人员/合作者档案
- `idea` — 想法卡片
- `reading` — 文献阅读笔记（轻量版，非 finding）

### 4.5.2 新 edge types（个人空间 → 项目桥）

- `inspired_by` — finding/synthesis 指向 idea 或 reading
- `discussed_in` — finding 指向 meeting

更新 `crates/rbrain-engine/src/research/edges.rs` 的 `ResearchEdge` 枚举从 12 → 14。

### 4.5.3 Tenancy 规则

- 上述 5 个 page types 强制 `user_id` 非空，`project_id` 可空（默认 NULL = 个人空间）
- Dream cycle **默认不读这层**（避免污染项目语境）
- `brain_query` / `brain_think` 接受 `include_personal: bool` 参数（默认 false），true 时把个人空间纳入检索结果
- 用户显式建 `inspired_by` / `discussed_in` 边时，那个 reading/idea/meeting 算作 project 源的一部分（"显式同意纳入"）

### 4.5.4 新 MCP 工具

- `brain_record_personal(kind, slug, title, content)` — 个人空间写入（kind ∈ daily/meeting/person/idea/reading）
- `brain_link_to_project(personal_slug, project_id, edge_type)` — 显式把个人 page 挂到 project

### 4.5.5 工作量

约相当于 M4 的 1/3。可以跟 M4 合并在一个 sprint。

---

## M5 — 期刊 Ingestion + Topic 标签 + Topic Digest 周报生成

### 5.1 期刊抓取

- 新 crate `crates/rbrain-ingestion/`（或在现有 worker 里加 module）
- 支持的来源：RSS、CrossRef API、CNKI（如有 API）、GScholar（爬虫，谨慎）
- 每个期刊配一个 source descriptor（YAML 配置）
- 输出：`raw` page，slug 形如 `raw/articles/{journal}/{year}/{slug}`，`user_id='global'`

### 5.2 Topic 标签（M5 阶段用关键词规则；后续可升级 LLM）

- 配置文件 `~/.rbrain/topics.yaml`：
  ```yaml
  topics:
    - id: china_basic_education
      label: "中国基础教育"
      keywords: ["基础教育", "义务教育", "中小学"]
      regions: ["china"]
    - id: education_equity
      label: "教育公平"
      keywords: ["教育公平", "教育均衡", "educational equity"]
  ```
- Ingestion worker 拉到新文章后跑 topic tagger：
  - 阶段 1（M5）：关键词匹配，置信度按 keyword 命中数
  - 阶段 2（后续）：换成 LLM 分类器
- 输出写入 `page_topics` 表

### 5.3 Topic Digest 周报生成

- ZeroClaw cron job（每周三 03:00）：
  1. 扫所有有订阅者的 topic（`project_topics` 表的 distinct topic）
  2. 对每个 topic：调 `brain_generate_topic_digest(topic, period_start, period_end)`
  3. 该工具内部跑一次 mini lit_review profile（max_pages=20, min_sources=3），输出一个 `topic_digest` 类型的 page 到 global
  4. 写入 `topic_digests` 表登记
  5. fan-out：对每个订阅该 topic 的 project 写一条 `push_inbox` 记录（push_kind='topic_digest'）

### 5.4 新 MCP 工具

- `brain_generate_topic_digest(topic, period_start, period_end)` — ZeroClaw 调用
- `brain_list_topic_digests(topic, limit)` — 列出某 topic 的近期 digest
- `brain_publish_editorial(topic, content)` — admin 写编辑推荐，自动 push 到订阅者

### 5.5 ZeroClaw 编排

- ZeroClaw 加一个 `rbrain-pipeline` skill / cron 模板
- 调度任务：每日 ingestion、每周 topic_digest、月度 cleanup
- 失败重试、监控告警

---

## M6 — Citation Accuracy 强化

- `brain_verify_citations` 误报分析：建一个 fixture（10 篇有人工标注 ground truth 的文档），跑当前 verify，统计 FP/FN
- 新增非 CNKI bib parser：
  - APA 7th edition
  - GB/T 7714（中国国标）
  - JCR / Vancouver
- 跨语言匹配（同一作者中英文名变体）
- Hallucination 检测：cite 了不存在于语料的文章 → 标 `bib_missing` / `bib_fabricated`
- 增强 `synthesis_sections_have_citations` 的 thin-citation 检测

工作量：~M3 的 1/2。

---

## M7 — Generality 完整化（非 lit_review 研究方法）

- `data_analysis` 做到 M3 深度：
  - 加 validator：`script_registered_for_result`、`codebook_exists_or_exempted`、`numeric_claim_has_result_reference`（基于 finding frontmatter 的结构化 claims 数组）
  - 加 fixture `data_analysis_deep_fixture`
- `mixed_methods` / `theory_building` 实质化：
  - 它们目前共用 `(analysis_plan, artifact_hash, finding_supports)` 三个 validator —— 太少
  - 加 method-specific validator
- 评估是否引入新 TaskType：
  - `systematic_review`（PRISMA 流程）
  - `case_study`（个案研究）
  - `qualitative_coding`（质性编码）

最终交付：所有 4–7 个 TaskType 都有对应深度的 validator + fixture。

---

## M8 — Retrieval Observability（原 M4 计划）

- `rbrain search diagnose "<query>" --target <slug>` — 找出某 slug 为何没被检索到
- Title / alias boost：题目命中给检索打分加权
- Query cache 命中报告：`--explain` 输出命中信息
- Sparse fallback 实现（若 LanceDB Rust SDK 仍无 sparse ANN）

---

## M9 — Durable Run Records（原 M5 计划）

- `artifacts` 表 + `stage_runs` 表
- Pipeline 每个 stage 的输入/输出指纹
- Fingerprint 增量：只在 fingerprint 变了的时候重跑

---

## M10 — Lightweight Agent Runtime + Personalization

**新写**一个对外的 SaaS agent runtime（Rust + Axum），不 fork ZeroClaw。

### 10.1 Runtime 形态

- Rust + Axum HTTP/SSE 服务
- 每个 user session 由 auth token 解出 `(user_id, project_id)`
- 工具调用：loopback 调 rbrain 的 MCP（不走网络）
- Session state 存 rbrain DB（新增 `agent_sessions` 表）
- LLM 调用走 rbrain `jobs` 表的 worker pool

### 10.2 Layer 2 个性化推荐

- 后台 worker：用户打开 project 时触发或周期触发
- 流程：
  1. 读 project 的 synthesis + findings 摘要
  2. 读近期匹配 topic 的 `topic_digest` 列表
  3. 一次 LLM 调用：基于 project state 排序"哪些 digest / 文章对该 project 最相关"
  4. 写 push_inbox（push_kind='agent_recommendation'）

### 10.3 ZeroClaw 角色

- 不参与 SaaS 用户的 agent 会话
- 留作：admin 工作台、个人 agent（团队成员自用）
- ZeroClaw 通过 `user_id='admin'` context 调 rbrain MCP

### 10.4 Auth / 部署

- 简单 token-based auth（M10 v1）
- Postgres 迁移评估（若 SQLite 真的撞天花板）
- pgbouncer / 连池
- Postgres + pgvector + `pg_search`（保留 Tantivy 质量）的 spike

---

## 验证 / 出 M4+M4.5 的判定标准

1. `cargo test -p rbrain-engine --lib` 全绿（≥ 59 + 新加的 tenant/project/push/personal 单测）
2. `cargo test -p rbrain-engine --test multi_tenant_fixture` 全绿
3. `cargo test -p rbrain-engine --test project_lifecycle_fixture` 全绿
4. `cargo test -p rbrain-engine --test push_inbox_fixture` 全绿
5. 既有 `data_analysis_fixture` / `m2_provenance_fixture` / `literature_validators_fixture` 改造后**仍全绿**
6. 手动端到端：
   - 建 2 user × 2 project，跑 `rbrain sync` 各自 markdown
   - 在 project A 跑 `dream --profile literature_review`，确认产出 concept/synthesis 只用 project A 的源 + 匹配 topic 的 global
   - 在 project B 看不到 project A 的派生物
   - 用 admin 身份调 `brain_publish_editorial`，验证推送到订阅 topic 的所有 project 的 inbox
   - 用户 accept push，验证在 user project 里建了 `cites` 边
   - `brain_provenance_of` 不跨 tenant 返回边
7. `cargo check --workspace` 无 error
8. CLAUDE.md 三处同步：MCP 工具表、tenancy 规约、push 规则、个人空间规约

## 输出 M4 + M4.5 后仓库长什么样

- ~130 个测试全绿（59 lib + 4 fixture × 平均 12 测试）
- ~32 个 MCP 工具（原 22 + 10 新）
- 20 条 migration（原 13 + 7 新）
- 一个新文档：`docs/tenancy-model.md` 解释三层 tenant、源集计算、push 机制
- 一个新文档：`docs/personal-space.md` 解释研究日志层和与项目的交互规则

## 不在 M4 / M4.5 范围（避免范围爆炸）

- Postgres 迁移（M10 视情况）
- 期刊抓取流水线（M5）
- ZeroClaw 编排集成（M5）
- LLM-based topic 打标签（M5，先用关键词规则）
- HTTP/SSE gateway + auth（M10）
- 轻量 agent runtime（M10）
- 个性化推荐 Layer 2（M10）
- 跨项目 admin 视图 / 分析（暂不需要）
- `brain_verify_citations` 精度优化（M6）

## 路线图全景

| Milestone | 范围 | 依赖 | 工作量估算 |
|---|---|---|---|
| **M4** | 多租户基础 + projects + push_inbox + topic_digests schema | M3 | 3-4 周 |
| **M4.5** | 研究日志层（daily/meeting/person/idea/reading） | M4 | 1 周 |
| **M5** | 期刊 ingestion + topic 标签 + topic_digest 周报生成 + ZeroClaw 编排 | M4 | 3-4 周 |
| **M6** | Citation accuracy 强化 | M3, M4 | 2 周 |
| **M7** | 非 lit_review TaskType 完整化 | M4 | 3 周 |
| **M8** | Retrieval observability | M0 | 2 周 |
| **M9** | Durable run records / fingerprint 增量 | M4, M7 | 2 周 |
| **M10** | Lightweight agent runtime + personalization + production deployment | M4–M9 | 4-6 周 |

总估算：**5-6 个月**到产品 MVP 上线（含基础 SaaS 多租户 + 期刊监控 + 文献综述 + 推送 + 个性化推荐）。

## Non-goals（贯穿所有 milestone）

1. rbrain 不执行任意生成的 Python/R/SQL
2. rbrain 不复制 ZeroClaw 的 shell/browser/file/审批/sandbox
3. ZeroClaw 永远不直写用户 project 内容
4. rbrain 生成的草稿默认不是终稿（用户主动 publish 才算）
5. 个人空间内容默认不进 dream cycle（避免污染项目语境）
6. 跨用户 / 跨项目内容隔离严格，任何 leak 是 hard fail
