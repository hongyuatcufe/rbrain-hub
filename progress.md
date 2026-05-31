---
{}
---
# rbrain 开发进展

## 项目概述

rbrain-hub 是面向学术研究的 Rust 知识库系统，包含 CLI、MCP 服务端和可编程 LLM pipeline。

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
