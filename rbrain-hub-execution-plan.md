# rbrain-hub × ZeroClaw 执行计划（v2，优化版）

本计划假设 ZeroClaw 保持不变。rbrain-hub 演进为研究记忆、实验簿、文献库与方法审阅层，ZeroClaw 通过 MCP 调用它。

> 本文件由初版 codex 计划经系统审查后重写，已锁定关键设计决策（见末尾"已锁定的决策"）。
> 配套文档：`rbrain-hub/plan.md`（rbrain-hub 内部 code-review 评估，列举 retrieval/pipeline/citation 等基础设施 gap，并把每条 gap 映射到本文件的 milestone）。

## 目标

搭建一个轻量级学术研究 Agent 栈：

- **ZeroClaw** 执行复杂工作：Python/R/SQL、shell、浏览器、文件、报告导出、用户交互、任务编排。
- **rbrain-hub** 记录与审阅研究：文献、研究 run、数据集、artifact、findings、provenance、citations、方法质量。

rbrain-hub 不应成为第二个通用 Agent runtime。它应让 ZeroClaw 的研究工作变得**可追溯、可引用、可复用、可审阅**。

## 设计基线（不可破坏的边界）

1. 不让 rbrain 执行任意生成的 Python/R/SQL。
2. 不复制 ZeroClaw 的 shell/browser/file/审批/sandbox 系统。
3. 第一版不要求修改 ZeroClaw 源码。
4. rbrain 生成的文献综述草稿默认不是终稿。

每个 PR 在描述里都应明确"未破坏以上 4 条"。

---

## Phase 1: Research State Conventions

在大规模新增表之前，先定 page type 与 frontmatter 约定。rbrain 的 `page_type` 在 `crates/rbrain-core/src/page.rs` 是自由文本，新增 page type **无需 schema migration**。

### 新增 page types

- `research_run`
- `research_question`
- `dataset`
- `codebook`
- `analysis_plan`
- `artifact`
- `script`
- `result`
- `finding`
- `limitation`
- `research_memo`
- `method_note`

### 最小 frontmatter 字段

#### `research_run`

```yaml
type: research_run
title: "..."
run_id: "uuid"               # 与 research_runs 表 PK 对应（M1 引入）
status: planned | running | validating | complete | blocked
task_type: literature_review | data_analysis | mixed_methods | theory_building
created_by: zeroclaw
started_at: "YYYY-MM-DDTHH:MM:SSZ"
updated_at: "YYYY-MM-DDTHH:MM:SSZ"
```

#### `dataset`

```yaml
type: dataset
title: "..."
source: "..."
path: "..."
abs_path_snapshot: "..."     # 注册时的绝对路径快照
hash: "sha256:..."
size_bytes: null
format: csv | xlsx | json | parquet | sql | other
rows: null
columns: null
license: null
access_notes: null
```

#### `artifact`

```yaml
type: artifact
title: "..."
artifact_kind: script | log | result_table | chart | model_output | report | notebook
path: "..."
abs_path_snapshot: "..."
hash: "sha256:..."
size_bytes: null
mime_type: "..."
stored_copy: true | false    # 是否已复制到 .rbrain/artifacts/
created_by: zeroclaw
research_run: "research/runs/..."
```

#### `finding`

```yaml
type: finding
title: "..."
research_run: "research/runs/..."
status: draft | claim | validated   # 用于 validator 是 warn 还是 fail
confidence: low | medium | high
claims:                              # 可选，结构化数值/定性 claim
  - text: "..."
    kind: numeric | qualitative
    support: "<artifact_slug>#cell:B12" | "<chunk_ref>"
```

### Artifact 存储策略（基于 `artifact_kind`，不再"optional"）

| kind | 默认策略 | 说明 |
|---|---|---|
| `result_table` / `chart` / `model_output` / `finding_table` | 强制 copy 到 `.rbrain/artifacts/` | 结果数据小且高价值 |
| `script` / `notebook` | 强制 copy | 体积小，提供可重现性 |
| `dataset` | 软链 + hash；>100MB 不 copy | hash 不匹配 → validator `fail` |
| `log` | 软链 | 体积变化大 |

工具：`brain_artifact_resolve(slug)` 在 `path` 失效时回落到 store 快照。

---

## Phase 2: MCP Tools for ZeroClaw（合并版，5–6 个）

让 ZeroClaw 能记录工作，**不修改 ZeroClaw**。工具数从原计划 10 → 5–6，降低 prompt 体积与 eval 面。

### 工具清单

| 工具 | 作用 |
|---|---|
| `brain_create_research_run` | 创建 run（同时插入 `research_runs` 表 + 渲染 page）。可选携带 `research_question`、`analysis_plan`。返回 `run_id`、`run_slug`、推荐下一步。 |
| `brain_get_research_protocol` | 输入 `run_id`，返回**带状态**的 protocol（见下） |
| `brain_register_input` | `kind: dataset \| artifact`，统一注册输入或产物；自动 hash/存储策略；自动建 link |
| `brain_record` | `kind: finding \| limitation \| analysis_plan`，离散事实记录；带 oneOf payload schema |
| `brain_validate_research_run` | 运行 validators，返回结构化报告与受控 `suggested_actions` enum |
| `brain_citation_check` | 检查文档引用合法性（复用现有 `rbrain audit`） |
| `brain_evidence_check` | 沿 provenance 边检查 finding 的证据闭包 |

### Protocol 是状态机，不是静态 checklist

```jsonc
brain_get_research_protocol(run_id) →
{
  "task_type": "data_analysis",
  "current_step": "register_analysis_artifacts",
  "completed_steps": [...],
  "next_actions": [
    { "action": "register_artifact", "hint": "register result table from outputs/desc_stats.csv" }
  ],
  "blocking_validators": [
    { "validator": "dataset_registered", "status": "fail", "...": "..." }
  ]
}
```

状态由 validators 最近一次结果推导，**不在 page frontmatter 里手工维护**。ZeroClaw 会话中断后可凭 `run_id` 完全恢复。

### Validator 输出（受控反馈）

```jsonc
{
  "validator": "finding_has_supporting_artifact",
  "status": "pass" | "warn" | "fail",
  "message": "...",
  "affected_slugs": ["..."],
  "suggested_actions": [
    { "action": "link_evidence",
      "payload": { "from": "...", "to": "...", "link_type": "supports" } }
  ]
}
```

#### `suggested_actions.action` 受控词表

```
register_dataset | register_artifact | link_evidence
| record_limitation | rerun_analysis | add_citation
| split_finding | add_codebook | hash_mismatch_reupload
```

每条 action 有 payload schema，让 ZeroClaw 能做 "auto-fix where safe" 闭环。

---

## Phase 3: Provenance Graph

扩展 link 类型让 rbrain 能表示研究过程，而非只表示文献关系。

### 新增 link 类型

```
uses_dataset | uses_variable | uses_method
| computed_by | derived_from
| supports | contradicts | tests_hypothesis
| cites | limits | produces | validates
```

### 期望图模式

#### 数据分析

```
research_run --uses_dataset--> dataset
research_run --uses_method--> method_note
analysis_plan --tests_hypothesis--> research_question
result --computed_by--> script
result --derived_from--> dataset
finding --supports--> result
finding --cites--> literature_chunk_or_page
limitation --limits--> finding
research_memo --cites--> finding
```

#### 文献综述

```
research_run --uses_dataset--> literature_corpus
concept --evidence--> note/source chunk
synthesis --cites--> source chunks
review_draft --cites--> synthesis
gap_analysis --derived_from--> synthesis
research_memo --cites--> review_draft
```

---

## Phase 4: Validators

Validators 不执行分析代码；仅检查已注册状态与证据。它们应放在 `rbrain-engine` 的 `evidence/` 公共模块，让数据分析与文献综述两条 profile 都消费，避免双份维护。

### 数据分析

- `dataset_registered`
- `dataset_hash_present`
- `analysis_plan_exists`
- `codebook_exists_or_exempted`
- `artifact_hash_present`
- `script_registered_for_result`
- `finding_has_supporting_artifact`
- `finding_has_dataset_lineage`
- `limitations_recorded`
- `research_memo_links_findings`

> `numeric_claim_has_result_reference` 仅在 finding frontmatter 提供结构化 `claims:` 数组时启用；不在 M1 上线，避免 LLM 文本抽取的不确定性。

### 文献综述

- `source_count_minimum`
- `citation_chunks_exist`
- `citation_chunk_matches_slug`
- `primary_source_ratio`
- `synthesis_sections_have_citations`
- `gap_analysis_present`
- `contradictions_recorded`
- `review_links_to_synthesis_pages`

### Validator 触发规则

- 对 `finding.status = draft` 默认仅给 `warn`，避免 ZeroClaw "思考中" 阶段被报失败。
- `status >= claim` 才视为正式 claim，触发完整 fail 判定。

---

## Phase 5: 文献综述强化

保留 rbrain 的文献综述 pipeline，作为**证据驱动综合引擎**。

### 改进项

- synthesis/compose 后接 citation checker。
- 增加 gap-analysis 阶段。
- 增加 contradiction/tension 阶段。
- 来源多样性检查。
- 检索时 page-level max-pooling。
- `query --explain` 与 search 诊断。
- title/alias boost。
- 查询展开与 intent 分类的 query cache。

### 两种使用模式

#### rbrain-led

```
ZeroClaw 触发 rbrain literature_review profile
→ rbrain 抽 concept、建 synthesis、composes draft
→ ZeroClaw review、补证据、导出。
```

#### ZeroClaw-led

```
ZeroClaw 反复调用 brain_query / brain_get / brain_graph / brain_think
→ ZeroClaw 写终稿
→ rbrain 验证 citations，存档 research_memo。
```

---

## Phase 6: 检索质量（M0 的核心）

检索质量是地基，因为 ZeroClaw 完全依赖 rbrain 取证据。

### 必做

- sparse 路径降级为可观察事件：doctor 报告 + `--explain` 中标记 sparse=disabled。
- page-level max-pooling 在最终排序前。
- `query --explain` 输出 dense rank/score、BM25 rank/score、RRF 贡献、boost 来源。
- search 诊断：`search diagnose "<q>" --target <slug>` 指明为何 miss。
- query cache 命中/未命中报告。
- 来源多样性约束。
- title/alias boost。

### CLI/MCP 接口

```bash
rbrain query "..." --explain
rbrain search diagnose "..." --target <slug>
rbrain eval retrieval --qrels fixtures.tsv
```

---

## Phase 7: Artifact 模型

Page 约定先行，artifact 表在 MCP 稳定后再加。M1 实际上已经把核心字段进了 `research_runs` 表 + page frontmatter，因此 artifact 表的真正职责是把高基数的 hash/path/metadata 从 frontmatter 中提出来。

### 拟新增表（M5）

```sql
CREATE TABLE artifacts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    slug TEXT NOT NULL REFERENCES pages(slug) ON DELETE CASCADE,
    path TEXT NOT NULL,
    abs_path_snapshot TEXT NOT NULL,
    kind TEXT NOT NULL,
    mime_type TEXT,
    hash TEXT NOT NULL,
    size_bytes INTEGER,
    stored_copy INTEGER NOT NULL DEFAULT 0,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;
```

---

## Phase 8: 研究 Protocol Profiles

rbrain profiles 是 **protocol**，不是 execution runner。

### `data_analysis`

```
create_research_run
  -> register_dataset
  -> record_analysis_plan
  -> handoff_to_zeroclaw_for_execution
  -> register_analysis_artifacts
  -> validate_result_provenance
  -> validate_findings
  -> compose_research_memo
```

执行密集型工作（CSV profiling、回归、可视化、模型）由 ZeroClaw 负责。

### `literature_review`

```
import_sources
  -> sync_embed
  -> extract_concepts
  -> synthesize_by_concept
  -> compose_review_draft
  -> validate_citations
  -> detect_gaps
  -> save_research_memo
```

---

## 实施顺序（修订版）

| Milestone | 范围 | 关键依赖 | 验收 |
|---|---|---|---|
| **M0**（与 M1 并行） | sparse 降级 doctor 告警；`query --explain` v1；`brain_citation_check` 最小实现（复用 `rbrain audit`） | 无 | doctor 显示 sparse 状态；`--explain` 输出分量；citation_check 能识别非法 chunk 引用 |
| **M1** | page 约定 + `research_runs` 表（migration 0013） + 5–6 个 MCP 工具 + 3 个 validator（`dataset_registered` / `artifact_hash_present` / `finding_has_supporting_artifact`） + protocol state machine | M0 的 citation_check | `fixtures/data_analysis_demo/` 跑通：ZeroClaw 创建 run、注册 dataset/artifact、记录 finding、`validate_research_run` 全绿 |
| **M2** ✅ | 全部 provenance 边类型（`ResearchEdge` 枚举 + 白名单）；真正的 `brain_evidence_check` 图遍历（finding → supports/cites → artifact|note → derived_from/computed_by → dataset/script）；新增 `brain_provenance_of(slug)` 一跳枚举研究边邻接关系；`SuggestedAction` 含 `record_analysis_plan` | M1 | `m2_provenance_fixture` 8 个测试全绿；rbrain 可回答 "what evidence supports this finding?" 与 "which dataset/script produced this result?" |
| **M3** | 文献综述质量升级：citation checker 强化、gap analysis、contradiction、source diversity、page-level max-pooling | M0 / M2 | `fixtures/literature_review_demo/` 全绿；55 篇语料能生成 draft 并通过 citation_check |
| **M4** | 检索可解释/可诊断：`search diagnose`、title/alias boost、query cache 命中报告 | M0 | 任一 miss case 仅靠 `--explain` 就能定位原因 |
| **M5** | `artifacts` 表 + `stage_runs` + 指纹增量 | M1/M2 稳定 | run 可被审计回放 |

### Agent 层端到端度量

每个 milestone 完成后跑：

- `fixtures/data_analysis_demo/`：小 CSV + 研究问题。
- `fixtures/literature_review_demo/`：复用 `rbrain-test` 现有 55 篇文献。

记录 wall-clock、人工介入次数。M1 → M5 期望"人工介入次数"单调下降。

---

## 关键文件落点

- `rbrain-hub/migrations/0013_research_runs.sql` — runs 表（M1）。
- `rbrain-hub/crates/rbrain-engine/src/evidence/` — citation_check / evidence_check / 公共 validator 面（M0/M1）。
- `rbrain-hub/crates/rbrain-engine/src/research/` — research_run state machine、protocol 状态推导（M1）。
- `rbrain-hub/crates/rbrain-mcp/src/lib.rs` — 注册 5–6 个新工具（M1）。
- `rbrain-hub/crates/rbrain-mcp/src/http.rs` — HTTP 镜像同步（M1）。
- `rbrain-hub/crates/rbrain-cli/src/main.rs` — 新增 `rbrain research run / validate / protocol` 命令；扩展 `query --explain`、`doctor`（M0/M1）。
- 复用：`rbrain audit`（现有命令）作为 `brain_citation_check` 内核。
- **不要新建**：`research_run` 的"执行器"。它是 protocol state machine，不跑任何代码。

## 验证方式

1. `cargo test -p rbrain-engine --lib`（evidence + research 模块单测）。
2. `cargo test -p rbrain-mcp --lib`（新工具 schema + dispatch）。
3. 两个 fixture 脚本跑通，期望 `brain_validate_research_run` 全绿。
4. 从 ZeroClaw（loopback MCP）跑一次完整研究任务，记录人工介入计数。
5. `rbrain query --explain "<known target>"` 输出 dense/BM25 分量与 sparse 降级标记。

---

## Non-goals（硬边界）

- 不让 rbrain 执行任意生成的 Python/R/SQL。
- 不复制 ZeroClaw 的 shell/browser/file/审批/sandbox 系统。
- 第一版不要求修改 ZeroClaw 源码。
- 默认不把 rbrain 生成的文献综述草稿视为终稿。

---

## 已锁定的决策

1. **M1 引入 `research_runs` 表**（migration 0013）。事实源在表，page 是渲染层。
2. **MCP 工具按 kind 合并为 5–6 个**：`brain_create_research_run`、`brain_register_input(kind=dataset|artifact)`、`brain_record(kind=finding|limitation|analysis_plan)`、`brain_validate_research_run`、`brain_citation_check`、`brain_evidence_check`。
3. **M0 与 M1 并行**：sparse 降级 doctor 告警 + `query --explain` v1 + 基于 `rbrain audit` 的 `brain_citation_check`，作为 M3 文献综述质量的可观察地基。
4. **Protocol 是状态机**，绑定到 `run_id`，状态由 validators 推导，跨会话可恢复。
5. **Artifact 存储策略按 `artifact_kind` 决定**，不再 "optional"。
6. **Validator 对 `finding.status = draft` 默认仅 warn**，避免 ZeroClaw 思考阶段噪声。
7. **`suggested_actions` 使用受控 action enum + payload schema**，支持 ZeroClaw 可编程响应。
8. **Evidence / citation / source-diversity / gap-analysis 原语放在 `rbrain-engine::evidence/` 公共模块**，文献综述与数据分析共享。

---

## 第一交付目标

```
brain_create_research_run
brain_register_input
brain_record
brain_validate_research_run
brain_citation_check
```

加上 `research_runs` 表与 protocol state machine，ZeroClaw 就能立刻把 rbrain 当作研究实验簿与方法审阅器使用。
