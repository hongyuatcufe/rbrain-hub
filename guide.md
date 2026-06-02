# rbrain-hub 使用指南

本指南覆盖从安装到高级 Pipeline 编写的完整使用流程。

---

## 目录

1. [安装与配置](#安装与配置)
2. [基础操作：页面读写与检索](#基础操作)
3. [知识图谱](#知识图谱)
4. [自动化流水线：Dream Cycle](#dream-cycle)
5. [Pipeline Profile：可编程多阶段流水线](#pipeline-profile)
6. [内置 Profile 详解：文献综述](#文献综述-profile)
7. [深度推理与综合：Think & Generate](#think--generate)
8. [引用工作流](#引用工作流)
9. [MCP 服务端集成](#mcp-服务端集成)
10. [时间线与诠释片段](#时间线与诠释片段)
11. [维护与诊断](#维护与诊断)
12. [自定义 Pipeline Profile](#自定义-pipeline-profile)
13. [常见问题](#常见问题)

---

## 安装与配置

### 从源码构建

```bash
git clone https://github.com/hongyuatcufe/rbrain-hub
cd rbrain-hub
cargo build --release -p rbrain-cli
ln -sf "$PWD/target/release/rbrain" ~/.local/bin/rbrain
```

要求 Rust 1.85+。

### 初始化知识库

```bash
cd my-research-project
rbrain init
```

这会在当前目录创建 `.rbrain/` 目录结构：

```
.rbrain/
  config.toml      ← API 密钥配置（已加入 .gitignore）
  brain.db         ← SQLite 数据库
  lance/           ← LanceDB 向量存储
  tantivy/         ← BM25 全文索引
```

### 配置 API 密钥

编辑 `.rbrain/config.toml`：

```toml
[qwen]
api_key = "sk-..."
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "text-embedding-v4"

[deepseek]
api_key = "sk-..."
base_url = "https://api.deepseek.com/v1"
model = "deepseek-v4-flash"
model_pro = "deepseek-v4-pro"

embedding_dim = 1024
```

- **Qwen**：用于生成密集向量和稀疏向量（嵌入），无需 LLM API
- **DeepSeek**：用于所有 LLM 任务（提取、综合、推理）

如果只需要检索不需要 LLM，可以只配置 Qwen。

### 知识库自动发现

rbrain 从当前目录向上查找 `.rbrain/`，找不到则回退到 `~/.rbrain/`。无需设置环境变量。

手动指定：
```bash
rbrain --brain-dir /path/to/brain stats
# 或
RBRAIN_HOME=/path/to/brain rbrain stats
```

---

## 基础操作

### 导入文献

```bash
# 批量导入目录下所有 .md 文件
rbrain import ./papers/

# 导入后自动嵌入（需要 Qwen API）
# import 命令默认会嵌入，加 --no-embed 跳过
rbrain import ./papers/ --no-embed

# 单篇导入
rbrain put raw/articles/my-paper --file my-paper.md
```

导入的文章通常放在 `raw/articles/` 路径下，类型设为 `note`（默认）。

### 读写页面

```bash
# 读取页面（含时间线）
rbrain get raw/articles/my-paper

# 从文件写入
rbrain put concepts/自主知识体系 --file concept.md --type concept

# 内联写入
rbrain put questions/研究问题1 --content "# 问题\n\n自主知识体系与西方范式的关系？" --type question

# 删除
rbrain delete concepts/废弃概念
```

**页面类型**：`note` | `concept` | `figure` | `synthesis` | `wiki` | `question` | `evidence` | `draft` | `memo` | `period` | `book`

### 列出页面

```bash
rbrain list                          # 全部页面
rbrain list --type concept           # 按类型过滤
rbrain list --tag 教育学             # 按标签过滤
rbrain list --type synthesis --json  # JSON 输出
rbrain list --limit 20               # 限制数量
```

### 检索

```bash
# BM25 关键词检索（无需 API，速度快）
rbrain search "自主知识体系"
rbrain search "教育学" --type concept --limit 10

# 三路混合检索（密集向量 + 稀疏向量 + BM25，质量最高）
rbrain query "中国教育学的本土化路径"
rbrain query "教育现代化" --expand --limit 15

# --expand：用 LLM 扩展查询词，提升召回率
```

检索结果包含 chunk ID，格式为 `[chunk:N]`，用于后续创建精确链接。

### 嵌入管理

```bash
rbrain embed --stale     # 只重新嵌入过期页面（推荐）
rbrain embed --all       # 重新嵌入全部
rbrain embed my/page     # 嵌入指定页面
```

导入后如果跳过了嵌入，随时可以补：
```bash
rbrain embed --stale
```

---

## 知识图谱

### 创建链接

链接可以锚定到具体段落（chunk），适合学术引用：

```bash
# Step 1：查找相关段落，获取 chunk ID
rbrain search "自主知识体系" --limit 5
# 输出：[1] raw/articles/郝文武论教育学 — [chunk:381] "主体对自己的思想和行动做主..."

# Step 2：创建锚定到段落的链接
rbrain link concepts/自主知识体系 raw/articles/郝文武论教育学 \
  --type evidence --from-chunk 381

# Step 3：验证
rbrain backlinks raw/articles/郝文武论教育学
rbrain links concepts/自主知识体系
```

**链接类型**：
| 类型 | 含义 |
|------|------|
| `evidence` | 为某概念提供实证支持 |
| `related` | 概念间相关联 |
| `supports` | 支持某论点 |
| `contrasts` | 与某观点对立 |
| `develops` | 在某基础上发展 |

### 图谱查询

```bash
rbrain graph-query concepts/自主知识体系 --depth 2 --direction both
rbrain backlinks raw/articles/郝文武论教育学   # 谁引用了这篇文章
rbrain orphans                                  # 无入向链接的孤立页面
```

### 自动提取 Wikilinks

页面内容中的 `[[slug]]` 和 `[[slug | chunk:N]]` 格式会被自动提取为图链接：

```bash
rbrain extract --all       # 对全部页面重新提取
rbrain extract my/page     # 对指定页面提取
```

---

## Dream Cycle

Dream Cycle 是四阶段自动化流水线，对源文献执行完整的知识提取：

```
lint → embed → extract → synthesize
```

### 完整运行

```bash
rbrain dream
```

### 分阶段运行

```bash
rbrain dream --stage lint         # 质量检查
rbrain dream --stage embed        # 嵌入过期页面
rbrain dream --stage extract      # 提取概念、人物、事件
rbrain dream --stage synthesize   # 生成概念综合页面
```

### Phase 3：Extract

对每篇未处理的 `note` 页面，调用 DeepSeek 提取：

- **concepts** → `research/concepts/<slug>`（类型：`concept`）
- **figures**（学者/人物）→ `research/figures/<slug>`（类型：`figure`）
- **timeline events** → 关联到人物页面，或 `research/evidence/events/<source-slug>`
- 通过 `dream_metadata` 表保证幂等，不会重复处理

重置并重新提取：
```bash
sqlite3 .rbrain/brain.db "DELETE FROM dream_metadata;"
rbrain dream --stage extract
```

### Phase 4：Synthesize

对有 **3+** 篇源文章反向链接的概念，生成结构化综合页面：

- 保存至 `research/synthesis/<concept-slug>`
- 自动建立链接：`synthesis → concept`（develops）、`synthesis → 源文章`（evidence）
- 源文章更新后自动重新综合

### Merge Concepts（嵌入去重）

基于 embedding 余弦相似度合并近义词概念：

```bash
rbrain dream --stage merge-concepts
```

默认阈值 0.85。保留入度最高的概念（被最多文章引用），将其他同义词的入向链接重定向过来。

---

## Pipeline Profile

Pipeline Profile 是 TOML 格式的多阶段流水线配置，比 Dream Cycle 更灵活，支持 EXTRACT → SYNTHESIZE → COMPOSE 三种阶段类型。

### 运行内置 Profile

```bash
rbrain dream --profile literature_review   # 文献综述流水线
rbrain dream --profile policy_analysis     # 政策分析流水线
```

Profile 是增量的：已处理页面自动跳过，可以安全重复运行。

### 三种输入模式

| `input_mode` | 含义 | 典型用途 |
|---|---|---|
| `self` | 每页单独处理 | EXTRACT：逐篇提取 |
| `linked_sources` | 以 anchor 页为中心，聚合其 source 页 | SYNTHESIZE：以 concept 为锚点 |
| `aggregate` | 将所有指定类型页面合并为一次 LLM 调用 | COMPOSE：综合多个 synthesis → 一篇综述 |

### 四种输出模式

| `output_mode` | 含义 |
|---|---|
| `return` | 返回给调用者，不写入知识库 |
| `save` | 每个输入对应一个输出页面（1:1） |
| `save_multi` | 一个输入对应多种类型的输出页面（1:N） |
| `update_frontmatter` | 将 LLM 结果合并到输入页面的 frontmatter |

### 关键参数速查

| 参数 | Stage | 含义 | 默认值 |
|------|-------|------|--------|
| `min_sources` | synthesize | concept 至少需要 N 篇 source | 3 |
| `dedup_sources_threshold` | synthesize | Jaccard 去重阈值（0=禁用） | 0.0 |
| `inject_existing_titles` | extract | 注入已知概念名防同义词 | null |
| `enrich_existing` | type_map | 追加而非覆盖已有页面 | false |
| `max_pages` | compose | 最多聚合 N 个页面 | 30 |
| `chars_per_page` | compose | 每页截取字符数 | 2000 |
| `incremental` | 所有 | 跳过已处理页面 | true |
| `batch_size` | 所有 | 每批处理页数 | 1 |

---

## 文献综述 Profile

`literature_review` 是三阶段内置 Profile，自动完成从原始文献到完整学术文献综述的全过程。

### 阶段说明

**Stage 1：extract**

```
input:  note 页面（每篇文献）
output: concept / figure / evidence 页面
```

- `inject_existing_titles = "concept"`：将已有 concept 名称注入 prompt，防止同义词增殖
- `enrich_existing = true`：已有 concept 追加新文献的描述，而非覆盖

**Stage 2：synthesize**

```
input:  concept anchor + 其反向链接的 note source 页面
output: synthesis 页面（每个有效 concept 一篇）
```

- `min_sources = 2`：concept 至少被 2 篇文献提及才生成综合
- `dedup_sources_threshold = 0.8`：source 集合重叠 ≥80% 时跳过 source 少的那个

**Stage 3：compose**

```
input:  所有 synthesis 页面（合并为单次 LLM 调用）
output: 一篇完整的 wiki 文献综述
```

输出保存至 `research/wiki/compose.md`，包含：`# 标题`、`## 研究背景与意义`、`## 核心概念辨析`、`## 主要研究议题`、`## 学术争论与分歧`、`## 工作判断`、`## 待研究问题`、`## 参考来源`。

### 标准工作流

```bash
# 1. 导入文献（第一次）
rbrain import ./papers/

# 2. 运行完整流水线
rbrain dream --profile literature_review > run.log 2>&1

# 3. 查看输出
rbrain get research/wiki/compose

# 4. 重新运行（增量，只处理新增文献）
rbrain dream --profile literature_review
```

### 调优建议

**覆盖面不够**：降低 `min_sources`
```toml
min_sources = 2   # 默认 3，降到 2 可显著增加 synthesis 数量
```

**synthesis 内容重复**：启用或降低 `dedup_sources_threshold`
```toml
dedup_sources_threshold = 0.8
```

**概念名称混乱**：确保 `inject_existing_titles = "concept"` 已开启

**compose 内容不够丰富**：增加 `max_pages` 或 `chars_per_page`
```toml
max_pages = 40
chars_per_page = 3000
```

### 重新生成

如果需要重新运行 synthesize 和 compose（例如调整了参数）：
```bash
rm research/synthesis/*.md research/wiki/*.md
rbrain dream --profile literature_review
```

只需删除输出文件，增量逻辑会重新触发。

---

## Think & Generate

### Think：深度推理

对检索到的上下文进行结构化推理，输出张力、工作判断、开放问题：

```bash
rbrain think "中国教育学自主知识体系建构的核心矛盾" --limit 12 --expand
rbrain think "topic" --limit 12 --expand --save           # 保存到 synthesis/<slug>
rbrain think "topic" --limit 12 --expand --save --draft   # 保存到 drafts/<slug>
```

输出格式（固定结构）：
- `## 核心观点`
- `## 张力与矛盾`
- `## 工作判断`
- `## 开放问题`

Citations 使用 `[[slug | chunk:N]]` 格式，保存时自动建立图链接。

### Generate：Wiki 综述

```bash
rbrain generate "topic" --limit 12 --expand
rbrain generate "topic" --save   # 保存到 wiki/<slug>
```

适合生成约 500 字的参考 wiki 摘要。完整文献综述推荐使用 `dream --profile literature_review`。

### 区别

| `generate` | `think` | `dream --profile` |
|---|---|---|
| 约 500 字 wiki 摘要 | 结构化推理，深度优先 | 完整文献综述，全自动 |
| 单次 LLM 调用 | 单次 LLM 调用 | 多阶段，逐篇提取+聚合 |
| 适合快速参考 | 适合分析特定问题 | 适合系统性综述 |

---

## 引用工作流

### rbrain cite

从任意页面出发，遍历引用图谱，收集所有可达的原始文献并生成参考文献列表：

```bash
# 打印参考文献（纯文本）
rbrain cite research/drafts/my-review --depth 3

# BibTeX 格式
rbrain cite research/drafts/my-review --depth 3 --format bibtex

# 追加到页面并保存（幂等，会替换已有 ## 参考文献 节）
rbrain cite research/drafts/my-review --depth 3 --append
```

**depth 说明**：
- depth 2：draft → concepts/synthesis → raw 原始文献
- depth 3：draft → synthesis → concepts → raw 原始文献（推荐，抓取间接引用）

### rbrain audit

检验引用规范性：

```bash
rbrain audit research/drafts/my-review
rbrain audit research/drafts/my-review --fix   # 自动修复重复和游离条目
```

检查项：
- **ERROR** `citation_type`：引用了 draft/synthesis/wiki 生成页而非原始文献
- **WARN** `bib_duplicate`：参考文献列表中存在重复条目
- **WARN** `bib_orphan`：参考文献条目在正文中没有对应 `[[slug]]` 引用
- **INFO** `bib_missing`：正文引用了原始文献但未出现在参考文献节

### 标准收尾流程

```bash
# 1. 组装并保存综述
rbrain put research/drafts/my-review --file assembled.md

# 2. 重新索引 wikilinks（确保 [[...]] 引用被记录为图链接）
rbrain extract --all

# 3. 追加参考文献
rbrain cite research/drafts/my-review --depth 3 --append

# 4. 检验质量
rbrain audit research/drafts/my-review
```

---

## MCP 服务端集成

### 启动

```bash
rbrain serve mcp                          # stdio 模式（Claude Code 使用）
rbrain serve mcp --http 127.0.0.1:3456   # 本地 HTTP 模式
```

### Claude Code 配置

在 `~/.claude/claude_desktop_config.json` 或项目 `.mcp.json` 中：

```json
{
  "mcpServers": {
    "rbrain": {
      "type": "stdio",
      "command": "rbrain",
      "args": ["serve", "mcp"]
    }
  }
}
```

### 工具速查

| 工具 | 参数 | 说明 |
|------|------|------|
| `brain_query` | `query`, `limit`, `expand`, `page_type`, `tag` | 三路混合检索 |
| `brain_get` | `slug` | 读取页面 |
| `brain_put` | `slug`, `content`, `page_type` | 写入/更新页面 |
| `brain_delete` | `slug` | 删除页面 |
| `brain_list` | `page_type`, `tag`, `language`, `limit` | 列出页面 |
| `brain_think` | `topic`, `limit`, `expand` | 深度推理 |
| `brain_generate` | `topic`, `limit`, `expand` | Wiki 综述 |
| `brain_link` | `from`, `to`, `link_type`, `chunk_id` | 创建图链接 |
| `brain_unlink` | `from`, `to`, `link_type` | 删除链接 |
| `brain_backlinks` | `slug` | 反向链接 |
| `brain_outlinks` | `slug` | 出向链接 |
| `brain_graph` | `slug`, `edge_type`, `depth`, `direction` | 图谱遍历 |
| `brain_orphans` | — | 孤立页面 |
| `brain_add_timeline_entry` | `slug`, `text`, `date`, `source` | 添加时间线条目 |
| `brain_add_tag` | `slug`, `tag` | 添加标签 |
| `brain_remove_tag` | `slug`, `tag` | 删除标签 |
| `brain_stats` | — | 知识库统计 |
| `brain_process` | `slugs`, `task`, `response_format` | 批量 LLM 处理 |

### MCP vs CLI 选择原则

| 用 CLI | 用 MCP |
|--------|--------|
| 批量导入/导出 | 对话中快速检索 |
| dream cycle / profile 运行 | 探索性检索 |
| 嵌入刷新 | 读取页面内容 |
| 健康检查和统计 | 添加时间线条目 |
| Git 快照 | 实时概念记录 |

---

## 时间线与诠释片段

### Timeline

给任意页面附加带日期的条目（不修改页面正文）：

```bash
rbrain timeline figures/冯建军 \
  --date "2024-03-15" \
  --text "发表《中国教育学自主知识体系及其自觉建构》" \
  --source "raw/articles/冯建军2024 chunk:1"

# 不加 --date 默认今天
rbrain timeline figures/冯建军 --text "入选教育部长江学者"
```

`rbrain get figures/冯建军` 会在 `---` 分隔线后显示时间线。

### Takes

不改写页面内容，附加简短诠释：

```bash
rbrain take concepts/自主知识体系 \
  "自主不等于排外，而是建立平等对话的主体地位" \
  --kind judgment

rbrain take concepts/自主知识体系 \
  "这一框架在基础教育领域是否同样适用？" \
  --kind question

rbrain takes concepts/自主知识体系   # 列出全部诠释
```

`--kind`：`judgment` | `question` | `hypothesis` | `interpretation`（默认）

---

## 维护与诊断

### 健康检查

```bash
rbrain stats       # 页面数、chunk 数、链接数、嵌入覆盖率
rbrain doctor      # 检查孤立页面、未嵌入页面、损坏链接
rbrain doctor --fix  # 自动修复可以修复的问题
rbrain lint        # 质量报告：缺少标题、链接问题
```

### 同步文件系统

```bash
rbrain sync              # 同步文件系统变更 → 数据库
rbrain sync --embed      # 同步后重新嵌入变更页面
```

如果直接编辑了 `.md` 文件而没有通过 `rbrain put`，运行 `rbrain sync` 使数据库与文件一致。

### 导出

```bash
rbrain export --dir /tmp/backup --format md    # 导出为 Markdown
rbrain export --dir /tmp/backup --format json  # 导出为 JSON
```

---

## 自定义 Pipeline Profile

### Profile 文件位置

放在 `$RBRAIN_HOME/profiles/` 或 `~/.rbrain/profiles/` 下，文件名即 profile 名。

### 完整 TOML 格式参考

```toml
[profile]
name = "my_workflow"
description = "自定义三阶段工作流"

# ─── Stage 1: Extract ─────────────────────────────────────────────────────

[[stages]]
id = "extract"
enabled = true

# Input
input_mode = "self"         # 每页单独处理
page_type  = "note"         # 处理哪种类型的页面
# tag = "my-tag"            # 可选：只处理带有该标签的页面
# language = "zh-hans"      # 可选：只处理该语言的页面
# slugs = ["raw/a", "raw/b"] # 可选：只处理这些 slug

# LLM
prompt          = "extract_academic"   # prompts/ 目录下的文件名（无扩展名）
response_format = "json"               # json | markdown

# Output
output_mode = "save_multi"             # 一个输入 → 多种类型输出

  [stages.type_map.concepts]
  page_type    = "concept"
  slug_prefix  = "research/concepts/"
  embed        = false
  enrich_existing = true               # 追加而非覆盖已有 concept

  [stages.type_map.figures]
  page_type    = "figure"
  slug_prefix  = "research/figures/"
  embed        = false

# Execution
incremental = true    # 跳过已处理页面
batch_size  = 1       # 每批处理页数

# Context
inject_existing_titles = "concept"   # 注入已有 concept 名称到 prompt

# ─── Stage 2: Synthesize ──────────────────────────────────────────────────

[[stages]]
id = "synthesize"
enabled = true

input_mode  = "linked_sources"
anchor_type = "concept"   # 以 concept 为锚点
source_type = "note"      # 查找引用该 concept 的 note 页面
min_sources = 2           # 至少 N 篇来源才触发综合
use_chunks  = true        # true = 用 DB chunks（高保真）；false = compiled_truth 截断
dedup_sources_threshold = 0.8  # Jaccard 去重，0.0 = 禁用

prompt          = "synthesize_academic"
response_format = "markdown"

output_mode        = "save"
output_page_type   = "synthesis"
output_slug_prefix = "research/synthesis/"
embed_output       = true

incremental = true
batch_size  = 1

# ─── Stage 3: Compose ─────────────────────────────────────────────────────

[[stages]]
id = "compose"
enabled = true

input_mode  = "aggregate"    # 所有指定类型页面合并为单次 LLM 调用
page_type   = "synthesis"
max_pages   = 25             # 最多聚合 N 个页面（most-recently-updated first）
chars_per_page = 2500        # 每页截取字符数（防止 context window overflow）

prompt          = "compose_literature_review"
response_format = "markdown"

output_mode        = "save"
output_page_type   = "wiki"
output_slug_prefix = "research/wiki/"
embed_output       = true

incremental = true
```

### 自定义 Prompt

在 `$RBRAIN_HOME/prompts/` 或 `~/.rbrain/prompts/` 下放置 `.md` 文件。

Extract 阶段的 prompt 需要 LLM 返回 JSON，格式示例：
```json
{
  "concepts": [{"name": "...", "description": "..."}],
  "figures": [{"name": "...", "description": "..."}]
}
```

Synthesize 和 Compose 阶段的 prompt 返回 Markdown。

引用格式约定（在 prompt 中说明）：
- 引用格式：`[[raw/articles/slug | chunk:N]]`
- N 是上下文中显示的 `[chunk:N | slug]` 标签中的 chunk ID
- 只引用原始文献（`raw/articles/` 路径），不引用 synthesis 页面本身

### 使用内置 Profile 为模板

查看内置 profile：
```bash
# 内置 profile 位于 rbrain-hub 源码的 crates/rbrain-engine/src/profiles/
ls crates/rbrain-engine/src/profiles/
```

复制修改：
```bash
cp crates/rbrain-engine/src/profiles/literature_review.toml \
   ~/.rbrain/profiles/my_review.toml
# 编辑 my_review.toml
rbrain dream --profile my_review
```

---

## 常见问题

### Q：`rbrain dream --profile` 卡住没有输出

运行时加重定向到日志文件，便于监控：
```bash
rbrain dream --profile literature_review > run.log 2>&1 &
tail -f run.log
```

每篇文章会输出 `[i/total] stage 'slug'` 进度行。

### Q：Synthesize 阶段跳过了所有 concept

原因：所有 concept 的 source 数量都少于 `min_sources`。

解决：降低 `min_sources = 2`，或检查 extract 阶段是否成功建立了 `mentions` 链接：
```bash
# 检查 concept 的入向链接数量
rbrain backlinks research/concepts/自主知识体系
```

### Q：同一个概念被创建了多个近义词页面

原因：extract 阶段没有开启 `inject_existing_titles`。

解决：
1. 在 profile 的 extract stage 添加 `inject_existing_titles = "concept"`
2. 删除旧的 concept 页面，重新跑 extract
3. 或者运行 `rbrain dream --stage merge-concepts` 合并相似概念

### Q：引用格式错误（synthesis 中引用了 concept 而不是 raw 文献）

原因：synthesize prompt 格式说明不清晰，或 LLM 理解错误。

检查：
```bash
grep -h '\[\[' research/synthesis/*.md | grep 'chunk:' | grep -v 'raw/articles/'
```

如有输出（非 raw/articles 的引用），说明存在错误引用。需要检查 `synthesize_academic.md` prompt 中的引用格式说明。

### Q：`rbrain embed` 一直报错

通常是 Qwen API 密钥或 base_url 配置错误。验证：
```bash
rbrain stats   # 如果嵌入覆盖率为 0 且页面数 > 0，说明 embed 从未成功
```

临时绕过（离线模式）：
```bash
rbrain put my/page --file draft.md --mock-embed
```

### Q：如何完全重置知识库

```bash
rm -rf .rbrain/
rbrain init
```

这会删除所有数据库和索引。源文献的 `.md` 文件不受影响。

### Q：dream cycle 和 pipeline profile 的区别

| `rbrain dream` | `rbrain dream --profile` |
|---|---|
| 硬编码四阶段（lint/embed/extract/synthesize） | TOML 配置任意阶段组合 |
| 使用内置 prompt 和逻辑 | 可自定义 prompt 和参数 |
| 无 compose 阶段 | 支持 aggregate → compose 输出完整文档 |
| 简单直接 | 灵活可扩展 |

对于简单的概念提取和综合，用 `rbrain dream`。需要生成完整文献综述，用 `rbrain dream --profile literature_review`。
