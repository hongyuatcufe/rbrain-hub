//! Token-budget aware context packing for LLM prompts (M3 Slice 5).
//!
//! Char-based truncation is unreliable across languages — a 2,500-char
//! Chinese block is ~2,500 tokens, but the same length of English is only
//! ~625 tokens. The aggregate/linked-sources packers used to take "first
//! N chars per page" without regard for the language, which either
//! overflowed DeepSeek's 64K context window on Chinese corpora or wasted
//! capacity on English ones.
//!
//! This module gives the pipeline two primitives:
//!
//! - [`estimate_tokens`] — a conservative heuristic estimator (CJK char
//!   = 1 token, ASCII char = 0.25, other = 0.5). Slightly overestimates
//!   so callers stay under model limits.
//! - [`pack_within_budget`] — greedy first-fit packer over a Vec of items
//!   with a per-item token cost function. Always includes the first item
//!   even if it overflows alone (callers should pre-truncate giants).
//!
//! The estimator is intentionally not tokenizer-exact. We trade ±20%
//! accuracy for zero new dependencies. Profiles that need precision can
//! lower their declared budget to compensate.

/// Estimate the token cost of `text` using a CJK-vs-ASCII heuristic.
///
/// - CJK characters (Hanzi, Hiragana, Katakana, Hangul): 1 token each.
/// - ASCII characters: 0.25 token each (matches GPT BPE roughly).
/// - Other Unicode (emoji, Cyrillic, etc.): 0.5 token each.
///
/// Returns the ceiling of the sum so a 1-char ASCII string costs 1 token,
/// not 0.
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    let mut total: f64 = 0.0;
    for c in text.chars() {
        if is_cjk(c) {
            total += 1.0;
        } else if c.is_ascii() {
            total += 0.25;
        } else {
            total += 0.5;
        }
    }
    total.ceil() as usize
}

/// True if `c` is in a CJK script range that counts as ~1 token under
/// most BPE/SentencePiece tokenizers used by Claude / GPT / DeepSeek.
fn is_cjk(c: char) -> bool {
    matches!(c,
        // CJK Unified Ideographs + Extension A/B
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{20000}'..='\u{2A6DF}'
            // Hiragana / Katakana
            | '\u{3040}'..='\u{309F}'
            | '\u{30A0}'..='\u{30FF}'
            // Hangul Syllables
            | '\u{AC00}'..='\u{D7AF}'
            // CJK Symbols & Punctuation (full-width, etc.)
            | '\u{3000}'..='\u{303F}'
            | '\u{FF00}'..='\u{FFEF}'
    )
}

/// Greedy first-fit packer. Walks `items` in order, summing `count_fn`
/// for each, and stops when adding the next item would exceed `budget`.
/// `budget = 0` disables the cap (returns all items).
///
/// Always includes the first item even if it alone exceeds `budget`;
/// otherwise the function could return an empty Vec from a single
/// oversized item. Callers that want strict no-overflow semantics should
/// pre-truncate items to fit the budget.
#[must_use]
pub fn pack_within_budget<T, F>(items: Vec<T>, budget: usize, count_fn: F) -> Vec<T>
where
    F: Fn(&T) -> usize,
{
    if budget == 0 {
        return items;
    }
    let mut packed = Vec::with_capacity(items.len());
    let mut used: usize = 0;
    for item in items {
        let cost = count_fn(&item);
        if used + cost > budget && !packed.is_empty() {
            break;
        }
        used += cost;
        packed.push(item);
    }
    packed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_english_quarter_ratio() {
        // 100 ASCII chars → 25 tokens (ceil).
        let text = "a".repeat(100);
        assert_eq!(estimate_tokens(&text), 25);
    }

    #[test]
    fn estimate_tokens_cjk_one_per_char() {
        // 50 CJK chars → 50 tokens.
        let text = "中".repeat(50);
        assert_eq!(estimate_tokens(&text), 50);
    }

    #[test]
    fn estimate_tokens_mixed_content() {
        // 4 CJK (4.0) + 8 ASCII (2.0) = 6.0 → 6.
        let text = "中文测试 hello!!";
        assert_eq!(estimate_tokens(text), 6);
    }

    #[test]
    fn estimate_tokens_empty_is_zero() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_full_width_punctuation_is_cjk() {
        // Full-width comma is CJK punctuation → 1 token per char.
        let text = "，。！？";
        assert_eq!(estimate_tokens(text), 4);
    }

    #[test]
    fn pack_within_budget_stops_at_budget() {
        // Each item costs 10 tokens. Budget 25 → 2 items fit, 3rd would push to 30.
        let items: Vec<usize> = vec![1, 2, 3, 4];
        let packed = pack_within_budget(items, 25, |_| 10);
        assert_eq!(packed.len(), 2);
        assert_eq!(packed, vec![1, 2]);
    }

    #[test]
    fn pack_within_budget_always_includes_oversized_first() {
        // First item costs 100, budget is 50 — must still include it,
        // else single-large-item callers would get an empty result.
        let items: Vec<usize> = vec![100, 5];
        let packed = pack_within_budget(items, 50, |&n| n);
        assert_eq!(packed, vec![100]);
    }

    #[test]
    fn pack_within_budget_zero_means_unlimited() {
        let items: Vec<i32> = vec![1, 2, 3, 4, 5];
        let packed = pack_within_budget(items.clone(), 0, |_| 1000);
        assert_eq!(packed, items);
    }

    #[test]
    fn pack_within_budget_exact_fit_keeps_all() {
        // 5 items × 10 cost = 50, budget = 50 → all fit.
        let items: Vec<usize> = (0..5).collect();
        let packed = pack_within_budget(items.clone(), 50, |_| 10);
        assert_eq!(packed.len(), 5);
    }

    #[test]
    fn pack_within_budget_empty_input_yields_empty_output() {
        let items: Vec<i32> = vec![];
        let packed = pack_within_budget(items, 100, |_| 5);
        assert!(packed.is_empty());
    }
}
