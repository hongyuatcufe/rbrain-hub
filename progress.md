---
{}
---
# rbrain 开发进展

## 项目概述

rbrain-hub 是面向学术研究的 Rust 知识库系统，包含 CLI、MCP 服务端和可编程 LLM pipeline。

当前定位正在从 **单租户单语料的学术研究记忆层** 演进为 **多用户多项目的 SaaS 学术研究记忆层**，目标产品是 200+ 期刊监控 + 智能体推送学术观点 + 协助文献综述。详见 `plan.md` 的 M4+ 路线图。

---

## 2026-06-11 — M4 PR-1c 安全收口：tenant scope 贯穿检索/MCP/worker/evidence

Claude M4 PR-1a/1b 后的系统审查发现：新增 tenancy columns 和 `TenantView` 只覆盖了部分 page/link API，`brain_query`、worker embed job、evidence/provenance、`page_stats` trigger 等路径仍可能全库读取或混用 tenant 排序信号。已完成一轮安全收口，目标是让当前远端多用户底座不会在核心检索和图谱路径泄漏其他用户内容。

### 已修复

- **检索 tenant scope**：`keyword_search` / `vector_search` / `sparse_search` / `hybrid_search` / `expanded_search` / `search_with_context` / `fetch_chunks_text` / `fetch_chunk_by_id` 新增 `_with_ctx` 路径，默认 API 继续走 `TenantContext::default_tenant()` 保持兼容。
- **MCP tenant 参数**：stdio MCP 与 HTTP MCP 的核心工具增加可选 `user_id` / `project_id`，`brain_query`、`brain_get`、`brain_put`、`brain_delete`、`brain_list`、`brain_graph`、`brain_backlinks`、`brain_outlinks`、`brain_link`、`brain_think`、`brain_generate`、timeline/tag 工具改走 scoped Engine 方法。
- **写权限硬化**：`put_page_with_ctx` 拒绝跨 tenant slug takeover；`delete_page_with_ctx`、`add_link_with_ctx`、timeline/tag 写入不再把 global 可读权限误当成可写权限，普通用户不能删除或修改 global 页面/边。
- **worker / embedding tenant 继承**：`submit_embed_job` 写入 `user_id` / `project_id`；`EmbedPageHandler` 从 job params 构造 `TenantContext`；`chunk_and_embed_page_with_ctx` 写入 chunks 的 tenant metadata。
- **evidence/provenance tenant scope**：`run_evidence_check_with_ctx` 与 `provenance_of_with_ctx` 已落地，MCP `brain_evidence_check` / `brain_provenance_of` 已切到 scoped 版本。
- **page_stats trigger 修复**：新增 migration `0016_tenant_page_stats_triggers.sql`，按目标页面 owner tenant 维护 indegree，避免其他 tenant 的 links 污染检索 boost。
- **TenantView 扩展**：新增 scoped `search_with_context`、`graph_query`、`get_stats` wrapper。
- **测试扩展**：`tenant_view_fixture` 从 8 测试扩展到 11 测试，新增跨 tenant 搜索不泄漏、global 不可写、同 slug takeover 拒绝、同用户不同 project 隔离。

### 测试状态

- `cargo check --workspace` ✅
- `cargo test -p rbrain-engine --lib`：65/65 ✅
- `cargo test -p rbrain-engine --test tenant_view_fixture`：11/11 ✅
- `cargo test -p rbrain-engine --test m2_provenance_fixture`：8/8 ✅
- `cargo test -p rbrain-engine --test project_lifecycle_fixture`：9/9 ✅
- `cargo test -p rbrain-engine --test data_analysis_fixture`：10/10 ✅
- `cargo test -p rbrain-engine --test literature_validators_fixture`：28/28 ✅
- `cargo test -p rbrain-mcp --no-default-features --lib` ✅
- `cargo test -p rbrain-worker --lib`：1/1 ✅
- `git diff --check` ✅

未运行 `cargo fmt`。

### 仍后置到后续 PR

- Lit-review validators 中仍有全库扫描 SQL，需要单独做 tenant scope。
- Pipeline / dream cycle / CLI 的显式 tenant 注入仍未完成，当前默认兼容路径继续走 `default` tenant。
- `remove_link` / orphan / stale / fix 类维护命令仍是默认兼容语义，远端多用户产品化前需要继续收口。
- SQLite schema 仍保留 `pages.slug` 全局主键；本轮通过写入 guard 阻止跨 tenant takeover，但长期 SaaS 若要求同 slug 多租户并存，需要 schema 级复合键或 slug namespace 策略。

---

## 2026-06-10 — M4 PR-1 启动：Tenancy 基础 + TenantView 包装器

### M4 PR-1a（commit `2e024df`）：Schema + 模块基础

- Migration `0014_tenancy_columns.sql`：`pages` / `chunks` / `links` / `page_stats` / `jobs` 加 `user_id` + `project_id` 列（默认 `'default'`），重建索引为 `(user_id, ...)` 前缀。保留 `'global'` / `'admin'` / `'default'` 三个系统 tenant 值。
- Migration `0015_projects.sql`：`projects(id, slug, owner_user_id, title, description, status, ...)`，`UNIQUE(owner_user_id, slug)`，status ∈ active/archived/complete。
- 新模块 `crates/rbrain-engine/src/research/tenant.rs`：`TenantContext` 类型 + 4 个构造函数（`global()` / `admin()` / `for_user(user, project)` / `default_tenant()`）+ `readable_user_ids()`（用户 = 自己 + global；admin = 仅自己；global = 仅自己）。5 个单测覆盖三层语义。
- 新模块 `crates/rbrain-engine/src/research/projects.rs`：`Project` / `ProjectStatus` / `ProjectStore` CRUD（create / get / find_by_slug / list_by_user / set_status / archive / update_metadata）+ 1 个单测。
- 新 fixture `project_lifecycle_fixture`：9 测试覆盖 create-get-find/duplicate-rejection/cross-user-slug-reuse/status-filter/archive-restore/metadata-update/error 路径。

### M4 PR-1b（commit `3065389`）：Engine 核心 tenant-aware + TenantView 包装器

新增 8 个 `_with_ctx` Engine 方法变体（put_page / put_page_force / get_page / delete_page / list_pages / add_link / outlinks / backlinks），既有方法签名不变，内部走 `TenantContext::default_tenant()` 保持完全向后兼容。

- `put_page_with_ctx`：INSERT pages 行带 `(user_id, project_id)`，自动提取的 wikilinks 同步携带 tenant。
- `get_page_with_ctx` / `list_pages_with_ctx`：`WHERE user_id IN (...)` 过滤（从 `ctx.readable_user_ids()`）。
- `delete_page_with_ctx`：tenant guard，跨 tenant 的 slug 删除被拒绝。
- `add_link_with_ctx`：tenant guard 检查 source_slug 是否属于 caller；INSERT links 行带 `(user_id, project_id)`。
- `outlinks_with_ctx` / `backlinks_with_ctx`：边的 user_id 必须在 readable set 里。

新增 `crates/rbrain-engine/src/research/tenant_view.rs`：`TenantView<'a>` 包装器，让 caller 写 `engine.with_tenant(ctx).put_page(page)` 替代显式 `&TenantContext` 参数。`Engine::with_tenant(ctx)` 入口已暴露。

新 fixture `tenant_view_fixture`：8 测试覆盖：
- user A 看不到 user B 的页面
- global 页面对所有用户可见
- list_pages 按 caller 切片
- list 中合并 caller + global 内容
- add_link 跨 tenant 被拒绝
- outlinks / backlinks 按 readable set 过滤
- delete_page 跨 tenant 被拒绝
- 同 slug 跨 tenant takeover 被拒绝（PK 仍是 slug，安全策略改为写入 guard）

### 测试状态

| Suite | 之前 | 现在 |
|---|---|---|
| `rbrain-engine --lib` | 59/59 | **65/65** ✅（+6 tenant/project 单测） |
| `data_analysis_fixture` | 10/10 | 10/10 ✅ |
| `m2_provenance_fixture` | 8/8 | 8/8 ✅ |
| `literature_validators_fixture` | 28/28 | 28/28 ✅ |
| `project_lifecycle_fixture` | — | **9/9** ✅（新） |
| `tenant_view_fixture` | — | **11/11** ✅（新 + 安全回归） |
| **总计** | 105 | **131** |

`cargo check --workspace` 全绿，无 error。既有 105 测试一行未改，pre-M4 代码路径完整保留。

### PR-1 推迟部分（PR-1c）

下列工作转入 PR-1c：

- **Lit-review validators 的 tenant scope**：目前 lit-review validator 仍是全库扫（M3 留下的 B3 issue）。M4 schema 提供了 user_id 列，但 14 个 validator 的 SQL 还没改。`provenance` / `evidence_walk` 已在 2026-06-11 安全收口中补上 scoped 版本。
- **既有 105 测试的 wrapper 迁移**：当前依赖 backwards-compat，未来要把所有 `engine.put_page(p)` 改成 `view.put_page(p)`。机械工作，约 113 个 call site。
- **Pipeline / CLI 调用站迁移**：MCP 核心工具已支持可选 `user_id` / `project_id` 并走 scoped Engine；CLI 与 pipeline/dream cycle 仍需解耦 tenant 注入逻辑（auth/project context → TenantContext）。

### 路线图更新

- **M4 PR-1**：core tenancy 进行中（PR-1a + PR-1b 完成；PR-1c 安全收口完成，lit-review validators / pipeline / CLI 等剩余项后置）
- **M4 PR-2**：剩余 schema（migration 0016–0020：project_topics / page_topics / push_inbox / topic_digests + research_runs.project_id）+ 对应 store 模块
- **M4 PR-3–PR-5**：见 plan.md M4+ 章节

---

## 2026-06-10 — M3 收尾 + M4+ 多租户路线图锁定

### M0–M3 全部上线（截至 commit `8c98000`）

- **M0**：sparse 降级 doctor 告警、`query --explain` v1、`brain_citation_check` 复用 `rbrain audit`。
- **M1**：`research_runs` 表（migration `0013`）+ 5 个核心 MCP 工具 + 3 个 validator + protocol state machine。9 个 data_analysis fixture。
- **M2**：`ResearchEdge` 词表（12 edges）+ 真实图遍历 `brain_evidence_check`（返回 `EvidenceChain`）+ 新工具 `brain_provenance_of`。8 个 m2_provenance fixture。
- **M3**：5 个 slice 全部完成 — lit-review validators、synthesis quality core、`primary_source_ratio` / `bibliography_consistency` / snippet enrichment、`gap_analysis` + `contradiction` pipeline stages、page-level max-pooling、token-budget-aware context packing。新工具 `brain_verify_citations`（解耦 citation verification + CnkiRefParser + pub_metadata pipeline）。28 个 literature_validators fixture。
- **M3 后审查修复（commit `8c98000`）**：5 个 review-found issue
  - B1 CLAUDE.md 加 `brain_verify_citations` 同步条目
  - B2 `primary_source_ratio` N+1 SQL → 批量 IN-clause
  - B3 lit-review validator 全库扫的注释从 "M3 Slice 3 will do" 改为 "M5 deferred"
  - B4 `bibliography_consistency` 不再把错误消息塞进 `chunk_ref` 字段
  - B5 执行计划 M3 标 ✅ + 列出 5 个 slice

**测试状态**：105 个测试全绿
- `rbrain-engine --lib`：59/59
- `data_analysis_fixture`：10/10
- `m2_provenance_fixture`：8/8
- `literature_validators_fixture`：28/28
- 仅 2 个 pre-existing `integration::test_dream_cycle_flow` 类 failure（与 M0–M3 无关，依赖真实 DeepSeek API）

### M4+ 路线图（新批准 — 见 plan.md "M4+ 路线图" 章节）

产品形态升级：从单租户 brain 升级到多租户 SaaS 学术研究记忆层。

**11 条锁定决策**：
1. 不迁 Postgres，SQLite + tenant_id 列足够支撑 200 期刊 × 几百用户
2. `projects` 表是一等公民（一个 project = 多个 research_run）
3. 数据分两层 tenancy：源数据可 global / 派生物强制 per-project（避免英文期刊概念污染中国语境）
4. 项目源范围 = topic 订阅（ingestion 给文章打 topic 标签，dream cycle 按订阅过滤）
5. 保留项目文件夹模型，markdown 是 source of truth，单 DB 是派生索引
6. 三层 tenant：`global`（pipeline 写）/ `admin`（ZeroClaw 写）/ `<user>`（用户写）
7. ZeroClaw 是 admin/平台工作台，**不是对外 user agent**，永远不直写用户内容
8. 对外 SaaS agent runtime **新写**（不 fork ZeroClaw gateway）
9. 研究日志层（`daily/meeting/person/idea/reading`）个人空间，默认不进 dream cycle
10. 学术观点推送 = 两层架构：Layer 1 topic_digest（ZeroClaw 周期生成，每 topic 一份共享）+ Layer 2 个性化排序（agent runtime 用户态跑）
11. `push_inbox` 是 admin/pipeline → 用户的唯一通道，opt-in，绝不直接改用户内容

**8 个 milestone 规划**：

| Milestone | 范围 | 工作量 |
|---|---|---|
| M4 | 多租户基础 + projects + push_inbox + topic_digests schema | 3-4 周 |
| M4.5 | 研究日志层（个人空间 5 个 page type + 2 个 edge） | 1 周 |
| M5 | 期刊 ingestion + topic 标签 + topic_digest 周报生成 + ZeroClaw 编排 | 3-4 周 |
| M6 | Citation accuracy 强化（多 bib parser、跨语言匹配、hallucination 检测） | 2 周 |
| M7 | 非 lit_review TaskType 完整化（data_analysis 做深、mixed_methods/theory_building 实质化） | 3 周 |
| M8 | Retrieval observability（`search diagnose`、title boost、cache 命中报告、sparse fallback） | 2 周 |
| M9 | Durable run records（artifacts 表、stage_runs 表、fingerprint 增量） | 2 周 |
| M10 | Lightweight agent runtime + Layer 2 个性化 + production deployment | 4-6 周 |

总估算 5-6 个月到 MVP。

### 下一步：M4 启动

M4 涉及 7 个新 migration、新增 4 个 research 模块（tenant/projects/push_inbox/topic_digests）、所有 Engine query 加 `&TenantContext`、所有 lit-review validator 改 tenant scope、7 个新 MCP 工具、3 个新 fixture。文档同步包括 CLAUDE.md（仓库根 + rbrain-hub）和 `rbrain-hub-execution-plan.md`。

---

## 2026-06-08 — Compose 超时修复、embedding 去重实验与引用矫正模块设计

### 1. Compose 阶段超时修复

DeepSeek HTTP client timeout 从 120 s 升至 600 s，修复 compose 阶段（完整文献综述生成，单次 LLM 调用）因生成时间较长而超时的问题。

### 2. Synthesize 去重：embedding 实验与回滚

**实验（commit `a71edb6`）**：在 synthesize 阶段引入 embedding-based 概念去重（cosine similarity ≥ 0.90 时合并近义概念，如"自主知识体系" vs "教育学自主知识体系"）。

**结论（commit `2d30f7e`）**：回滚。synthesize 设计为单概念深度综合，embedding 去重将多概念合并后扩大了上下文，反而降低了合成焦点。跨概念整合是 compose 阶段的职责。

**保留的改进**：LinkedSources 锚点列表在 dedup/embedding 步骤之前先按 `min_sources` 过滤。实测将候选从 225 个降至 25 个，减少了不必要的 embedding 计算。

### 3. 引用核校（rbrain-test 测试项目）

对 `research/draft/disciplinary_genealogy_paper.md`（学科谱系论文）进行全量引用核校：

- 逐一比对文中所有引文（作者、年份、期刊）与 `raw/articles/` 原始文件
- 发现并修正：年份错误、期刊名错误、联合作者缺失等，共 19 处
- 特殊情况：袁振国（2022）为书序性质，原始文件无出版元数据，标注"出版信息待核"

### 4. 引用矫正模块：完整设计（Tasks #32–#41）

设计三个协作组件，**代码尚未实现**，下次 session 继续：

#### 组件1：PubMetadata 提取

| 来源 | 实现 | confidence |
|------|------|-----------|
| CNKI 导出文件（`ref_entry` 页） | `CnkiRefParser`：纯 Rust regex，解析 `EnglishKey-中文Key: value` 格式 | `"high"` |
| note 文件 header 自动提取 | flash 模型 + `extract_pub_metadata` prompt | `"medium"` |

CNKI 优先策略：两个 stage 共享同一 `output_slug_prefix`，CNKI 先跑，auto 阶段通过 `skip_if_target_exists = true` 看到已有页面则跳过。

#### 组件2：compose 注入

`inject_pub_metadata = true` 在 compose 前查询所有 `pub_metadata` 页，置信度去重后构建元数据表，注入 compose prompt system section，使 LLM 生成文献综述时直接引用正确的作者/年份/期刊。

#### 组件3：post-hoc 引用核校报告

`verify_citations` stage（aggregate 模式）：读取所有 synthesis 页，与 pub_metadata 表逐一比对，输出差异报告页（`citation_report` 类型）。可重跑、可审计，独立于生成流程。

#### 新增 StageConfig 字段

| 字段 | 类型 | 用途 |
|------|------|------|
| `model_tier` | `Option<String>` | `"none"` 绕过 LLM 用 CnkiRefParser；`"flash"` / `"pro"` 显式指定 |
| `inject_pub_metadata` | `bool` | compose 阶段注入 pub_metadata 表 |
| `skip_if_target_exists` | `bool` | 目标 slug 已存在则跳过（实现 CNKI 优先覆盖） |

### 验证

- `cargo test -p rbrain-engine --lib`：通过。
- `git diff --check`：通过。

---

## 2026-06-02 — Literature Review Pipeline 质量门禁与干净重建复测

### 修复问题

| # | 问题 | 修复 |
|---|------|------|
| P1 | `RetryParser` 先按 `Vec<Value>` 解析，但 extract prompt 返回单个 JSON object，导致每篇文献出现 2 次无效 JSON 重试 | 改为先解析 `serde_json::Value`，再将 object/array 统一规范化为 result items |
| P2 | synthesis 可能生成大量无引用、模板化、截断的内容（如 `国家逻辑.md`、`中国教育学学科体系.md` 曾出现几十节内容） | 新增 `validate_synthesis_quality`，保存前检查 traceable chunk 引用、二级标题数量、无引用正文节和过薄章节 |
| P3 | 删除 `research/synthesis/*.md` 后，DB 中旧 synthesis page 仍会被 compose 聚合，污染最终综述 | `AggregateContent` 聚合前过滤没有对应 Markdown 文件的 stale DB page，并打印跳过数量 |
| P4 | incremental 只看 DB 更新时间，不看输出文件是否存在；清空目录后可能误判 `synthesis up-to-date` | SaveAs 和 LinkedSources incremental 现在只有在 DB 记录与 Markdown 文件都存在时才允许跳过 |

### Synthesis 质量规则

- `synthesis` 保存前必须有可追溯 `[[raw/articles/... | chunk:N]]` 引用。
- 引用数量下限：按 source 数量要求 1-3 个 traceable chunk citation。
- 二级标题 `##` 总数上限为 9。最初实测 6 过严，会误拒绝 7-9 节的正常综合；9 能保留较完整结构，同时仍能拒绝 13、58、70 节这类章节爆炸。
- substantive section 超过 2 个无 chunk 引用会被拒绝。
- 过薄正文 section 超过 2 个会被拒绝。
- prompt 已同步要求不超过 9 个 `##` section，并禁止无材料支撑的维度、例子、启示和模板化长枚举。

### 干净重建测试（/Users/hongyu/project/rbrain-test）

按用户要求重置测试项目，只保留：

- `raw/articles/*.md`：55 篇测试文献
- `.rbrain/config.toml`：保留 API key 配置，未打印内容

重建步骤与结果：

```bash
cargo run -p rbrain-cli -- --brain-dir /Users/hongyu/project/rbrain-test/.rbrain sync
cargo run -p rbrain-cli -- --brain-dir /Users/hongyu/project/rbrain-test/.rbrain embed --all
cargo run -p rbrain-cli -- --brain-dir /Users/hongyu/project/rbrain-test/.rbrain stats
```

统计结果：

| 指标 | 数值 |
|------|------|
| raw/note pages | 55 |
| chunks | 696 |
| embedding coverage | 100.0% |
| research Markdown 初始状态 | 0 |

随后运行：

```bash
cargo run -p rbrain-cli -- --brain-dir /Users/hongyu/project/rbrain-test/.rbrain dream --profile literature_review
```

阶段观察：

- Extract：55 篇全部进入处理，产生 489 个结果。
- 未再出现 `[RetryParser] attempt 1/2 still invalid JSON`，JSON object 解析重试问题未复现。
- Synthesize：进入 72 个去重后 anchor。
- 质量门禁已实测拒绝章节爆炸结果，例如 13 节、58 节的 synthesis；同时允许合规 synthesis 保存。
- 本轮测试在 synthesis 中途按用户要求停止，以便换办公室前提交代码。

### 验证

- `cargo test -p rbrain-engine --lib`：通过。
- `git diff --check`：通过。

---

## 2026-05-31 — PipelineRunner 全功能落地 & 文献综述 E2E 验证

### 核心功能：可编程 Pipeline（TOML 配置）

**设计目标**：用 TOML profile 描述多阶段 LLM 处理流程，替代硬编码的 `dream_extract` / `dream_synthesize`，支持任意 EXTRACT → SYNTHESIZE → COMPOSE 组合。

#### 新增 InputSpec 变体：`AggregateContent`

```rust
AggregateContent {
    page_type: String,    // 聚合哪种类型的页面
    tag: Option<String>,
    max_pages: usize,     // 默认 30，无 SQL LIMIT 上限限制
    chars_per_page: usize // 默认 2000，每页截取字符数
}
```

将同类型所有页面合并为单次 LLM 调用，输出一个文档（COMPOSE 阶段）。

#### 新增 SaveTypeConfig.enrich_existing

`enrich_existing = true` 时，对已存在的页面追加新内容而非覆盖，并用 `mentions` 链接做幂等检查，防止重复追加。

#### 新增 inject_existing_titles

`inject_existing_titles = "concept"` 在每次 LLM 提取调用前，将已存在的 concept 名称注入 prompt，防止同义词增殖（如"自主""自主性""自主知识体系"被创建为三个独立概念）。

#### Jaccard 来源去重（dedup_sources_threshold）

synthesize 阶段：当两个 concept anchor 的 source 文章集合 Jaccard 相似度 ≥ 阈值时，跳过 source 集合较小的那个，避免生成近乎相同的综合页面。

### literature_review Profile（三阶段）

```toml
# Stage 1: extract — note → concept/figure/evidence
# Stage 2: synthesize — concept anchor + source notes → synthesis
# Stage 3: compose — 所有 synthesis 聚合 → 一篇 wiki 文献综述
```

**运行**：
```bash
rbrain dream --profile literature_review
```

**E2E 验证结果**（55 篇教育学文献，`min_sources=2`）：
- Extract：55 篇 note → 63 个 concept（incremental，已处理页面自动跳过）
- Synthesize：63 个 concept → 20 个有效 synthesis（Jaccard 去重后）
- Compose：20 个 synthesis → 1 篇 wiki 文献综述（约 3500 字，引用 35+ 篇原始文献）

输出文档质量评估：结构完整（8 个必需章节），所有引用格式正确（`[[raw/articles/slug | chunk:N]]`），学术争论识别有实质内容。

### Bug 修复（今日代码审查）

| # | 问题 | 修复 |
|---|------|------|
| C1 | `AggregateContent` 的 `list_pages` 调用含 `clamp(1,200)` 上限，`max_pages > 200` 时静默截断 | 去掉 SQL LIMIT，改为 Rust 侧 sort+truncate |
| C3 | `enrich_existing` 路径：`already_linked=true` 时未追加内容，仍上报 `"enriched"` | 引入 `did_enrich` 标志，跳过时报 `"skipped"` |
| C8 | `run_aggregate_step` 对非 `SaveAs` OutputMode 静默返回空 | 新增 `Return` 支持；其余 OutputMode 抛出明确错误 |

### 关键参数说明

| 参数 | 位置 | 说明 |
|------|------|------|
| `min_sources` | synthesize stage | concept 至少需要 N 篇 source 才触发综合，默认 3，建议 2 |
| `dedup_sources_threshold` | synthesize stage | Jaccard 去重阈值，0.8 时跳过 80% 重叠的概念 |
| `max_pages` | compose stage | 最多聚合 N 个 synthesis 页面，默认 30 |
| `chars_per_page` | compose stage | 每个 synthesis 页面截取的最大字符数，默认 2500 |
| `inject_existing_titles` | extract stage | 注入已知 concept 名称防止同义词，值为页面类型名 |
| `enrich_existing` | type_map.concepts | true = 追加新来源描述到已有 concept 页面 |
| `incremental` | 所有 stages | true = 跳过已处理页面（默认开启） |

### 测试数据状态（/Users/hongyu/project/rbrain-test）

| 指标 | 数值 |
|------|------|
| 源文章（note/raw） | 55 |
| 概念（concept） | 63 |
| 综合分析（synthesis） | 20 |
| Wiki 文献综述（wiki） | 1 |
| 引用来源覆盖 | 35+ 篇原始文献 |

---

## 2026-05-27 — 安全加固、正确性修复与设计决策

### 已提交功能（commit c5f45d7）

**安全加固**
- `validated_slug`：拒绝路径穿越（`../`、绝对路径、反斜杠），所有 slug 输入统一走此函数
- `INSERT ON CONFLICT DO UPDATE` 替换 `INSERT OR REPLACE`，保留原始 `created_at`

**正确性修复**
- `chunk_and_embed_page`：先获取 embedding 再删除旧 chunk，API 失败时保留可搜索内容
- links 表加 `is_generated` 列（migration 0011）；re-extract 只删除 `is_generated=1` 的行，保留用户手动创建的边
- 向量搜索：改为 `score / boost` + ASC 排序（距离语义：越小越相关）
- `is_derived_research_context`：将过滤范围从 `draft|synthesis|wiki` 扩展到 `concept|figure|evidence|memo`，think 只检索 note/raw 原始文献
- dream extract：无关联人物的事件路由到 `research/evidence/events/` 而非写入原始文章

**新增集成测试**（17 个全部通过）

### 设计决策

- **Timeline 定位**：展示层（人工浏览），不参与 AI 检索
- **Citation Graph 方向**：核心价值是 cites 引文关系（层次 2），而非 timeline 事件关系

---

## 2026-05-26 — 引用质量体系 & 关键 Bug 修复

### 新功能：`rbrain audit`

`rbrain audit <slug> [--fix]` — 检验引用规范性。

检查项：citation_type（引用了生成页而非原文）、bib_duplicate（重复条目）、bib_orphan（正文无引用的条目）、bib_missing（正文有引用但参考文献节无条目）。

### Bug 修复

- `to_canonical` timeline 分隔符缺失（根因：`compiled_truth` 与 `timeline` 之间无 `---`）
- think/generate 未过滤 synthesis/wiki 类型进入检索池
- timeline chunk 污染向量索引
- `is_compiled_truth` 因正文中 `---` 被错误翻转

---

## 2026-05-25 — Bug 修复 & Dream Cycle 完善

### Bug 修复（6 commits）

- 语言检测误判（Jpn/Kor → ZhHant/ZhHans）
- `clean_json()` 单标记 panic
- CJK 文本 1000-byte 截断 panic
- Frontmatter 空 `{}` 写入
- `add_take()` DB/文件写入顺序
- MCP `brain_outlinks` 字段名错误

### Dream Cycle 功能改进

- Extract：figure 描述质量提升，禁止"本文作者"等模糊引用
- Synthesize：改为概念聚类合成（backlinks → source notes），测试自动生成 17 个 synthesis 页面

---

## 待办

- [ ] Citation graph 层次 2：dream_extract 解析参考文献节，写入 `cites` 类型链接
- [ ] `policy_analysis.toml` profile 的 E2E 验证
- [ ] concept/figure 页面补充 language 字段（部分仍为 unknown）
- [ ] 多租户 HTTP routing（Phase 3）
- [ ] Sparse ANN（等待 LanceDB Rust SDK 上游支持）
