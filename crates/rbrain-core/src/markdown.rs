use gray_matter::{Matter, engine::YAML};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Serialize, Deserialize)]
pub struct ParseResult {
    pub frontmatter: Value,
    pub compiled_truth: String,
    pub timeline: String,
}

#[derive(Debug)]
pub struct MarkdownParser;

impl MarkdownParser {
    pub fn parse(content: &str) -> ParseResult {
        let matter = Matter::<YAML>::new();
        let result = matter.parse(content);

        let (frontmatter, body) = match result {
            Ok(parsed) => (parsed.data.unwrap_or_default(), parsed.content),
            Err(_) => (Value::Object(serde_json::Map::new()), content.to_string()),
        };
        let (compiled_truth, timeline) = Self::split_body(&body);

        ParseResult {
            frontmatter,
            compiled_truth,
            timeline,
        }
    }

    fn split_body(body: &str) -> (String, String) {
        match body.rfind("\n---\n") {
            Some(pos) => {
                let timeline = body[pos + 5..].trim().to_string();
                if timeline.is_empty() {
                    // trailing `---` with nothing after it — treat as part of body, no timeline
                    (body.trim().to_string(), String::new())
                } else {
                    let truth = body[..pos].trim().to_string();
                    (truth, timeline)
                }
            }
            None => (body.trim().to_string(), String::new()),
        }
    }

    pub fn to_canonical(frontmatter: &Value, compiled_truth: &str, timeline: &str) -> String {
        let sorted_fm = Self::sort_frontmatter_key(frontmatter);
        let fm_str = serde_json::to_string(&sorted_fm).unwrap_or_default();
        if timeline.trim().is_empty() {
            format!("---\n{}\n---\n{}\n", fm_str, compiled_truth.trim())
        } else {
            // The `\n---\n` separator is required so split_body can find and extract the
            // timeline section when the file is re-read by sync or put.
            format!("---\n{}\n---\n{}\n\n---\n{}\n", fm_str, compiled_truth.trim(), timeline.trim())
        }
    }

    pub fn content_hash(canonical: &str) -> String {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;

        let normalized = canonical.nfc().collect::<String>();
        let mut hasher = DefaultHasher::new();
        normalized.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    fn sort_frontmatter_key(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut sorted: Vec<_> = map.iter().collect();
                sorted.sort_by_key(|(k, _)| *k);
                let mut new_map = serde_json::Map::new();
                for (k, v) in sorted {
                    new_map.insert(k.clone(), Self::sort_frontmatter_key(v));
                }
                Value::Object(new_map)
            }
            _ => value.clone(),
        }
    }

    pub fn normalize_slug(slug: &str) -> String {
        slug.nfc().collect::<String>()
    }

    /// Extract a plain-text snippet from compiled_truth for display in list views.
    /// Skips H1, code blocks, horizontal rules, and headings.
    /// Strips [[wikilinks]] and **bold** markers. Truncates to max_len chars.
    pub fn extract_snippet(compiled_truth: &str, max_len: usize) -> String {
        let mut result = String::new();
        let mut in_code_block = false;

        for line in compiled_truth.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                in_code_block = !in_code_block;
                continue;
            }
            if in_code_block { continue; }
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" { continue; }

            let stripped = Self::strip_wikilinks_for_snippet(trimmed);
            let stripped = stripped.replace("**", "").replace('`', "");
            let stripped = stripped.trim().to_string();
            if stripped.is_empty() { continue; }

            if !result.is_empty() { result.push(' '); }
            result.push_str(&stripped);

            if result.chars().count() >= max_len {
                let cutoff = result.char_indices().nth(max_len)
                    .map(|(i, _)| i)
                    .unwrap_or(result.len());
                result.truncate(cutoff);
                result.push('…');
                break;
            }
        }
        result
    }

    fn strip_wikilinks_for_snippet(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '[' && chars.peek() == Some(&'[') {
                chars.next();
                let mut inner = String::new();
                loop {
                    match chars.next() {
                        None => break,
                        Some(']') if chars.peek() == Some(&']') => { chars.next(); break; }
                        Some(ch) => inner.push(ch),
                    }
                }
                if let Some(pipe_pos) = inner.find('|') {
                    let display = &inner[pipe_pos + 1..];
                    // Skip chunk:N style references — not human-readable
                    if !display.trim_start().starts_with("chunk:") {
                        out.push_str(display);
                    }
                } else {
                    out.push_str(&inner);
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Normalize LLM-generated Markdown output:
    /// - `* ` bullets → `- ` (line-start only, code blocks excluded)
    /// - Remove blank lines between adjacent list items (tight lists)
    /// - Collapse 3+ consecutive blank lines to one
    /// - Strip trailing whitespace per line
    pub fn normalize_llm_output(text: &str) -> String {
        let mut processed: Vec<String> = Vec::new();
        let mut in_code_block = false;

        for line in text.lines() {
            let trimmed_end = line.trim_end();
            if trimmed_end.trim_start().starts_with("```") {
                in_code_block = !in_code_block;
                processed.push(trimmed_end.to_string());
                continue;
            }
            if in_code_block {
                processed.push(trimmed_end.to_string());
                continue;
            }
            let normalized = if trimmed_end.starts_with("* ") {
                format!("- {}", &trimmed_end[2..])
            } else {
                trimmed_end.to_string()
            };
            processed.push(normalized);
        }

        // Remove blank lines between adjacent list items (tight lists)
        let mut tight: Vec<String> = Vec::with_capacity(processed.len());
        let n = processed.len();
        for i in 0..n {
            if processed[i].trim().is_empty()
                && i > 0
                && i + 1 < n
                && Self::is_list_item(processed[i - 1].trim())
                && Self::is_list_item(processed[i + 1].trim())
            {
                continue;
            }
            tight.push(processed[i].clone());
        }

        // Collapse 3+ blank lines to one
        let mut output = String::new();
        let mut blank_run = 0usize;
        for line in &tight {
            if line.trim().is_empty() {
                blank_run += 1;
                if blank_run <= 1 { output.push('\n'); }
            } else {
                blank_run = 0;
                output.push_str(line);
                output.push('\n');
            }
        }
        output.trim_end().to_string()
    }

    fn is_list_item(s: &str) -> bool {
        s.starts_with("- ") || s.starts_with("* ") || s.starts_with("+ ")
            || (s.len() > 2
                && s.chars().next().map_or(false, |c| c.is_ascii_digit())
                && s[1..].starts_with(". "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_with_frontmatter_and_divider() {
        let content = "---\ntype: concept\ntitle: Test\n---\ntruth content\n\n---\n\ntimeline content";
        let result = MarkdownParser::parse(content);
        assert_eq!(result.compiled_truth, "truth content");
        assert_eq!(result.timeline, "timeline content");
    }

    #[test]
    fn test_to_canonical_round_trip_with_timeline() {
        let fm = serde_json::json!({"type": "note", "title": "T"});
        let ct = "body content";
        let tl = "- 2024-01: event [Source: raw/foo]";
        let canonical = MarkdownParser::to_canonical(&fm, ct, tl);
        // canonical must contain \n---\n so split_body can recover timeline
        let parsed = MarkdownParser::parse(&canonical);
        assert_eq!(parsed.compiled_truth, ct);
        assert_eq!(parsed.timeline, tl);
    }

    #[test]
    fn test_to_canonical_no_timeline() {
        let fm = serde_json::json!({"type": "note"});
        let canonical = MarkdownParser::to_canonical(&fm, "body", "");
        let parsed = MarkdownParser::parse(&canonical);
        assert_eq!(parsed.compiled_truth, "body");
        assert_eq!(parsed.timeline, "");
    }

    #[test]
    fn test_parse_without_divider() {
        let result = MarkdownParser::parse("just content");
        assert_eq!(result.compiled_truth, "just content");
        assert_eq!(result.timeline, "");
    }

    // ── extract_snippet tests ────────────────────────────────────────────────

    #[test]
    fn test_extract_snippet_skips_h1_and_returns_first_body_line() {
        let ct = "# My Title\n\nThis is the first sentence of the body.";
        let s = MarkdownParser::extract_snippet(ct, 160);
        assert_eq!(s, "This is the first sentence of the body.");
    }

    #[test]
    fn test_extract_snippet_strips_wikilink_pipe() {
        let ct = "See [[research/concepts/foo|知识体系]] for details.";
        let s = MarkdownParser::extract_snippet(ct, 160);
        assert_eq!(s, "See 知识体系 for details.");
    }

    #[test]
    fn test_extract_snippet_strips_wikilink_no_pipe() {
        let ct = "References [[research/concepts/foo]] and [[bar]].";
        let s = MarkdownParser::extract_snippet(ct, 160);
        assert_eq!(s, "References research/concepts/foo and bar.");
    }

    #[test]
    fn test_extract_snippet_skips_chunk_ref() {
        let ct = "See [[raw/article|chunk:42]] for evidence.";
        let s = MarkdownParser::extract_snippet(ct, 160);
        // chunk:N display is skipped
        assert_eq!(s, "See  for evidence.");
    }

    #[test]
    fn test_extract_snippet_skips_code_block() {
        let ct = "Before code.\n```rust\n* code line\n```\nAfter code.";
        let s = MarkdownParser::extract_snippet(ct, 160);
        assert_eq!(s, "Before code. After code.");
    }

    #[test]
    fn test_extract_snippet_truncates_at_char_boundary() {
        let ct = "中文内容测试字符边界截断不应破坏多字节字符序列。";
        let s = MarkdownParser::extract_snippet(ct, 5);
        // Should be exactly 5 CJK chars + ellipsis
        assert!(s.ends_with('…'));
        let char_count = s.chars().count();
        assert_eq!(char_count, 6); // 5 chars + ellipsis
    }

    #[test]
    fn test_extract_snippet_empty_returns_empty() {
        assert_eq!(MarkdownParser::extract_snippet("", 160), "");
        assert_eq!(MarkdownParser::extract_snippet("# Only heading", 160), "");
        assert_eq!(MarkdownParser::extract_snippet("---", 160), "");
    }

    // ── normalize_llm_output tests ───────────────────────────────────────────

    #[test]
    fn test_normalize_bullet_star_to_dash() {
        let input = "* item one\n* item two\n* item three";
        let output = MarkdownParser::normalize_llm_output(input);
        assert_eq!(output, "- item one\n- item two\n- item three");
    }

    #[test]
    fn test_normalize_tight_list_removes_blank_between_items() {
        let input = "- item one\n\n- item two\n\n- item three";
        let output = MarkdownParser::normalize_llm_output(input);
        assert_eq!(output, "- item one\n- item two\n- item three");
    }

    #[test]
    fn test_normalize_keeps_blank_between_paragraphs() {
        let input = "First paragraph.\n\nSecond paragraph.";
        let output = MarkdownParser::normalize_llm_output(input);
        assert_eq!(output, "First paragraph.\n\nSecond paragraph.");
    }

    #[test]
    fn test_normalize_collapses_triple_blank() {
        let input = "Para one.\n\n\n\nPara two.";
        let output = MarkdownParser::normalize_llm_output(input);
        assert_eq!(output, "Para one.\n\nPara two.");
    }

    #[test]
    fn test_normalize_strips_trailing_spaces() {
        let input = "line with spaces   \nclean line";
        let output = MarkdownParser::normalize_llm_output(input);
        assert_eq!(output, "line with spaces\nclean line");
    }

    #[test]
    fn test_normalize_preserves_code_block_content() {
        let input = "Before:\n```\n* not a bullet\n```\nAfter.";
        let output = MarkdownParser::normalize_llm_output(input);
        // Code block content must not be modified
        assert!(output.contains("* not a bullet"));
        assert!(output.contains("Before:"));
        assert!(output.contains("After."));
    }
}
