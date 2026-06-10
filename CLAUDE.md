# CLAUDE.md — ZeroClaw × rbrain-hub 项目执行规范

本仓库是 ZeroClaw（执行器）与 rbrain-hub（研究记忆/审阅层）协作的轻量学术研究 Agent 栈。任何在本目录工作的 agent（含 Claude Code、codex、子 agent）必须先读本文件。

> 配套文档：
> - `rbrain-hub-execution-plan.md` — 顶层执行计划 v2（M0–M5）
> - `plan.md` — rbrain-hub 内部 gap 清单 + milestone 映射
> - 子项目自有 `AGENTS.md` / `CLAUDE.md`：`zeroclaw/`、`gbrain/`、`rbrain-hub/crates/*` — 子项目内部规范以子目录文档为准

---

## 1. 职责切线（硬边界，禁止跨越）

- **ZeroClaw = 执行器**：shell / Python / R / SQL / browser / files / 审批 / sandbox / 报告导出 / 多轮用户交互。
- **rbrain-hub = 研究记忆 + 方法审阅器**：研究状态、证据检索、provenance、citation/method 校验、文献综合。

### rbrain 永远不做

1. 不执行任意生成的 Python/R/SQL。
2. 不复制 ZeroClaw 的 shell / browser / file / 审批 / sandbox。
3. 第一版不要求修改 ZeroClaw 源码。
4. rbrain 生成的文献综述草稿默认不是终稿。

> **任何 PR 描述里必须有一句**："未破坏 4 条 non-goals"。破坏其中任意一条需在 PR 顶部用 `Breaks non-goal #N:` 显式声明并说明理由。

---

## 2. 当前 milestone

**已完成**：M0 + M1 + M2 + M3。**下一步**：M4 检索可解释/可诊断（`search diagnose`、title/alias boost、query cache 命中报告）。详见 `rbrain-hub-execution-plan.md` 的"实施顺序"章节。

提交代码前请确认你的改动归属于哪个 milestone；偏离 milestone 的工作请先开 issue 或更新计划。

---

## 3. MCP 工具命名与设计约定

M1 工具表已锁定为 5–6 个合并工具，**禁止**新增独立的 `brain_register_dataset` / `brain_register_artifact` / `brain_record_finding` / `brain_record_limitation` 等：

| 工具 | kind 取值 |
|---|---|
| `brain_create_research_run` | — |
| `brain_get_research_protocol` | — |
| `brain_register_input` | `dataset` / `artifact` |
| `brain_record` | `finding` / `limitation` / `analysis_plan` |
| `brain_validate_research_run` | — |
| `brain_citation_check` | — |
| `brain_evidence_check` | — |
| `brain_provenance_of` | — (M2) |
| `brain_verify_citations` | — (M3) |

新工具采用 `kind` 判别 union + oneOf payload schema。新增 kind 优先于新增工具。

`brain_evidence_check` 返回 `EvidenceChain { direct_support, datasets, scripts, literature_sources }`，支持 data_analysis (supports → artifact → derived_from → dataset) 与 literature_review (cites/supports → note|raw) 两种 finding 形态。

`brain_provenance_of(slug)` 一跳枚举研究图邻接边（受白名单约束的 12 个 research edge：derived_from / computed_by / uses_dataset / uses_method / uses_variable / supports / contradicts / produces / cites / tests_hypothesis / validates / limits），返回 `{ page_type, edges: [{ edge_type, neighbour_slug, neighbour_page_type, incoming }] }`。非研究边（references/mentions/related 等）被过滤；完整 finding 证据链由 `brain_evidence_check` 返回。

`brain_verify_citations(slug, content?, check_content?, hints?)` 给定文档 slug（或直接传 `content` 字符串）+ 可选的 `CitationHint[]`（author/year/title_fragment），抽取每条引用、按 `pub_metadata` 或语义检索解析出原文 slug、并核对作者/年份/期刊。`check_content=true` 还会让 LLM 判定原文是否实际支持该 claim。返回 `DocCitationReport { citations[], summary { total, resolved, bib_error, bib_warn, ... } }`。

### ⚠ MCP `TenantArgs` 是 M4 → M10 的过渡设计

M4 PR-1c（2026-06-11）在 MCP 核心工具（query / get / put / delete / list / graph / backlinks / outlinks / think / generate / link / timeline / tag …）的参数 schema 里通过 `#[serde(flatten)] tenant: TenantArgs { user_id, project_id }` 暴露了 tenant 字段。**这与本文档 §4.4 "不在参数里暴露——避免客户端伪造身份"原则冲突**，但在 M4 阶段没有 auth gateway 时是唯一可行的过渡方案。

**M10 落地 lightweight agent runtime + auth 时必须做的事**（写在 `rbrain-hub/plan.md` §10.4）：

1. 从 auth token 解出 `(user_id, project_id)` 注入到 MCP server 上下文
2. 从所有 MCP arg schemas 中移除 `TenantArgs`
3. 若 `TenantArgs` 仍出现在请求里，server 应**忽略**（不能被客户端覆盖 auth 上下文）

这是 M10 启动时**第一件要做的事**，否则任意客户端可冒充任意身份。

---

## 4. 数据模型约定

- `research_runs` 表是 run 状态的**事实源**（M1 migration `0013`）。Markdown page 只是渲染层。
- 新增 page type 不需 schema migration（`page_type` 在 `rbrain-core` 是自由文本），只需在文档与 frontmatter 模板里登记。
- `finding.status` 必须是 `draft | claim | validated`；validator 对 `draft` 仅 `warn`。
- artifact 存储策略按 `artifact_kind` 决定（见执行计划 Phase 1 表格），不再"optional"。

---

## 5. Validator 与 `suggested_actions`

- Validator 不执行分析代码，只检查已注册状态与证据。
- 输出形态固定：`{ validator, status: pass|warn|fail, message, affected_slugs, suggested_actions[] }`。
- `suggested_actions[].action` 必须来自受控 enum：
  ```
  register_dataset | register_artifact | record_analysis_plan
  | link_evidence | record_limitation | rerun_analysis
  | add_citation | split_finding | add_codebook
  | hash_mismatch_reupload
  ```
- 每条 action 附 payload schema。新增 action 必须同时更新该 enum + ZeroClaw 侧的响应映射。

---

## 6. 公共模块归属

- citation_check / evidence_check / source_diversity / gap_analysis 等原语放在 `rbrain-hub/crates/rbrain-engine/src/evidence/`。
- research_run state machine、protocol 状态推导放在 `rbrain-hub/crates/rbrain-engine/src/research/`。
- **不要**为 literature_review 与 data_analysis 各写一份 citation 校验逻辑。

---

## 7. 复用优先

新建代码前先搜：

- `rbrain audit`（`rbrain-hub/crates/rbrain-cli/src/main.rs`）是 `brain_citation_check` 的内核 — 不要重写。
- `brain_query` / `brain_get` / `brain_graph` / `brain_think` 已有，新检索需求先看能否复合。
- pipeline 的 `AggregateContent` / `LinkedSources` / `SaveAs` 已能覆盖大部分综合场景。

---

## 8. 测试与验证

每次实现完成必须能通过：

1. `cargo test -p rbrain-engine --lib`
2. `cargo test -p rbrain-mcp --lib`
3. fixtures：
   - `fixtures/data_analysis_demo/` — ZeroClaw 跑完，`brain_validate_research_run` 全绿
   - `fixtures/literature_review_demo/` — 复用 `rbrain-test` 55 篇语料，`brain_citation_check` 全绿
4. 端到端运行需在 PR 描述里记录 wall-clock 与人工介入次数（M1 后期望单调下降）。

---

## 9. Agent 行为约定

- **状态恢复**：ZeroClaw 中断后凭 `run_id` 调 `brain_get_research_protocol` 恢复，不要靠 frontmatter 自维护。
- **回注册**：ZeroClaw 每产出一个 dataset / script / result / chart / log 立即调 `brain_register_input`，不要堆积到任务末尾。
- **反馈闭环**：validator `suggested_actions` 是结构化指令，ZeroClaw 应优先尝试 `auto-fix where safe`，不能 fix 的再上抛给用户。
- **绝不**：让 rbrain 跑 ZeroClaw 该跑的代码，或让 ZeroClaw 自己重新发明 citation/evidence 校验。

---

## 10. 文档同步

修改以下任一处时**必须**同步另一处：

- `rbrain-hub-execution-plan.md` 的 milestone / 工具表 / non-goals
- `plan.md` 的 Locked decisions / Gap mapping
- 本 CLAUDE.md 第 1、3、5、6 节

不一致是 hard fail，PR 不予合并。

---

## 11. 子目录入口

- ZeroClaw 内部规范：`../zeroclaw/AGENTS.md`、`../zeroclaw/CLAUDE.md`
- rbrain-hub 进展与设计：`progress.md`、`DESIGN.md`、`guide.md`
- gbrain（参考实现）：`../gbrain/CLAUDE.md`、`../gbrain/AGENTS.md`、`../gbrain/DESIGN.md`
