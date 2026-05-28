# rbrain-hub

**rbrain-hub** is the team/cloud edition of [rbrain](https://github.com/hongyuatcufe/rbrain) — a Rust-based AI knowledge base CLI for academic research. It replaces the single-user usearch HNSW index with [LanceDB](https://lancedb.github.io/lancedb/) (MVCC concurrent writes, S3 backend) and adds Qwen dense+sparse dual-vector hybrid search.

**rbrain-hub** 是 [rbrain](https://github.com/hongyuatcufe/rbrain) 的团队/云版本，面向学术研究。将单用户的 usearch HNSW 索引替换为 [LanceDB](https://lancedb.github.io/lancedb/)（MVCC 并发写入，支持 S3），并增加了 Qwen 稠密+稀疏双向量混合检索。

---

## What's different from rbrain / 与 rbrain 的主要区别

| | rbrain (personal) | rbrain-hub (team/cloud) |
|---|---|---|
| Vector store | usearch HNSW (in-process) | LanceDB (MVCC, S3-ready) |
| Embedding | Qwen dense only | Qwen dense + sparse (`output_type="dense&sparse"`) |
| Hybrid search | 2-way RRF (dense + BM25) | 3-way RRF (dense + sparse + BM25) |
| Concurrent writes | Single-threaded Mutex | LanceDB MVCC |
| Cloud storage | Local disk only | LanceDB → S3; SQLite + Tantivy → EBS/EFS |
| CJK full-text | Tantivy + Lindera | Tantivy + Lindera (unchanged) |

> **Why keep Tantivy?** LanceDB's Rust SDK FTS has open CJK issues (#2168, #2329). The existing Tantivy pipeline (per-language indices, Lindera morphological analysis, traditional→simplified conversion, CJK stopwords) is more robust and is kept as-is.

---

## Features / 功能特性

- **3-way hybrid search / 三路混合检索** — BM25 keyword + dense ANN + sparse ANN via parallel `tokio::join!`, merged with Reciprocal Rank Fusion; sparse falls back gracefully if unavailable
- **Dual embedding / 双向量嵌入** — single Qwen API call returns both dense (1024-dim) and sparse vectors via `output_type="dense&sparse"`
- **LanceDB vector store** — MVCC concurrent-safe writes, IVF-PQ ANN index (auto-built at ≥256 rows), S3-compatible storage backend
- **Knowledge graph / 知识图谱** — typed directed links anchored to specific passages (`evidence`, `related`, `supports`, `contrasts`, `develops`)
- **Dream Cycle / 自动知识提取流水线** — lint → embed → extract concepts/figures → synthesize concept clusters
- **Think / 深度推理** — structured reasoning over retrieved context: tensions, judgments, open questions
- **Timeline / 时间线** — dated evidence log attached to any page
- **Takes / 诠释片段** — interpretive fragments (`judgment`, `question`, `hypothesis`, `interpretation`) without overwriting page content
- **MCP server / MCP 服务端** — expose all brain operations to Claude and other MCP-compatible AI assistants
- **CJK support / 中日韩支持** — language detection (zh-hans, zh-hant, ja, ko, en), CJK-safe chunking, Lindera morphological analysis, traditional→simplified normalization

---

## Install / 安装

```bash
git clone https://github.com/hongyuatcufe/rbrain-hub
cd rbrain-hub
cargo build --release -p rbrain-cli

# add to PATH
ln -sf "$PWD/target/release/rbrain" ~/.local/bin/rbrain
```

Requires Rust 1.85+. / 需要 Rust 1.85+。

---

## Quick Start / 快速上手

```bash
# Initialize a project brain / 初始化项目知识库
cd my-research-project
rbrain init

# Configure API keys (.rbrain/config.toml is gitignored)
# 配置 API 密钥（.rbrain/config.toml 默认已加入 .gitignore）
cat > .rbrain/config.toml <<EOF
[qwen]
api_key = "sk-..."
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "text-embedding-v4"

[deepseek]
api_key = "sk-..."
base_url = "https://api.deepseek.com/v1"
model = "deepseek-chat"

embedding_dim = 1024
EOF

# Import source articles / 导入源文献
rbrain import ./papers/

# Run full dream cycle / 运行完整自动化流水线
# embed → extract concepts/figures → synthesize
rbrain dream

# Search and retrieve / 检索
rbrain search "自主知识体系"
rbrain query "中国教育学的本土化路径" --expand
rbrain get concepts/自主知识体系
```

---

## Storage Layout / 存储结构

```
.rbrain/
  config.toml      ← API keys and settings (gitignored)
  brain.db         ← SQLite: pages, chunks, links, dream_metadata
  lance/           ← LanceDB: dense + sparse vectors  (→ S3 in cloud deploy)
  tantivy/         ← Tantivy BM25 index               (local disk / EFS only)
  dictionaries/    ← Lindera CJK dictionaries
```

Cloud deployment: point `lance_dir` at an S3 URI; keep SQLite and Tantivy on EBS (single instance) or EFS (multi-replica).

---

## CLI Reference / 命令参考

### Brain Management / 知识库管理

```bash
rbrain init                    # create .rbrain/ in CWD / 在当前目录初始化
rbrain stats                   # page/chunk/link/embedding counts / 统计信息
rbrain doctor                  # health check / 健康检查
rbrain lint                    # issues: broken links, orphans, unembedded pages
```

### Reading & Writing Pages / 读写页面

```bash
rbrain put <slug> --file <path>          # write page from file / 从文件写入页面
rbrain get <slug>                        # read page + timeline / 读取页面及时间线
rbrain list [--type <type>] [--tag <t>]  # list pages / 列出页面
rbrain import <dir>                      # import all .md files / 批量导入
rbrain export --dir <out> --format md    # export as Markdown / 导出为 Markdown
rbrain export --dir <out> --format json  # export as JSON / 导出为 JSON
```

Page types / 页面类型: `note` | `concept` | `figure` | `synthesis` | `wiki` | `question` | `evidence` | `draft` | `memo` | `period` | `book`

### Search & Retrieval / 检索

```bash
rbrain search "query"                         # BM25 keyword search / BM25 关键词检索
rbrain search "query" --tag t --type concept  # filtered / 过滤检索
rbrain query "question" --expand              # 3-way hybrid + query expansion / 三路混合检索
```

### Knowledge Graph / 知识图谱

```bash
rbrain link <from> <to> --type evidence --from-chunk <id>   # 链接到具体段落
rbrain link <from> <to> --type related
rbrain unlink <from> <to> [--type <t>]
rbrain backlinks <slug>          # incoming links / 反向链接
rbrain links <slug>              # outgoing links / 出向链接
rbrain graph-query <slug> --depth 2 --direction both
rbrain orphans                   # pages with no incoming links / 孤立页面
rbrain extract --all             # extract [[wikilinks]] from content / 提取双链
```

Link types / 链接类型: `evidence` | `related` | `supports` | `contrasts` | `develops`

### Timeline / 时间线

```bash
rbrain timeline <slug> --text "..." --date "2026-05-15" --source "<slug> chunk:<id>"
# --date defaults to today / --date 默认为今天
```

### Takes / 诠释片段

```bash
rbrain take <slug> "judgment text" --kind judgment
rbrain takes <slug>              # list all takes / 列出所有诠释
```

`--kind`: `judgment` | `question` | `hypothesis` | `interpretation`

### Tags / 标签

```bash
rbrain tag <slug> <tag>
rbrain untag <slug> <tag>
rbrain tags <slug>
```

### Synthesis / 综合分析

```bash
rbrain think "topic" --limit 12 --expand   # deep reasoning / 深度推理
rbrain think "topic" --save                # saves as synthesis/<slug>
rbrain generate "topic" --limit 12 --expand  # wiki-style summary / 百科综述
rbrain generate "topic" --save             # saves as wiki/<slug>
```

### Embeddings / 向量嵌入

```bash
rbrain embed --stale     # re-embed stale pages / 重新嵌入过期页面
rbrain embed --all       # re-embed everything / 重新嵌入全部
rbrain sync --embed      # sync filesystem → DB then re-embed / 同步后重新嵌入
```

### Dream Cycle / 自动化流水线

```bash
rbrain dream                        # full pipeline / 完整流水线
rbrain dream --stage lint           # Phase 1: lint
rbrain dream --stage embed          # Phase 2: embed stale pages / 嵌入过期页面
rbrain dream --stage extract        # Phase 3: extract concepts & figures / 提取概念和人物
rbrain dream --stage synthesize     # Phase 4: synthesize concept clusters / 概念聚类综合
```

**Phase 3 — Extract**: For each unprocessed `note`, calls DeepSeek to extract concepts, figures, and timeline events. Creates `concepts/<slug>` and `figures/<slug>` pages; events associated with people are written to figure pages. Idempotent via `dream_metadata`.

对每篇未处理的 `note`，调用 DeepSeek 提取概念、学者/人物和时间线事件。自动创建 `concepts/<slug>` 和 `figures/<slug>` 页面；关联人物的事件写入人物页。通过 `dream_metadata` 保证幂等。

**Phase 4 — Synthesize**: For each concept with 3+ source note backlinks, generates a structured synthesis at `synthesis/<concept-slug>`. Auto-links synthesis → concept (`develops`) and synthesis → source notes (`evidence`). Re-synthesizes when sources are updated.

对有 3 篇以上源文章反向链接的概念，自动生成结构化综合页面，保存至 `synthesis/<concept-slug>`。源文章更新后自动重新综合。

### Citation Workflow / 引用工作流

```bash
rbrain cite <slug> --depth 3 --append   # collect sources, append bibliography
rbrain audit <slug>                     # check citation quality (ERRORs and WARNs)
rbrain audit <slug> --fix               # auto-fix duplicates and orphan entries
```

---

## MCP Server / MCP 服务端

rbrain-hub exposes all brain operations to Claude Code and other MCP-compatible clients.

```bash
rbrain serve mcp                          # stdio mode (for Claude Code)
rbrain serve mcp --http 127.0.0.1:3456   # local HTTP mode
```

HTTP mode only accepts loopback addresses. Do not expose to a network without authentication.

### Claude Code Setup / Claude Code 配置

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

### MCP Tools / MCP 工具列表

| Tool | Description |
|------|-------------|
| `brain_put` | Write or update a page |
| `brain_get` | Read a page |
| `brain_delete` | Delete a page |
| `brain_list` | List pages with optional type/tag filter |
| `brain_query` | 3-way hybrid semantic search |
| `brain_think` | Deep reasoning synthesis on a topic |
| `brain_generate` | Search + LLM wiki synthesis |
| `brain_link` | Create a typed graph link |
| `brain_unlink` | Remove a link |
| `brain_backlinks` | Get incoming links |
| `brain_outlinks` | Get outgoing links |
| `brain_graph` | Traverse graph neighborhood |
| `brain_orphans` | List pages with no incoming links |
| `brain_add_timeline_entry` | Add a dated entry to a page |
| `brain_add_tag` | Add a tag |
| `brain_remove_tag` | Remove a tag |
| `brain_stats` | Brain statistics |

---

## Architecture / 架构

```
rbrain-hub/
├── crates/
│   ├── rbrain-cli/      # CLI entry point (clap)
│   ├── rbrain-engine/   # Core logic: dream, hybrid search, linking, synthesis
│   ├── rbrain-core/     # Page model, SparseVec, VectorStore/Embedder traits
│   ├── rbrain-db/       # SQLite schema and queries (sqlx)
│   ├── rbrain-search/   # LanceStore (LanceDB) + Tantivy BM25
│   ├── rbrain-llm/      # DeepSeek chat + Qwen dual-embedding client
│   ├── rbrain-mcp/      # MCP server (stdio + HTTP)
│   └── rbrain-worker/   # Background job queue
└── spike/lancedb/       # LanceDB feasibility spike (validation only)
```

**Embedding**: Qwen `text-embedding-v4` — single API call returns 1024-dim dense vector + sparse vector via `output_type="dense&sparse"`

**LLM**: DeepSeek `deepseek-chat` — extraction, synthesis, think, generate

**Storage**:
- SQLite (`brain.db`) — pages, chunks, links, dream_metadata, tags, takes, timeline
- LanceDB (`lance/`) — dense `FixedSizeList<Float32>[1024]` + sparse `List<Int64>` / `List<Float32>`; IVF-PQ ANN index auto-built at ≥256 rows
- Tantivy (`tantivy/`) — BM25 full-text with per-language indices (zh-hans, zh-hant, ja, ko, en) and Lindera morphological analysis

**Hybrid search pipeline**:
```
query
  ├── dense ANN    (LanceDB, _distance)      ─┐
  ├── sparse ANN   (stub → empty in 0.17)    ─┤─ tokio::join! → 3-way RRF
  └── BM25 keyword (Tantivy + Lindera)       ─┘
```

---

## Brain Auto-Discovery / 知识库自动发现

rbrain-hub walks up from CWD to discover the active brain:
1. finds `.rbrain/` → uses project-local brain
2. falls back to `~/.rbrain/`

No `BRAIN_HOME` environment variable needed. / 无需设置环境变量。

---

## Roadmap / 路线图

- **Phase 3**: Multi-tenant HTTP routing via `X-Brain-ID` header; per-tenant LanceDB table isolation
- **Sparse ANN**: Enable when LanceDB Rust SDK adds sparse index support (tracking upstream)
- **S3 backend**: Validated in spike; production wiring pending Phase 3

---

## Skill / Claude Code 技能

A Claude Code skill for research workflows is at [`rbrain-research-cli/SKILL.md`](rbrain-research-cli/SKILL.md). Covers brain discovery, retrieval, writing, graph linking, dream cycle, citation workflow, and MCP usage patterns.
