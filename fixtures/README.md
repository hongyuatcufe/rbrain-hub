# M1 fixtures

End-to-end acceptance fixtures for the v2 execution plan. See
`../../rbrain-hub-execution-plan.md` and `../../CLAUDE.md` §8.

Two fixtures:

| Fixture | API keys required | Where it lives |
|---|---|---|
| `data_analysis_demo` | No (MockEmbedder) | `crates/rbrain-engine/tests/data_analysis_fixture.rs` |
| `literature_review_demo` | **Yes** — DeepSeek + DashScope/Qwen | This directory + existing `/Users/hongyu/project/rbrain-test` corpus |

## data_analysis_demo

Already wired as an integration test. Run:

```bash
cargo test -p rbrain-engine --test data_analysis_fixture
```

What it covers (3 tests):

- `data_analysis_protocol_runs_end_to_end` — full M1 path: create_research_run
  → register_input(dataset) → register_input(artifact) → record(finding,
  status=claim) → all three validators pass → protocol halts at
  `record_analysis_plan` (M2 validator).
- `validators_emit_typed_actions_for_missing_dataset` — confirms the
  `dataset_registered` validator emits a typed `SuggestedAction::RegisterDataset`.
- `draft_finding_only_warns_not_fails` — confirms a finding with `status: draft`
  raises `warn`, not `fail` (CLAUDE.md §5).

No API keys, no network. CI-safe.

## literature_review_demo

Reuses the existing 55-paper corpus at `/Users/hongyu/project/rbrain-test`.
Requires real API keys for embeddings and synthesis.

### 1. Config

`/Users/hongyu/project/rbrain-test/.rbrain/config.toml`:

```toml
[qwen]
api_key = "sk-..."                 # DashScope, used for dense+sparse embeddings
# base_url defaults to https://dashscope.aliyuncs.com/compatible-mode/v1

[deepseek]
api_key = "sk-..."                 # used for extract / synthesize / compose
model = "deepseek-v4-flash"         # extract stage
model_pro = "deepseek-v4-pro"       # synthesize + compose stages
```

### 2. Sync + embed

```bash
cd /Users/hongyu/project/zeroclaw_with_rbrain/rbrain-hub
cargo run -p rbrain-cli -- \
    --brain-dir /Users/hongyu/project/rbrain-test/.rbrain sync

cargo run -p rbrain-cli -- \
    --brain-dir /Users/hongyu/project/rbrain-test/.rbrain embed --all

cargo run -p rbrain-cli -- \
    --brain-dir /Users/hongyu/project/rbrain-test/.rbrain stats
```

Expect (per progress.md 2026-06-02):

- 55 raw/note pages
- ~696 chunks
- 100% embedding coverage

### 3. Run the literature_review profile

```bash
cargo run -p rbrain-cli -- \
    --brain-dir /Users/hongyu/project/rbrain-test/.rbrain \
    dream --profile literature_review
```

This is the rbrain-led mode (CLAUDE.md §3). ZeroClaw would normally
trigger it via `brain_process` over MCP.

### 4. M0 validator gates (the part this milestone wires)

After the dream completes:

```bash
# Sparse degradation warning shows up in the doctor report.
cargo run -p rbrain-cli -- \
    --brain-dir /Users/hongyu/project/rbrain-test/.rbrain doctor
# Expected: "SPARSE_DEGRADED: sparse ANN unavailable..."

# Per-result attribution for a query — verifies dense/BM25/sparse channels.
cargo run -p rbrain-cli -- \
    --brain-dir /Users/hongyu/project/rbrain-test/.rbrain query \
    "教育公平" --explain --limit 5
# Expected lines:
#   ⚠  SPARSE_DEGRADED — sparse ANN unavailable; RRF used dense + BM25 only.
#   [1] chunk=... rrf=... page=... (note)
#       dense: rank=1 score=...
#       bm25:  rank=3 score=...
#       sparse: rank=— score=— (enabled=false)

# Citation check on the composed wiki review.
cargo run -p rbrain-cli -- \
    --brain-dir /Users/hongyu/project/rbrain-test/.rbrain \
    audit research/wiki/<slug-of-composed-review>
# Or via MCP: brain_citation_check { slug: "research/wiki/..." }
```

### 5. Expected acceptance criteria (M3 fully delivered)

This fixture is the M3 regression baseline. M3 has not shipped yet, so
the current expected output is:

- doctor reports `SPARSE_DEGRADED` (M0 ✅).
- `query --explain` shows dense + BM25 attribution with sparse=disabled (M0 ✅).
- `brain_citation_check` on the wiki review returns `pass` or `warn` only — no
  ERRORs about citation_type (because synthesis prompts already enforce raw/note
  citations; see progress.md 2026-05-27).
- Generated draft contains ≥ 8 `##` sections, traceable `[[raw/articles/... | chunk:N]]`
  citations, ≥ 35 referenced source articles (per the 2026-06-02 baseline).

### Notes

- This fixture is **not** wired as a `cargo test` because it requires live API
  keys and ~minutes of LLM calls. Run it manually before M3 declares ready.
- If you want to switch the embedding provider, see `[qwen]` in
  `crates/rbrain-core/src/config.rs`.
- ZeroClaw-led mode is the alternative path (CLAUDE.md §3) — it uses
  `brain_query`/`brain_get`/`brain_think` repeatedly and finishes with
  `brain_citation_check`. That flow is not scripted here because it is
  authored by ZeroClaw at runtime.
