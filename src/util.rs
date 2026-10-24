//! Small shared helpers: tokens, a light stemmer, sentence splitting, hashing, time.

use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

/// Lowercase words made of letters and digits. "F1-score" -> ["f1", "score"].
pub fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for c in ch.to_lowercase() { cur.push(c); }
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() { out.push(cur); }
    out
}

/// A deliberately small stemmer: enough for "normalised", "normalization" and
/// "normalize" to meet, without pulling in a full NLP library.
pub fn stem(w: &str) -> String {
    let mut s = w.to_lowercase();
    if s.len() <= 3 || s.chars().any(|c| c.is_ascii_digit()) { return s; }
    if s.ends_with("ies") && s.len() > 4 { s.truncate(s.len() - 3); s.push('y'); return s; }
    for suf in ["izations", "isations", "ization", "isation", "ations", "ation", "ings", "ing", "edly", "ed", "es"] {
        if s.ends_with(suf) && s.len() - suf.len() >= 3 { s.truncate(s.len() - suf.len()); break; }
    }
    let mut cut = false;
    for suf in ["iz", "is"] {
        if s.ends_with(suf) && s.len() - suf.len() >= 4 { s.truncate(s.len() - suf.len()); cut = true; break; }
    }
    if !cut && s.ends_with('s') && !s.ends_with("ss") && s.len() > 3 { s.pop(); }
    if s.ends_with('e') && s.len() > 3 { s.pop(); }
    for suf in ["iz", "is"] {
        if s.ends_with(suf) && s.len() - suf.len() >= 4 { s.truncate(s.len() - suf.len()); break; }
    }
    s
}

pub fn stems(text: &str) -> Vec<String> {
    words(text).iter().map(|w| stem(w)).collect()
}

const STOP: &[&str] = &[
    "the", "a", "an", "of", "and", "or", "to", "in", "on", "for", "with", "by", "is", "are", "was", "were", "be",
    "this", "that", "we", "our", "it", "as", "at", "from", "which", "these", "those", "using", "use", "used", "into",
    "can", "how", "what", "do", "does", "my", "i", "me", "you", "your", "not", "than", "then", "also",
];

/// Search tokens: stemmed words without filler.
pub fn search_tokens(text: &str) -> Vec<String> {
    words(text).into_iter().filter(|w| !STOP.contains(&w.as_str())).map(|w| stem(&w)).collect()
}

/// Split prose into sentences without breaking on "e.g.", "et al.", "Fig. 3" or "0.92".
pub fn sentences(text: &str) -> Vec<String> {
    const ABBR: &[&str] = &["e.g", "i.e", "al", "fig", "eq", "vs", "etc", "approx", "no", "sec", "tab", "ref", "cf", "resp"];
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '.' || c == '!' || c == '?' {
            let next_space = chars.get(i + 1).map(|n| n.is_whitespace()).unwrap_or(true);
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() { j += 1; }
            let next_start = chars.get(j).map(|n| n.is_uppercase() || n.is_ascii_digit() || *n == '(' || *n == '[').unwrap_or(true);
            let word: String = chars[start..i].iter().rev().take_while(|c| !c.is_whitespace()).collect::<Vec<_>>().into_iter().rev().collect();
            let wl = word.to_lowercase().trim_start_matches('(').to_string();
            let is_abbr = c == '.' && ABBR.iter().any(|a| wl == *a || wl.ends_with(&format!(" {}", a)));
            if next_space && next_start && !is_abbr {
                let s: String = chars[start..=i].iter().collect();
                let s = clean_space(&s);
                if s.len() > 2 { out.push(s); }
                start = j;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    if start < chars.len() {
        let s = clean_space(&chars[start..].iter().collect::<String>());
        if s.len() > 2 { out.push(s); }
    }
    out
}

pub fn clean_space(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn word_count(s: &str) -> usize {
    s.split_whitespace().count()
}

/// Rough token estimate used for prompt budgets: 4 bytes per token.
pub fn tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

pub fn slug(s: &str, max: usize) -> String {
    let w = words(s);
    let mut out = String::new();
    for p in w {
        if out.len() + p.len() + 1 > max { break; }
        if !out.is_empty() { out.push('-'); }
        out.push_str(&p);
    }
    if out.is_empty() { "paper".into() } else { out }
}

pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => { out.push(b' '); i += 1; }
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) { out.push(v); i += 3; } else { out.push(b'%'); i += 1; }
            }
            c => { out.push(c); i += 1; }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { return s.to_string(); }
    let mut t: String = s.chars().take(n).collect();
    t.push('…');
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stems_meet() {
        assert_eq!(stem("normalization"), stem("normalized"));
        assert_eq!(stem("normalize"), stem("normalised"));
        assert_eq!(stem("encoding"), stem("encoded"));
        assert_eq!(stem("imputation"), stem("imputed"));
        assert_eq!(stem("features"), stem("feature"));
    }
    #[test]
    fn sentence_split_keeps_abbreviations() {
        let s = sentences("We follow Smith et al. in using SMOTE. Accuracy was 0.92 on the test set. See Fig. 3 for details.");
        assert_eq!(s.len(), 3, "{:?}", s);
    }
    #[test]
    fn decode() {
        assert_eq!(url_decode("a%20b+c"), "a b c");
    }
}
