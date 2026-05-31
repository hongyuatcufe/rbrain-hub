# rbrain-hub

**rbrain-hub** is the team/cloud edition of [rbrain](https://github.com/hongyuatcufe/rbrain) — a Rust-based AI knowledge base CLI for academic research. It replaces the single-user usearch HNSW index with [LanceDB](https://lancedb.github.io/lancedb/) (MVCC concurrent writes, S3 backend), adds Qwen dense+sparse dual-vector hybrid search, and introduces a programmable multi-stage LLM pipeline for automated knowledge synthesis.

**rbrain-hub** 是 [rbrain](https://github.com/hongyuatcufe/rbrain) 的团队/云版本，面向学术研究。将单用户的 usearch HNSW 索引替换为 [LanceDB](https://lancedb.github.io/lancedb/)（MVCC 并发写入，支持 S3），增加了 Qwen 稠密+稀疏双向量混合检索，并引入可编程多阶段 LLM 流水线，支持自动化知识综合。

---

## What's different from rbrain / 与 rbrain 的主要区别

| | rbrain (personal) | rbrain-hub (team/cloud) |
|---|---|---|
| Vector store | usearch HNSW (in-process) | LanceDB (MVCC, S3-ready) |
| Embedding | Qwen dense only | Qwen dense + sparse (`output_type="dense&sparse"`) |
| Hybrid search | 2-way RRF (dense + BM25) | 3-way RRF (dense + sparse + BM25) |
| Concurrent writes | Single-threaded Mutex | LanceDB MVCC |
| Cloud storage | Local disk only | LanceDB → S3; SQLite + Tantivy → EBS/EFS |
| Pipeline | Hardcoded dream stages | TOML-configurable multi-stage profiles |
| CJK full-text | Tantivy + Lindera | Tantivy + Lindera (unchanged) |

> **Why keep Tantivy?** LanceDB's Rust SDK FTS has open CJK issues (#2168, #2329). The existing Tantivy pipeline (per-language indices, Lindera morphological analysis, traditional→simplified conversion, CJK stopwords) is more robust and is kept as-is.

---

## Features / 功能特性

- **3-way hybrid search / 三路混合检索** — BM25 keyword + dense ANN + sparse ANN via parallel `tokio::join!`, merged with Reciprocal Rank Fusion
- **Dual embedding / 双向量嵌入** — single Qwen API call returns both dense (1024-dim) and sparse vectors via `output_type="dense&sparse"`
- **LanceDB vector store** — MVCC concurrent-safe writes, IVF-PQ ANN index (auto-built at ≥256 rows), S3-compatible storage backend
- **Knowledge graph / 知识图谱** — typed directed links anchored to specific passages (`evidence`, `related`, `supports`, `contrasts`, `develops`)
- **Programmable pipeline profiles / 可编程流水线** — TOML-configured EXTRACT → SYNTHESIZE → COMPOSE stages; run with `dream --profile <name>`
- **Dream Cycle / 自动知识提取流水线** — lint → embed → extract concepts/figures → synthesize concept clusters
- **Literature review automation / 文献综述自动化** — `literature_review` profile runs extract → synthesize → compose, producing a structured academic review from raw articles
- **Concept deduplication / 概念去重** — Jaccard-based source-set dedup prevents near-identical synthesis pages; embedding-based concept merging via `merge-concepts`
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

# Option A: step-by-step dream cycle
rbrain dream                        # embed → extract → synthesize

# Option B: full literature review pipeline (extract → synthesize → compose)
rbrain dream --profile literature_review

# Search and retrieve / 检索
rbrain search "自主知识体系"
rbrain query "中国教育学的本土化路径" --expand
rbrain get research/wiki/compose
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
rbrain init                    # create .rbrain/ in CWD
rbrain stats                   # page/chunk/link/embedding counts
rbrain doctor                  # health check: broken links, orphans, unembedded pages
rbrain doctor --fix            # auto-fix stale chunks and orphan pages
rbrain lint                    # quality issues report
```

### Reading & Writing Pages / 读写页面

```bash
rbrain put <slug> --file <path>          # write page from file
rbrain put <slug> --content "..."        # write page from inline content
rbrain get <slug>                        # read page + timeline
rbrain delete <slug>                     # delete page and embeddings
rbrain list [--type <type>] [--tag <t>] [--limit <n>] [--json]
rbrain import <dir>                      # import all .md files
rbrain export --dir <out> --format md    # export as Markdown
rbrain export --dir <out> --format json  # export as JSON
rbrain sync                              # sync filesystem → database
rbrain sync --embed                      # sync + re-embed changed pages
```

Page types / 页面类型: `note` | `concept` | `figure` | `synthesis` | `wiki` | `question` | `evidence` | `draft` | `memo` | `period` | `book`

### Search & Retrieval / 检索

```bash
rbrain search "query"                         # BM25 keyword search
rbrain search "query" --tag t --type concept  # filtered search
rbrain query "question" --expand              # 3-way hybrid + query expansion
```

### Knowledge Graph / 知识图谱

```bash
rbrain link <from> <to> --type evidence --from-chunk <id>
rbrain unlink <from> <to> [--type <t>]
rbrain backlinks <slug>                  # incoming links
rbrain links <slug>                      # outgoing links with context
rbrain graph-query <slug> --depth 2 --direction both
rbrain orphans                           # pages with no incoming links
rbrain extract --all                     # re-index [[wikilinks]] from content
```

Link types / 链接类型: `evidence` | `related` | `supports` | `contrasts` | `develops`

### Dream Cycle / 自动化流水线

```bash
rbrain dream                        # full pipeline (lint → embed → extract → synthesize)
rbrain dream --stage lint           # Phase 1: lint
rbrain dream --stage embed          # Phase 2: embed stale pages
rbrain dream --stage extract        # Phase 3: extract concepts & figures
rbrain dream --stage synthesize     # Phase 4: synthesize concept clusters
rbrain dream --stage merge-concepts # merge semantically similar concepts (threshold 0.85)

# Pipeline profiles (TOML-configured multi-stage workflows)
rbrain dream --profile literature_review   # extract → synthesize → compose
rbrain dream --profile policy_analysis     # extract → synthesize
```

**Phase 3 — Extract**: For each unprocessed `note`, calls DeepSeek to extract concepts, figures, and timeline events. Creates `research/concepts/<slug>` and `research/figures/<slug>` pages. Idempotent via `dream_metadata`. Uses `inject_existing_titles` to normalize concept names against the existing vocabulary.

**Phase 4 — Synthesize**: For each concept with N+ source note backlinks (configurable `min_sources`), generates a structured synthesis. Skips near-duplicate concept pairs by Jaccard source-set similarity (`dedup_sources_threshold`).

### Pipeline Profiles / 流水线 Profile

Profiles are TOML files in `$RBRAIN_HOME/profiles/` (or built-in). Each profile defines ordered stages:

```toml
[[stages]]
id = "extract"
input_mode = "self"          # self | linked_sources | aggregate
page_type = "note"
prompt = "extract_academic"
response_format = "json"
output_mode = "save_multi"   # return | save | save_multi | update_frontmatter
inject_existing_titles = "concept"
incremental = true
batch_size = 1

  [stages.type_map.concepts]
  page_type = "concept"
  slug_prefix = "research/concepts/"
  enrich_existing = true      # append to existing concept pages

[[stages]]
id = "synthesize"
input_mode = "linked_sources"
anchor_type = "concept"
source_type = "note"
min_sources = 2               # minimum source articles per concept
dedup_sources_threshold = 0.8 # skip near-duplicate concept pairs
prompt = "synthesize_academic"
response_format = "markdown"
output_mode = "save"
output_page_type = "synthesis"
output_slug_prefix = "research/synthesis/"

[[stages]]
id = "compose"
input_mode = "aggregate"      # aggregate ALL synthesis pages into ONE LLM call
page_type = "synthesis"
max_pages = 25
chars_per_page = 2500
prompt = "compose_literature_review"
response_format = "markdown"
output_mode = "save"
output_page_type = "wiki"
output_slug_prefix = "research/wiki/"
```

Built-in profiles: `literature_review`, `policy_analysis`

Custom profiles: place `.toml` files in `$RBRAIN_HOME/profiles/` or `~/.rbrain/profiles/`.

### Synthesis / 综合分析

```bash
rbrain think "topic" --limit 12 --expand          # deep reasoning
rbrain think "topic" --save                        # saves as synthesis/<slug>
rbrain generate "topic" --limit 12 --expand        # wiki-style summary
rbrain generate "topic" --save                     # saves as wiki/<slug>
```

### Citation Workflow / 引用工作流

```bash
rbrain cite <slug> --depth 3 --append     # collect sources, append bibliography
rbrain audit <slug>                        # check citation quality (ERRORs and WARNs)
rbrain audit <slug> --fix                  # auto-fix duplicates and orphan entries
```

### Embeddings / 向量嵌入

```bash
rbrain embed --stale     # re-embed stale pages
rbrain embed --all       # re-embed everything
```

### Timeline & Takes / 时间线和诠释

```bash
rbrain timeline <slug> --text "..." --date "2026-05-15" --source "<slug> chunk:<id>"
rbrain take <slug> "judgment text" --kind judgment
rbrain takes <slug>
```

`--kind`: `judgment` | `question` | `hypothesis` | `interpretation`

### Tags / 标签

```bash
rbrain tag <slug> <tag>
rbrain untag <slug> <tag>
rbrain tags <slug>
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
| `brain_list` | List pages with optional type/tag/language filter |
| `brain_query` | 3-way hybrid semantic search |
| `brain_think` | Deep reasoning synthesis on a topic |
| `brain_generate` | Search + LLM wiki synthesis |
| `brain_link` | Create a typed graph link |
| `brain_unlink` | Remove a link |
| `brain_backlinks` | Get incoming links |
| `brain_outlinks` | Get outgoing links with context |
| `brain_graph` | Traverse graph neighborhood |
| `brain_orphans` | List pages with no incoming links |
| `brain_add_timeline_entry` | Add a dated entry to a page |
| `brain_add_tag` | Add a tag |
| `brain_remove_tag` | Remove a tag |
| `brain_stats` | Brain statistics |
| `brain_process` | Batch LLM processing with a custom task description |

---

## Architecture / 架构

```
rbrain-hub/
├── crates/
│   ├── rbrain-cli/      # CLI entry point (clap)
│   ├── rbrain-engine/   # Core logic: pipeline, dream, hybrid search, linking, synthesis
│   │   ├── src/pipeline.rs       # PipelineStep, InputSpec, OutputMode structs
│   │   ├── src/engine.rs         # Engine: run_pipeline_step, run_dream_cycle
│   │   ├── src/profiles/         # Built-in TOML pipeline profiles
│   │   └── src/prompts/          # Built-in LLM prompt templates
│   ├── rbrain-core/     # Page model, SparseVec, VectorStore/Embedder traits
│   ├── rbrain-db/       # SQLite schema and queries (sqlx)
│   ├── rbrain-search/   # LanceStore (LanceDB) + Tantivy BM25
│   ├── rbrain-llm/      # DeepSeek chat + Qwen dual-embedding client
│   ├── rbrain-mcp/      # MCP server (stdio + HTTP), 18 tools
│   └── rbrain-worker/   # Background job queue
└── spike/lancedb/       # LanceDB feasibility spike (validation only)
```

**Embedding**: Qwen `text-embedding-v4` — single API call returns 1024-dim dense vector + sparse vector via `output_type="dense&sparse"`

**LLM**: DeepSeek `deepseek-chat` — extraction, synthesis, think, generate, compose

**Storage**:
- SQLite (`brain.db`) — pages, chunks, links, dream_metadata, tags, takes, timeline
- LanceDB (`lance/`) — dense `FixedSizeList<Float32>[1024]` + sparse `List<Int64>` / `List<Float32>`; IVF-PQ ANN index auto-built at ≥256 rows
- Tantivy (`tantivy/`) — BM25 full-text with per-language indices and Lindera morphological analysis

**Hybrid search pipeline**:
```
query
  ├── dense ANN    (LanceDB, _distance)     ─┐
  ├── sparse ANN   (LanceDB)                ─┤─ tokio::join! → 3-way RRF
  └── BM25 keyword (Tantivy + Lindera)      ─┘
```

**Pipeline execution**:
```
dream --profile <name>
  └── for each stage in profile.toml:
        ├── InputSpec::SelfContent     → one LLM call per page
        ├── InputSpec::LinkedSources   → one LLM call per anchor (concept)
        └── InputSpec::AggregateContent → ONE LLM call for all pages combined
```

---

## Brain Auto-Discovery / 知识库自动发现

rbrain-hub walks up from CWD to discover the active brain:
1. finds `.rbrain/` → uses project-local brain
2. falls back to `~/.rbrain/`

No `BRAIN_HOME` environment variable needed.

Override with `--brain-dir <path>` or `RBRAIN_HOME=<path>`.

---

## Skill / Claude Code 技能

A Claude Code skill for research workflows is at [`rbrain-research-cli/SKILL.md`](rbrain-research-cli/SKILL.md).

A comprehensive usage guide (installation through advanced pipeline authoring) is at [`guide.md`](guide.md).

---

## Roadmap / 路线图

- **Citation graph**: Parse reference sections in dream_extract → write `cites` links; weight `think` retrieval by citation indegree
- **Phase 3**: Multi-tenant HTTP routing via `X-Brain-ID` header; per-tenant LanceDB table isolation
- **Sparse ANN**: Enable when LanceDB Rust SDK adds sparse index support (tracking upstream)
- **S3 backend**: Validated in spike; production wiring pending Phase 3
