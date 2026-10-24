//! The comparison table for one pipeline stage: a row per paper plus "Mine".
//! Both sides are sets of catalog technique IDs, so every cell is explainable:
//! a paper cell points to the passage that says it, your cell to the code line.

use crate::store::State;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize)]
pub struct Evidence {
    pub kind: String,
    pub text: String,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub cell: Option<usize>,
    pub unit: Option<String>,
    pub passage: Option<String>,
    pub source: String,
}

#[derive(Serialize)]
pub struct Row {
    pub kind: String,
    pub id: String,
    pub label: String,
    pub title: String,
    pub link: String,
    pub cells: BTreeMap<String, Vec<Evidence>>,
    pub facts: Vec<String>,
}

#[derive(Serialize)]
pub struct Column {
    pub id: String,
    pub name: String,
    pub papers: usize,
    pub mine: bool,
}

#[derive(Serialize)]
pub struct Table {
    pub stage: String,
    pub stage_name: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    pub mine: Row,
    pub only_mine: Vec<String>,
    pub common_missing: Vec<String>,
    pub unknown: Vec<(String, String, usize)>,
    pub removed: Vec<(String, String)>,
    pub files_used: Vec<String>,
}

pub fn mine_cells(st: &State, stage: &str) -> (BTreeMap<String, Vec<Evidence>>, Vec<String>) {
    let current = st.current_files();
    let mut cells: BTreeMap<String, Vec<Evidence>> = BTreeMap::new();
    let mut used = BTreeSet::new();
    for (path, (_, cf)) in &st.code {
        if !current.contains(path) { continue; }
        for h in &cf.hits {
            let Some(t) = st.catalog.get(&h.technique) else { continue };
            if t.stage != stage { continue; }
            if st.overrides.removed.contains(&format!("{}|{}", h.technique, path)) { continue; }
            used.insert(path.clone());
            let list = cells.entry(h.technique.clone()).or_default();
            if list.len() < 6 {
                list.push(Evidence { kind: "code".into(), text: h.snippet.clone(), path: Some(path.clone()), line: Some(h.line), cell: h.cell, unit: Some(h.unit.clone()), passage: None, source: "code".into() });
            }
        }
    }
    for m in st.overrides.added.iter().filter(|m| m.stage == stage) {
        cells.entry(m.technique.clone()).or_default().push(Evidence { kind: "you".into(), text: if m.note.is_empty() { "Added by you".into() } else { m.note.clone() }, path: None, line: None, cell: None, unit: None, passage: None, source: "you".into() });
    }
    (cells, used.into_iter().collect())
}

pub fn build(st: &State, stage: &str) -> Table {
    let cat = &st.catalog;
    let mut rows = Vec::new();
    for c in &st.cards {
        let mut cells: BTreeMap<String, Vec<Evidence>> = BTreeMap::new();
        for e in c.stages.get(stage).cloned().unwrap_or_default() {
            let ev: Vec<Evidence> = e.passages.iter().filter_map(|pid| c.passage(pid)).map(|p| Evidence { kind: "paper".into(), text: p.text.clone(), path: None, line: None, cell: None, unit: None, passage: Some(p.id.clone()), source: e.source.clone() }).collect();
            if !ev.is_empty() { cells.insert(e.technique.clone(), ev); }
        }
        let mut facts = Vec::new();
        if stage == "data" { facts.extend(c.datasets.iter().map(|d| d.label.clone())); }
        if stage == "evaluation" { facts.extend(c.metrics.iter().take(6).map(|m| format!("{} {}", m.label, m.value))); }
        if stage == "training" { facts.extend(c.hyperparams.iter().take(6).map(|h| format!("{} {}", h.label, h.value))); }
        rows.push(Row { kind: "paper".into(), id: c.id.clone(), label: c.cite(), title: c.title.clone(), link: c.link.clone(), cells, facts });
    }
    let (mcells, files_used) = mine_cells(st, stage);
    let mine = Row { kind: "mine".into(), id: "mine".into(), label: "Mine".into(), title: "Your code".into(), link: String::new(), cells: mcells, facts: Vec::new() };

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &rows { for k in r.cells.keys() { *counts.entry(k.clone()).or_default() += 1; } }
    let mut ids: BTreeSet<String> = counts.keys().cloned().collect();
    ids.extend(mine.cells.keys().cloned());
    let name_of = |id: &str| -> String {
        if let Some(t) = cat.get(id) { return t.name.clone(); }
        for c in &st.cards { for list in c.stages.values() { if let Some(e) = list.iter().find(|e| e.technique == id) { return e.name.clone(); } } }
        id.trim_start_matches("new:").replace('-', " ")
    };
    let mut columns: Vec<Column> = ids.iter().map(|id| Column { id: id.clone(), name: name_of(id), papers: *counts.get(id).unwrap_or(&0), mine: mine.cells.contains_key(id) }).collect();
    columns.sort_by(|a, b| b.papers.cmp(&a.papers).then(b.mine.cmp(&a.mine)).then(a.name.cmp(&b.name)));
    let n_papers = rows.len();
    let only_mine = columns.iter().filter(|c| c.mine && c.papers == 0).map(|c| c.name.clone()).collect();
    let common_missing = columns.iter().filter(|c| !c.mine && c.papers >= 2 && c.papers * 2 >= n_papers).map(|c| c.name.clone()).collect();
    let current = st.current_files();
    let mut unknown = Vec::new();
    for (path, (_, cf)) in &st.code {
        if !current.contains(path) { continue; }
        for (n, line) in &cf.unknown { if unknown.len() < 30 { unknown.push((n.clone(), path.clone(), *line)); } }
    }
    let removed = st.overrides.removed.iter().filter_map(|k| { let (t, p) = k.split_once('|')?; (cat.get(t).map(|x| x.stage.as_str()) == Some(stage)).then(|| (t.to_string(), p.to_string())) }).collect();
    Table { stage: stage.into(), stage_name: cat.stage_name(stage), columns, rows, mine, only_mine, common_missing, unknown, removed, files_used }
}

/// Counts per stage for the overview trail.
#[derive(Serialize)]
pub struct Coverage {
    pub stage: String,
    pub name: String,
    pub papers: usize,
    pub paper_techniques: usize,
    pub mine: usize,
    pub shared: usize,
}

pub fn coverage(st: &State) -> Vec<Coverage> {
    st.catalog.stages.iter().map(|s| {
        let mut pt = BTreeSet::new();
        let mut papers = 0;
        for c in &st.cards {
            if let Some(l) = c.stages.get(&s.id) { if !l.is_empty() { papers += 1; } for e in l { pt.insert(e.technique.clone()); } }
        }
        let (m, _) = mine_cells(st, &s.id);
        let shared = m.keys().filter(|k| pt.contains(*k)).count();
        Coverage { stage: s.id.clone(), name: s.name.clone(), papers, paper_techniques: pt.len(), mine: m.len(), shared }
    }).collect()
}

/// The table as Markdown, for prompt packs.
pub fn markdown(t: &Table) -> String {
    if t.columns.is_empty() { return format!("No techniques found yet for {}.\n", t.stage_name); }
    let cols: Vec<&Column> = t.columns.iter().take(10).collect();
    let mut s = format!("| {} | {} |\n|", t.stage_name, cols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(" | "));
    for _ in 0..=cols.len() { s.push_str(" --- |"); }
    s.push('\n');
    for r in t.rows.iter().chain(std::iter::once(&t.mine)) {
        s.push_str(&format!("| {} |", r.label));
        for c in &cols { s.push_str(if r.cells.contains_key(&c.id) { " yes |" } else { " |" }); }
        s.push('\n');
    }
    s
}
