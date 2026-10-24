//! Keyword search (BM25) over the key passages of every paper card, with a boost
//! for passages that mention the same catalog techniques as the question.
//! Embeddings are planned as an addition; BM25 + techniques already handles
//! technical terms well at this scale (10–50 papers).

use crate::catalog::Catalog;
use crate::extract::Card;
use crate::util::search_tokens;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize, Clone)]
pub struct Hit {
    pub paper: String,
    pub cite: String,
    pub title: String,
    pub link: String,
    pub passage: String,
    pub section: String,
    pub stage: String,
    pub text: String,
    pub score: f64,
    pub techniques: Vec<String>,
}

pub fn search(query: &str, cards: &[Card], cat: &Catalog, stage: Option<&str>, limit: usize) -> Vec<Hit> {
    let q = search_tokens(query);
    let q_tech = cat.match_text(query);
    if q.is_empty() && q_tech.is_empty() && stage.is_none() { return Vec::new(); }
    struct Doc<'a> { card: &'a Card, idx: usize, toks: Vec<String> }
    let mut docs = Vec::new();
    for c in cards {
        for (i, p) in c.passages.iter().enumerate() {
            let mut toks = search_tokens(&p.text);
            toks.extend(search_tokens(&p.section));
            docs.push(Doc { card: c, idx: i, toks });
        }
    }
    if docs.is_empty() { return Vec::new(); }
    let n = docs.len() as f64;
    let avg = docs.iter().map(|d| d.toks.len()).sum::<usize>() as f64 / n;
    let mut df: HashMap<&str, usize> = HashMap::new();
    for d in &docs {
        let mut seen = std::collections::HashSet::new();
        for t in &d.toks { if seen.insert(t.as_str()) { *df.entry(t.as_str()).or_default() += 1; } }
    }
    let (k1, b) = (1.2, 0.75);
    let mut hits: Vec<Hit> = docs.iter().filter_map(|d| {
        let p = &d.card.passages[d.idx];
        let mut tf: HashMap<&str, usize> = HashMap::new();
        for t in &d.toks { *tf.entry(t.as_str()).or_default() += 1; }
        let len = d.toks.len() as f64;
        let mut s = 0.0;
        for t in &q {
            let f = *tf.get(t.as_str()).unwrap_or(&0) as f64;
            if f == 0.0 { continue; }
            let dfi = *df.get(t.as_str()).unwrap_or(&0) as f64;
            let idf = ((n - dfi + 0.5) / (dfi + 0.5) + 1.0).ln();
            s += idf * f * (k1 + 1.0) / (f + k1 * (1.0 - b + b * len / avg));
        }
        let shared = p.techniques.iter().filter(|t| q_tech.contains(t)).count();
        s += 2.5 * shared as f64;
        if let Some(st) = stage { if p.stage == st { s += if q.is_empty() { 1.0 } else { 0.8 }; } else if q.is_empty() { return None; } }
        if s <= 0.0 { return None; }
        Some(Hit {
            paper: d.card.id.clone(), cite: d.card.cite(), title: d.card.title.clone(), link: d.card.link.clone(),
            passage: p.id.clone(), section: p.section.clone(), stage: p.stage.clone(), text: p.text.clone(), score: (s * 100.0).round() / 100.0,
            techniques: p.techniques.clone(),
        })
    }).collect();
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(limit);
    hits
}
