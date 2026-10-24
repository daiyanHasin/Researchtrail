//! Prompt packs: everything a free AI chat needs for one task, cut to a token
//! budget, with every paper passage and code snippet cited.

use crate::compare;
use crate::search;
use crate::store::State;
use crate::util::tokens;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;

#[derive(Serialize)]
pub struct Pack {
    pub text: String,
    pub tokens: usize,
    pub budget: usize,
    pub included: Vec<String>,
    pub left_out: Vec<String>,
}

fn code_excerpt(st: &State, path: &str, line: usize, cell: Option<usize>) -> Option<(String, String)> {
    let full = st.code_root().join(path);
    let text = fs::read_to_string(&full).ok()?;
    if let Some(ci) = cell {
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        let c = v["cells"].get(ci)?;
        return Some((format!("{} · cell {}", path, ci + 1), crate::code::cell_source(c)));
    }
    let lines: Vec<&str> = text.lines().collect();
    let (_, cf) = st.code.get(path)?;
    let unit = cf.units.iter().filter(|u| u.start <= line && line <= u.end).last();
    let (a, b) = match unit { Some(u) if u.end - u.start < 80 => (u.start, u.end), _ => (line.saturating_sub(4).max(1), (line + 4).min(lines.len())) };
    let body = lines.get(a.saturating_sub(1)..b.min(lines.len()))?.join("\n");
    Some((format!("{} lines {}–{}{}", path, a, b, unit.map(|u| format!(" ({})", u.name)).unwrap_or_default()), body))
}

pub fn build(st: &State, task: &str, stage: Option<&str>, budget: usize) -> Pack {
    let budget = budget.clamp(1000, 64000);
    let mut out = String::new();
    let mut included = Vec::new();
    let mut left_out = Vec::new();
    out.push_str("You are helping me with my research code. Use the paper passages and code below.\n");
    out.push_str("Rules: cite papers as [Author Year, section] when you rely on them; say when the passages don't cover something instead of guessing; change only the code I show; explain each change in one line.\n\n");
    out.push_str(&format!("## My task\n{}\n\n", if task.trim().is_empty() { "Review my approach and suggest improvements." } else { task.trim() }));
    let papers: Vec<String> = st.cards.iter().map(|c| format!("- {}: {}", c.cite(), c.title)).collect();
    out.push_str(&format!("## Papers I've read ({})\n{}\n\n", st.cards.len(), papers.join("\n")));

    let stages: Vec<String> = match stage { Some(s) if !s.is_empty() => vec![s.to_string()], _ => st.catalog.stages.iter().map(|s| s.id.clone()).collect() };
    let mut mine_hits: Vec<(String, usize, Option<usize>)> = Vec::new();
    for s in &stages {
        let t = compare::build(st, s);
        if t.columns.is_empty() { continue; }
        let block = format!("## {}: what each paper did vs. my code\n{}\n", t.stage_name, compare::markdown(&t));
        if tokens(&out) + tokens(&block) < budget / 3 || stages.len() == 1 { out.push_str(&block); included.push(format!("{} comparison", t.stage_name)); } else { left_out.push(format!("{} comparison", t.stage_name)); }
        for ev in t.mine.cells.values().flatten() { if let (Some(p), Some(l)) = (&ev.path, ev.line) { mine_hits.push((p.clone(), l, ev.cell)); } }
    }

    let q = format!("{} {}", task, stage.map(|s| st.catalog.stage_name(s)).unwrap_or_default());
    let hits = search::search(&q, &st.cards, &st.catalog, stage.filter(|s| !s.is_empty()), 24);
    let mut ev = String::from("## Paper evidence\n");
    let mut n = 0;
    for h in &hits {
        let line = format!("- [{}, {}] {}\n", h.cite, h.section, h.text);
        if tokens(&out) + tokens(&ev) + tokens(&line) > budget * 6 / 10 { left_out.push(format!("{} more passages", hits.len() - n)); break; }
        ev.push_str(&line);
        n += 1;
    }
    if n > 0 { out.push_str(&ev); out.push('\n'); included.push(format!("{} paper passages", n)); }

    let mut seen = BTreeSet::new();
    let mut code = String::from("## My code\n");
    let mut shown = 0;
    let mut outline = Vec::new();
    for (p, l, c) in mine_hits {
        let Some((label, body)) = code_excerpt(st, &p, l, c) else { continue };
        if !seen.insert(label.clone()) { continue; }
        let block = format!("### {}\n```python\n{}\n```\n", label, body.trim_end());
        if tokens(&out) + tokens(&code) + tokens(&block) > budget { outline.push(label); continue; }
        code.push_str(&block);
        shown += 1;
    }
    if !outline.is_empty() {
        let o = format!("Other related code (not shown): {}\n", outline.join("; "));
        if tokens(&out) + tokens(&code) + tokens(&o) <= budget { code.push_str(&o); }
        left_out.push(format!("{} code excerpts", outline.len()));
    }
    if shown > 0 { out.push_str(&code); included.push(format!("{} code excerpts", shown)); }
    let tk = tokens(&out);
    Pack { text: out, tokens: tk, budget, included, left_out }
}
