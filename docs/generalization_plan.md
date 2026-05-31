# rbrain-hub 泛化架构：统一 Pipeline 参数体系

## Context

代码分析结论：`dream_extract`（~320行）和 `dream_synthesize`（~168行）共享约60%结构（LLM调用、页面创建、链接建立）。  
核心差异只有3点：①上下文组装方式（自身内容 vs 关联来源）②响应格式（JSON vs Markdown）③输出模式（一对多 vs 多对一）。  
这3个差异可以用枚举参数化，从而将所有阶段（EXTRACT/ANNOTATE/AGGREGATE/EVALUATE/TRANSFORM）统一到一个 `PipelineStep` + `PipelineRunner` 中，做到：**新增自定义阶段 = 新 TOML 配置 + 新 Prompt 文件，无需改代码**。

**关于 LangChain 集成（调研结论）**：无官方 Rust SDK，`langchain-rust` 社区版功能杂乱，`rig`（ArcadeAI）更专注但侧重 Agent/Tool。rbrain-hub 当前 rbrain-llm 是轻薄 HTTP 客户端，设计已经符合 Rust 最佳实践。**结论：不引入外部 LLM 框架依赖**，但借鉴 LangChain 三个核心设计模式（见下）并用原生 Rust 实现。

---

## Part 1：统一参数体系（核心设计）

### `PipelineStep` 统一参数结构

所有可配置阶段（EXTRACT、ANNOTATE、AGGREGATE、EVALUATE、TRANSFORM）共享同一个结构体：

```rust
// crates/rbrain-engine/src/pipeline.rs

pub struct PipelineStep {
    pub id: String,

    // ─── Input ─────────────────────────────────────────────────
    pub input: InputSpec,

    // ─── LLM Processing ────────────────────────────────────────
    pub prompt: PromptSpec,
    pub output_schema: Option<String>,  // JSON Schema 描述，指导 LLM 输出格式
    pub response_format: ResponseFormat,

    // ─── Output ────────────────────────────────────────────────
    pub output_mode: OutputMode,
    pub link_config: Option<LinkConfig>,

    // ─── Execution Control ─────────────────────────────────────
    pub incremental: bool,   // true = 跳过已有输出的输入页面
    pub batch_size: usize,   // 每次 LLM 调用处理多少输入
    pub max_inputs: Option<usize>,
}
```

### 三个核心枚举（覆盖全部阶段差异）

#### ① `InputSpec`（解决上下文来源差异）

```rust
pub enum InputSpec {
    /// 直接使用输入页面的 compiled_truth（EXTRACT、ANNOTATE、EVALUATE、TRANSFORM）
    SelfContent {
        page_type: String,
        tag: Option<String>,
        language: Option<String>,
        slugs: Option<Vec<String>>,
    },

    /// 以锚点页面（如 concept）为中心，收集其关联来源页面的 chunks（AGGREGATE）
    LinkedSources {
        anchor_page_type: String,     // 锚点类型，如 "concept"
        source_page_type: String,     // 来源类型，如 "note"
        min_sources: usize,           // 至少多少来源才触发处理
        use_chunks: bool,             // true = 从 DB 拉 chunks（高精度），false = compiled_truth
    },
}
```

**为什么这样设计**：`dream_extract` 处理 `note` 页面时直接读 `compiled_truth` → `SelfContent`；`dream_synthesize` 以 `concept` 为锚点、找关联 `note` 的 DB chunks → `LinkedSources`。两者从此共享一个执行路径。

#### ② `OutputMode`（解决输出模式差异）

```rust
pub enum OutputMode {
    /// 结果直接返回调用方（EVALUATE、brain_process 默认）
    Return,

    /// 一对一：每个输入生成一个输出页面（AGGREGATE、ANNOTATE 的部分用法）
    SaveAs {
        page_type: String,
        slug_prefix: String,
        embed: bool,
    },

    /// 一对多：一次 LLM 调用产出多种类型的页面（EXTRACT 独有）
    /// key = JSON 响应中的数组键名 → 对应页面类型配置
    SaveMulti {
        type_map: IndexMap<String, SaveTypeConfig>,
    },

    /// 原地更新：更新输入页面的 frontmatter（ANNOTATE）
    UpdateFrontmatter,
}

pub struct SaveTypeConfig {
    pub page_type: String,
    pub slug_prefix: String,
    pub embed: bool,
}
```

#### ③ `ResponseFormat`（解决响应解析差异）

```rust
pub enum ResponseFormat {
    /// LLM 返回 JSON，按 output_schema 验证，失败时最多 retry 2 次
    Json,
    /// LLM 返回 Markdown 文本，直接使用
    Markdown,
}
```

---

### `PipelineRunner`（统一执行引擎）

```rust
pub struct PipelineRunner<'e> {
    engine: &'e Engine,
    prompt_loader: &'e PromptLoader,
}

impl<'e> PipelineRunner<'e> {
    pub async fn run_step(&self, step: &PipelineStep) -> Result<Vec<serde_json::Value>> {
        // 1. 按 InputSpec 获取处理单元（group = 一个 LLM 调用的输入集合）
        let groups = match &step.input {
            InputSpec::SelfContent { .. } => self.fetch_batched(step).await?,
            InputSpec::LinkedSources { .. } => self.fetch_grouped(step).await?,
        };

        // 2. incremental 过滤（跳过输出已存在且未过期的）
        let groups = if step.incremental {
            self.filter_stale(groups, &step.output_mode).await?
        } else { groups };

        let mut all_results = vec![];
        for group in &groups {
            // 3. 上下文组装
            let context = match &step.input {
                InputSpec::SelfContent { .. } => self.build_plain_context(group).await?,
                InputSpec::LinkedSources { use_chunks, .. } =>
                    self.build_chunk_context(group, *use_chunks).await?,
            };

            // 4. 加载 Prompt
            let system = match &step.prompt {
                PromptSpec::File(name) => self.prompt_loader.load(name),
                PromptSpec::Inline(text) => text.clone(),
            };

            // 5. LLM 调用（含 retry）
            let response = self.llm_with_retry(&system, &context, &step.output_schema).await?;

            // 6. 解析响应
            let items: Vec<serde_json::Value> = match step.response_format {
                ResponseFormat::Json => serde_json::from_str(&clean_json(&response))
                    .unwrap_or_default(),
                ResponseFormat::Markdown => {
                    let normalized = MarkdownParser::normalize_llm_output(&response);
                    vec![serde_json::json!({"content": normalized})]
                }
            };

            // 7. 输出处理
            let results = self.apply_output(&items, group, step).await?;
            all_results.extend(results);
        }
        Ok(all_results)
    }
}
```

**代码量估算**：`PipelineRunner` 约 250 行，替换 `dream_extract`（320行）+ `dream_synthesize`（168行）+ 未来每个新阶段不再需要新代码。

---

## Part 1.5：从 LangChain 借鉴的三个设计模式（纯 Rust 实现）

### 模式 A：`Runnable` Trait（组合单元）

LangChain LCEL 的核心：每个组件都是 `Runnable`，可以用 `|` 串联。我们用 Rust trait 实现：

```rust
// crates/rbrain-engine/src/pipeline.rs

/// 所有 Pipeline 组件的统一接口（借鉴 LangChain Runnable）
#[async_trait]
pub trait Runnable: Send + Sync {
    type Input: Send;
    type Output: Send;

    async fn invoke(&self, input: Self::Input) -> Result<Self::Output>;

    async fn batch(&self, inputs: Vec<Self::Input>) -> Result<Vec<Self::Output>> {
        let mut out = Vec::with_capacity(inputs.len());
        for i in inputs { out.push(self.invoke(i).await?); }
        Ok(out)
    }
}

/// 顺序组合（相当于 LangChain 的 `A | B`）
pub struct Sequential<A, B> { a: A, b: B }

#[async_trait]
impl<A, B> Runnable for Sequential<A, B>
where A: Runnable, B: Runnable<Input = A::Output>,
{
    type Input = A::Input;
    type Output = B::Output;
    async fn invoke(&self, input: A::Input) -> Result<B::Output> {
        self.b.invoke(self.a.invoke(input).await?).await
    }
}
```

`PipelineStep` 自身实现 `Runnable<Input=PageBatch, Output=Vec<Value>>`。多步 Workflow = `Sequential<Step1, Sequential<Step2, Step3>>`。

### 模式 B：`PromptTemplate`（变量替换，借鉴 LangChain PromptTemplate）

Prompt 文件支持 `{variable}` 占位符，运行时由 `PipelineRunner` 填充：

```rust
pub struct PromptTemplate(String);

impl PromptTemplate {
    pub fn from_str(s: &str) -> Self { Self(s.to_string()) }

    /// 替换 {key} 占位符
    pub fn render(&self, vars: &HashMap<&str, &str>) -> String {
        let mut out = self.0.clone();
        for (k, v) in vars {
            out = out.replace(&format!("{{{}}}", k), v);
        }
        out
    }
}
```

**Prompt 文件示例**（`prompts/extract_academic.md`）：
```markdown
你是学术知识提取专家。

已知概念（避免重复）：
{known_concepts}

文章语言：{language}

请从以下文章中提取知识，按此格式返回：
{output_schema}
```

PipelineRunner 在调用前注入：`known_concepts`, `language`, `output_schema`。这样同一 Prompt 文件可以在不同上下文复用。

### 模式 C：`OutputParser` 带 Retry（借鉴 LangChain RetryWithErrorOutputParser）

```rust
/// 带 retry 的输出解析器
pub struct RetryParser<T> {
    max_retries: usize,
    _marker: PhantomData<T>,
}

impl<T: DeserializeOwned> RetryParser<T> {
    pub async fn parse(
        &self,
        response: &str,
        schema_hint: &str,
        llm: &dyn LlmClient,
        system: &str,
        original_context: &str,
    ) -> Result<T> {
        // 1. 尝试直接解析
        if let Ok(v) = serde_json::from_str::<T>(&clean_json(response)) {
            return Ok(v);
        }
        // 2. Retry：将错误信息反馈给 LLM
        for attempt in 1..=self.max_retries {
            let fix_prompt = format!(
                "你上次返回的 JSON 解析失败。Schema 要求：{}\n\
                 你的上次返回：{}\n\
                 请重新返回合法 JSON。",
                schema_hint, response
            );
            let fixed = llm.chat(system, &fix_prompt).await?;
            if let Ok(v) = serde_json::from_str::<T>(&clean_json(&fixed)) {
                return Ok(v);
            }
            eprintln!("  retry {}/{} failed", attempt, self.max_retries);
        }
        Err(anyhow!("failed to parse LLM output after {} retries", self.max_retries))
    }
}
```

这三个模式直接整合进 `PipelineRunner`：`PromptTemplate` 渲染 system prompt，`RetryParser` 处理 JSON 解析失败，`Runnable` trait 支持多步 Workflow 的类型安全组合。

---

## Part 2：7 个阶段全部映射到统一参数体系

### 阶段全景图

| # | 阶段 | 驱动方式 | 使用 PipelineRunner? | 核心差异点 |
|---|------|---------|---------------------|-----------|
| 1 | INGEST | `rbrain sync + embed`，无 LLM | ❌ 独立基础设施 | 文件系统→DB，向量索引 |
| 2 | EXTRACT | 遍历页面，LLM | ✅ SelfContent + SaveMulti | 一对多输出，JSON 解析 |
| 3 | ANNOTATE | 遍历页面，LLM | ✅ SelfContent + UpdateFrontmatter | 更新原页面 frontmatter |
| 4 | AGGREGATE | 以锚点聚合关联来源，LLM | ✅ LinkedSources + SaveAs | 多对一，Chunked 上下文 |
| 5 | EVALUATE | 遍历页面，LLM | ✅ SelfContent + Return | 结果返回调用方，不写入 KB |
| 6 | THINK | 检索驱动，LLM | ❌ 独立方法（搜索不遍历页面） | 动态检索，response_schema 可配置 |
| 7 | COMPOSE | 检索驱动，LLM | ❌ 独立方法（搜索不遍历页面） | 生成最终产品，template 可配置 |

**INGEST、THINK、COMPOSE 不走 PipelineRunner**（前者是基础设施，后两者是检索驱动而非页面遍历），但都通过 PromptTemplate + PromptLoader 获得外置 Prompt 配置能力。

---

### EXTRACT（`dream_extract` 重构后）

```rust
PipelineStep {
    id: "extract",
    input: InputSpec::SelfContent { page_type: "note", .. },
    prompt: PromptSpec::File("extract_academic"),
    response_format: ResponseFormat::Json,
    output_schema: Some(EXTRACT_SCHEMA),  // concepts[]/figures[]/events[]
    output_mode: OutputMode::SaveMulti {
        type_map: {
            "concepts" → SaveTypeConfig { page_type: "concept",  slug_prefix: "research/concepts/", embed: false },
            "figures"  → SaveTypeConfig { page_type: "figure",   slug_prefix: "research/figures/",  embed: false },
            "events"   → SaveTypeConfig { page_type: "evidence", slug_prefix: "research/evidence/", embed: false },
        }
    },
    incremental: true,
    batch_size: 1,
}
```

### AGGREGATE（`dream_synthesize` 重构后）

```rust
PipelineStep {
    id: "synthesize",
    input: InputSpec::LinkedSources {
        anchor_page_type: "concept",
        source_page_type: "note",
        min_sources: 3,
        use_chunks: true,
    },
    prompt: PromptSpec::File("synthesize_academic"),
    response_format: ResponseFormat::Markdown,
    output_schema: None,
    output_mode: OutputMode::SaveAs {
        page_type: "synthesis",
        slug_prefix: "research/synthesis/",
        embed: true,
    },
    incremental: true,
    batch_size: 1,
}
```

### ANNOTATE（新阶段，零代码变化）

```rust
PipelineStep {
    id: "annotate_theme",
    input: InputSpec::SelfContent { page_type: "passage", .. },
    prompt: PromptSpec::File("annotate_policy_theme"),
    response_format: ResponseFormat::Json,
    output_schema: Some(r#"{"theme":"string","importance":"high|medium|low"}"#),
    output_mode: OutputMode::UpdateFrontmatter,
    incremental: true,
    batch_size: 5,
}
```

### EVALUATE（新阶段，零代码变化）

```rust
PipelineStep {
    id: "evaluate_coverage",
    input: InputSpec::SelfContent { page_type: "synthesis", .. },
    prompt: PromptSpec::File("evaluate_coverage"),
    response_format: ResponseFormat::Markdown,
    output_schema: None,
    output_mode: OutputMode::Return,
    incremental: false,
    batch_size: 10,
}
```

### TRANSFORM / brain_process（新阶段，零代码变化）

```rust
// 参数完全来自调用方：
PipelineStep {
    id: "ad_hoc_transform",
    input: from_args.input_filter,
    prompt: PromptSpec::Inline(from_args.task_prompt),
    response_format: ResponseFormat::Json,
    output_schema: from_args.output_schema,
    output_mode: from_args.output_mode,
    incremental: false,
    batch_size: from_args.batch_size,
}
```

### THINK（独立方法，Prompt 外置 + response_schema 可配置）

不走 PipelineRunner（搜索驱动，不遍历页面），但通过 PromptLoader + response_schema 参数化：

```rust
// brain_think MCP 工具接受可选 response_schema
pub struct ThinkArgs {
    pub topic: String,
    pub response_schema: Option<String>,  // 留空 = 用 think_cjk.md 默认格式
}

// 内部调用 PromptLoader
let system = PromptTemplate::from_str(&self.inner.prompts.load("think_cjk"))
    .render(&hashmap! {
        "response_schema" => response_schema.as_deref().unwrap_or(DEFAULT_THINK_SCHEMA),
    });
```

**不同任务的 response_schema 实例**：
- 文献综述：`None`（使用 think_cjk.md 的「核心观点/张力/开放问题」格式）
- 考题分析：`'{"混淆点":"string","辨析角度":"string","典型错误":"string"}'`
- 论文写作：`'{"核心论点":"string","最强反驳":"string","回应策略":"string"}'`

### COMPOSE（独立方法，template 可配置）

```rust
// brain_generate MCP 工具接受可选 template
pub struct GenerateArgs {
    pub topic: String,
    pub template: Option<String>,  // → prompts/compose_{template}.md
}

let system = self.inner.prompts.load(
    &format!("compose_{}", template.as_deref().unwrap_or("review_article"))
);
```

**不同任务的 template 实例**：
- 文献综述：`"review_article"` → `prompts/compose_review_article.md`
- 政策简报：`"policy_brief"` → `prompts/compose_policy_brief.md`（用户自建）
- 论文章节：`"thesis_section"` → `prompts/compose_thesis_section.md`（用户自建）

---

## Part 3：TOML 配置中的统一参数

Profile 文件（控制 `dream` 流水线）和 Workflow 文件（多步任务）**使用完全相同的参数键**：

### 通用 Stage 配置格式（所有阶段共享）

```toml
# 每个阶段都是一个 [[stage]] 块，参数完全标准化
[[stages]]
id      = "my_stage"          # 阶段唯一 ID
enabled = true                # 是否启用

# ── Input ──────────────────────────────────────────────────────────
input_mode = "self"           # "self" | "linked_sources"
page_type  = "note"           # 输入页面类型（self 模式）
tag        = ""               # 可选：按 tag 过滤
language   = ""               # 可选：按语言过滤

# linked_sources 模式额外参数：
anchor_type   = "concept"     # 锚点页面类型
source_type   = "note"        # 来源页面类型
min_sources   = 3             # 最少来源数量
use_chunks    = true          # 使用 DB chunks（高精度）还是 compiled_truth

# ── Processing ─────────────────────────────────────────────────────
prompt          = "extract_academic"   # → prompts/{name}.md 文件名
response_format = "json"               # "json" | "markdown"
output_schema   = """                  # 可选：JSON Schema 描述
  {"concepts":[{"name":"string","description":"string"}]}
"""

# ── Output ─────────────────────────────────────────────────────────
output_mode = "save_multi"    # "return" | "save" | "save_multi" | "update_frontmatter"

# save 模式：
output_page_type   = "synthesis"
output_slug_prefix = "research/synthesis/"
embed_output       = true

# save_multi 模式：
[stages.type_map.concepts]    # JSON 响应键 → 输出配置
page_type   = "concept"
slug_prefix = "research/concepts/"
embed       = false

[stages.type_map.figures]
page_type   = "figure"
slug_prefix = "research/figures/"
embed       = false

# ── Execution ──────────────────────────────────────────────────────
incremental = true
batch_size  = 1
max_inputs  = 0               # 0 = 不限制
```

### 完整 Profile 示例（文献综述）

```toml
# RBRAIN_HOME/profiles/literature_review.toml

[profile]
name = "literature_review"

[[stages]]
id = "extract"
enabled = true
input_mode = "self"
page_type  = "note"
prompt          = "extract_academic"
response_format = "json"
output_mode = "save_multi"
incremental = true
batch_size  = 1

[stages.type_map.concepts]
page_type = "concept"; slug_prefix = "research/concepts/"; embed = false

[stages.type_map.figures]
page_type = "figure";  slug_prefix = "research/figures/";  embed = false

[stages.type_map.events]
page_type = "evidence"; slug_prefix = "research/evidence/"; embed = false

[[stages]]
id = "synthesize"
enabled = true
input_mode    = "linked_sources"
anchor_type   = "concept"
source_type   = "note"
min_sources   = 3
use_chunks    = true
prompt          = "synthesize_academic"
response_format = "markdown"
output_mode        = "save"
output_page_type   = "synthesis"
output_slug_prefix = "research/synthesis/"
embed_output       = true
incremental = true
batch_size  = 1
```

### 自定义阶段（研究者新增，无需改代码）

```toml
# 在任何 Profile 文件中追加此块即可新增阶段：
[[stages]]
id = "annotate_cognitive_level"
enabled = true
input_mode = "self"
page_type  = "passage"
prompt          = "annotate_cognitive_level"   # 研究者创建 prompts/annotate_cognitive_level.md
response_format = "json"
output_schema   = '{"cognitive_level":"识记|理解|运用","difficulty":"easy|medium|hard"}'
output_mode     = "update_frontmatter"
incremental = true
batch_size  = 10
```

---

## Part 4：5 个学术任务的 Pipeline 实例

### 文献综述

```
[INGEST] → [EXTRACT: concept+figure+evidence] → [AGGREGATE: concept→synthesis, min=3]
         → THINK("核心议题") → COMPOSE("综述文章")
```

### 政策分析 + 考题

```
[INGEST, meta=false] → [EXTRACT: passage] → [ANNOTATE: theme+importance] 
→ [AGGREGATE: passage→policy_synthesis, min=5]
[by agent] → brain_process({passages, generate_questions_prompt, return})
```

### 质性研究报告

```
[INGEST] → [EXTRACT: theme+quote+memo] → [ANNOTATE: axial_coding]
→ [AGGREGATE: theme→theoretical_category, min=3]
→ [EVALUATE: saturation_check, return] → THINK("核心范畴") → COMPOSE("研究报告")
```

### 元分析（定量文献）

```
[INGEST] → [EXTRACT: variable+hypothesis+finding] → [ANNOTATE: effect_size_category]
→ [AGGREGATE: finding→meta_finding, min=5]
→ [EVALUATE: heterogeneity, return] → COMPOSE("元分析报告")
```

### 论文写作辅助

```
[INGEST] → [EXTRACT: claim+evidence+counterargument] → [ANNOTATE: evidence_strength]
→ [AGGREGATE: claim→argument_map, min=3] → [EVALUATE: argument_gaps, return]
→ THINK("最强反驳") → COMPOSE("论文章节草稿")
```

---

## Part 5：THINK 和 COMPOSE 的参数化

THINK（`brain_think`）和 COMPOSE（`brain_generate`）不走 PipelineRunner（它们是搜索驱动，不遍历页面），但同样通过参数配置化：

```rust
// brain_think 增加可选 response_schema：
pub struct ThinkArgs {
    pub topic: String,
    pub response_schema: Option<String>,  // 留空=用 prompt 文件的默认格式
}
// → 不同任务传不同 schema:
// 文献综述: None（用 think_cjk.md 的默认格式：核心观点/张力/开放问题）
// 考题分析: '{"混淆点":"string","辨析角度":"string","典型错误":"string"}'
// 论文写作: '{"核心论点":"string","最强反驳":"string","回应策略":"string"}'

// brain_generate 增加可选 template：
pub struct GenerateArgs {
    pub topic: String,
    pub template: Option<String>,  // → prompts/compose_{template}.md
}
// 文献综述: template="review_article"
// 政策简报: template="policy_brief"
// 论文章节: template="thesis_section"
```

---

## Part 6：实现规划

### 新文件

| 文件 | 内容 |
|------|------|
| `crates/rbrain-engine/src/pipeline.rs` | `PipelineStep`, `PipelineRunner`, 所有枚举定义 |
| `crates/rbrain-core/src/prompt_loader.rs` | `PromptLoader`（file + builtin fallback） |
| `crates/rbrain-core/src/pipeline_config.rs` | Profile/Workflow TOML 反序列化结构 |
| `crates/rbrain-engine/prompts/*.md` | 9 个 Prompt 文件（include_str! 内嵌默认） |
| `crates/rbrain-engine/profiles/literature_review.toml` | 内嵌默认 Profile |

### 修改文件

| 文件 | 改动 |
|------|------|
| `crates/rbrain-core/src/config.rs` | 加 `prompts_dir`, `profiles_dir` |
| `crates/rbrain-engine/src/engine.rs` | `dream_extract/synthesize` 重构为调用 `PipelineRunner`；加 `process_pages()` |
| `crates/rbrain-mcp/src/lib.rs` | 新增 `brain_process`；`brain_think` 加 `response_schema`；`brain_generate` 加 `template` |
| `crates/rbrain-cli/src/main.rs` | `dream --profile`；`workflow run` 子命令 |

### 实施顺序

```
Phase 1: PromptLoader + Prompt 文件外置（1-2天）← 基础，无破坏性
Phase 2: PipelineStep + PipelineRunner（2-3天）  ← 核心抽象
Phase 3: dream_extract/synthesize 重构（1-2天） ← 使用 PipelineRunner 重写
Phase 4: Profile/Workflow TOML 系统（1-2天）    ← 配置层
Phase 5: brain_process MCP 工具（1天）          ← 对外接口
Phase 6: 新 Prompt 文件质量改进（1天）          ← 随时可做
```

**总计：7-11天工作量**

---

## 验证方法

```bash
# Phase 1: Prompt 外置
cargo test -p rbrain-core -- prompt_loader
echo "3句话回答" > $RBRAIN_HOME/prompts/think_cjk.md
rbrain think "测试"

# Phase 2-3: PipelineRunner + 重构验证（功能不变）
RBRAIN_HOME=/path/to/test-kb rbrain dream --stage extract
# 对比重构前后结果是否一致

# Phase 4: Profile 系统
rbrain dream --profile policy_analysis --stage extract
rbrain list --type passage --limit 5

# Phase 5: brain_process
# brain_process({input_filter:{page_type:"note",limit:3},
#   task_prompt:"提取主题", output_schema:'{"topic":"string"}', output_mode:"return"})

# 集成测试
cargo test -p rbrain-engine -- pipeline
cargo test -p rbrain-engine -- dream  # 确保重构后行为一致
```
