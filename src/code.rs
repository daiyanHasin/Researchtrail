//! Reads your Python files and notebooks and finds the techniques they use.
//!
//! Code is parsed with tree-sitter, so comments and strings never count. Names
//! used in one statement (calls, attributes, keyword arguments) are matched
//! against the catalog's code patterns. Notebook cells that were never run are
//! skipped, so experiments you abandoned don't show up as "what you did".

use crate::catalog::Catalog;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser};

pub const SKIP_DIRS: &[&str] = &[".trail", ".git", "node_modules", "venv", ".venv", "env", ".env", "__pycache__", ".ipynb_checkpoints", "site-packages", "dist", "build", "target", ".idea", ".vscode", ".mypy_cache", ".pytest_cache"];
pub const MAX_FILE: u64 = 3 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct Unit {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Hit {
    pub technique: String,
    pub line: usize,
    pub cell: Option<usize>,
    pub unit: String,
    pub snippet: String,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct CodeFile {
    pub path: String,
    pub kind: String,
    pub lines: usize,
    pub units: Vec<Unit>,
    pub hits: Vec<Hit>,
    pub unknown: Vec<(String, usize)>,
    pub never_run: bool,
    pub skipped_cells: usize,
    pub error: Option<String>,
}

pub fn is_code(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()), Some("py") | Some("ipynb"))
}

/// All Python files and notebooks under `root`, as paths relative to it.
pub fn discover(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') { stack.push(p); }
            } else if ft.is_file() && is_code(&p) {
                if e.metadata().map(|m| m.len() <= MAX_FILE).unwrap_or(false) {
                    if let Ok(rel) = p.strip_prefix(root) { out.push(rel.to_path_buf()); }
                }
            }
        }
    }
    out.sort();
    out
}

/// Notebook source with outputs and execution counts removed, one cell after another.
/// Returns (virtual source, line map to (cell, line in cell), never_run, skipped cells).
pub fn notebook_code(json: &str, only_run: bool) -> Result<(String, Vec<(usize, usize)>, bool, usize), String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("not a valid notebook: {}", e))?;
    let cells = v["cells"].as_array().cloned().unwrap_or_default();
    let code_cells: Vec<&serde_json::Value> = cells.iter().filter(|c| c["cell_type"] == "code").collect();
    let any_run = code_cells.iter().any(|c| c["execution_count"].is_number());
    let mut src = String::new();
    let mut map = Vec::new();
    let mut skipped = 0;
    for (ci, c) in cells.iter().enumerate() {
        if c["cell_type"] != "code" { continue; }
        if only_run && any_run && !c["execution_count"].is_number() { skipped += 1; continue; }
        let text = cell_source(c);
        for (li, line) in text.lines().enumerate() {
            let t = line.trim_start();
            let safe = if t.starts_with('%') || t.starts_with('!') { "" } else { line };
            src.push_str(safe);
            src.push('\n');
            map.push((ci, li + 1));
        }
        src.push('\n');
        map.push((ci, 0));
    }
    Ok((src, map, !any_run && !code_cells.is_empty(), skipped))
}

pub fn cell_source(c: &serde_json::Value) -> String {
    match &c["source"] {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_str()).collect::<String>(),
        _ => String::new(),
    }
}

/// Strip a notebook down to what matters for history: cell types, ids and sources.
pub fn strip_notebook(json: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return json.to_string() };
    let mut out = String::new();
    for (i, c) in v["cells"].as_array().cloned().unwrap_or_default().iter().enumerate() {
        let kind = c["cell_type"].as_str().unwrap_or("code");
        out.push_str(&format!("# ── cell {} ({}) ──\n", i + 1, kind));
        out.push_str(&cell_source(c));
        if !out.ends_with('\n') { out.push('\n'); }
    }
    out
}

const NOISE: &[&str] = &["DataFrame", "Series", "Path", "Figure", "Counter", "OrderedDict", "Exception", "ValueError", "TypeError", "KeyError", "Image", "Timestamp", "Timedelta", "Tensor", "ArgumentParser", "Pipeline", "ColumnTransformer", "Sequential", "Module", "Linear", "ReLU", "Softmax", "Sigmoid", "Tanh", "DataLoader", "Dataset", "TensorDataset", "Input", "Model", "Flatten", "Activation", "True", "False", "None", "GridSpec", "Axes"];

pub fn analyse(root: &Path, rel: &Path, cat: &Catalog) -> CodeFile {
    let full = root.join(rel);
    let mut cf = CodeFile { path: rel.to_string_lossy().replace('\\', "/"), ..Default::default() };
    let Ok(text) = std::fs::read_to_string(&full) else { cf.error = Some("couldn't read this file".into()); return cf };
    let is_nb = rel.extension().and_then(|e| e.to_str()) == Some("ipynb");
    cf.kind = if is_nb { "notebook".into() } else { "python".into() };
    let (src, map) = if is_nb {
        match notebook_code(&text, true) {
            Ok((s, m, never, skipped)) => { cf.never_run = never; cf.skipped_cells = skipped; (s, Some(m)) }
            Err(e) => { cf.error = Some(e); return cf; }
        }
    } else { (text.clone(), None) };
    cf.lines = text.lines().count();
    analyse_source(&src, map.as_deref(), cat, &mut cf);
    cf
}

pub fn analyse_source(src: &str, map: Option<&[(usize, usize)]>, cat: &Catalog, cf: &mut CodeFile) {
    let mut parser = Parser::new();
    if parser.set_language(&tree_sitter_python::language()).is_err() { cf.error = Some("Python parser failed to load".into()); return; }
    let Some(tree) = parser.parse(src, None) else { cf.error = Some("couldn't parse".into()); return };
    let lines: Vec<&str> = src.lines().collect();
    let mut by_stmt: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    let mut calls: Vec<(String, usize)> = Vec::new();
    let mut funcs: Vec<Unit> = Vec::new();
    walk(tree.root_node(), src.as_bytes(), &mut by_stmt, &mut calls, &mut funcs);

    let place = |row: usize| -> (usize, Option<usize>) {
        match map { Some(m) => m.get(row).map(|(c, l)| (*l, Some(*c))).unwrap_or((row + 1, None)), None => (row + 1, None) }
    };
    let unit_for = |row: usize, cell: Option<usize>| -> String {
        if let Some(f) = funcs.iter().filter(|f| f.start <= row && row <= f.end).last() { return f.name.clone(); }
        match cell { Some(c) => format!("cell {}", c + 1), None => "top level".into() }
    };
    let mut seen = BTreeSet::new();
    for (row, names) in &by_stmt {
        for t in cat.match_code(names) {
            let (line, cell) = place(*row);
            if !seen.insert((t.clone(), line, cell)) { continue; }
            cf.hits.push(Hit { technique: t, line, cell, unit: unit_for(*row, cell), snippet: lines.get(*row).map(|l| l.trim().chars().take(160).collect()).unwrap_or_default() });
        }
    }
    // Class-like calls the catalog doesn't know: candidates for "name this step".
    let known: BTreeSet<String> = cat.techniques.iter().flat_map(|t| t.code.iter().flat_map(|c| c.split('&').map(|s| s.to_string()))).collect();
    let mut unknown: BTreeMap<String, usize> = BTreeMap::new();
    for (name, row) in &calls {
        let mut ch = name.chars();
        let camel = ch.next().map(|c| c.is_ascii_uppercase()).unwrap_or(false) && name.chars().any(|c| c.is_ascii_lowercase());
        if camel && !known.contains(name) && !NOISE.contains(&name.as_str()) && name.len() > 3 {
            unknown.entry(name.clone()).or_insert(place(*row).0);
        }
    }
    cf.unknown = unknown.into_iter().collect();
    cf.units = funcs.into_iter().map(|f| { let (s, _) = place(f.start); let (e, _) = place(f.end); Unit { name: f.name, start: s, end: e } }).collect();
}

fn statement_row(mut n: Node) -> usize {
    loop {
        let Some(p) = n.parent() else { return n.start_position().row };
        let k = p.kind();
        if k == "module" || k == "block" { return n.start_position().row; }
        n = p;
    }
}

fn walk(n: Node, src: &[u8], by_stmt: &mut BTreeMap<usize, BTreeSet<String>>, calls: &mut Vec<(String, usize)>, funcs: &mut Vec<Unit>) {
    let text = |x: Node| x.utf8_text(src).unwrap_or("").to_string();
    match n.kind() {
        "call" => {
            if let Some(f) = n.child_by_field_name("function") {
                let name = match f.kind() {
                    "identifier" => Some(text(f)),
                    "attribute" => f.child_by_field_name("attribute").map(text),
                    _ => None,
                };
                if let Some(name) = name {
                    by_stmt.entry(statement_row(n)).or_default().insert(name.clone());
                    calls.push((name, n.start_position().row));
                }
            }
        }
        "attribute" => {
            if let Some(a) = n.child_by_field_name("attribute") { by_stmt.entry(statement_row(n)).or_default().insert(text(a)); }
        }
        "keyword_argument" => {
            if let Some(k) = n.child_by_field_name("name") { by_stmt.entry(statement_row(n)).or_default().insert(text(k)); }
        }
        "function_definition" => {
            if let Some(name) = n.child_by_field_name("name") {
                funcs.push(Unit { name: text(name), start: n.start_position().row, end: n.end_position().row });
            }
        }
        _ => {}
    }
    let mut cur = n.walk();
    for c in n.children(&mut cur) { walk(c, src, by_stmt, calls, funcs); }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cat() -> Catalog { Catalog::load(Path::new("/nonexistent")) }
    #[test]
    fn detects_techniques_and_ignores_comments() {
        let src = "import pandas as pd\n# df = df.dropna()  (old idea)\ndef features(df):\n    df['card_freq'] = df['card'].map(df['card'].value_counts())\n    df['u_mean'] = (df.groupby('user')['amt']\n        .transform('mean'))\n    return df\nfrom imblearn.over_sampling import SMOTE\nX, y = SMOTE(random_state=0).fit_resample(X, y)\nmodel = XGBClassifier(scale_pos_weight=10)\nmodel = QuantileTransformer()\n";
        let mut cf = CodeFile::default();
        analyse_source(src, None, &cat(), &mut cf);
        let ids: BTreeSet<&str> = cf.hits.iter().map(|h| h.technique.as_str()).collect();
        assert!(ids.contains("encode_frequency"), "{:?}", ids);
        assert!(ids.contains("feat_aggregation"), "multi-line chain should count: {:?}", ids);
        assert!(ids.contains("oversample_smote"));
        assert!(ids.contains("model_xgboost"));
        assert!(ids.contains("train_class_weights"));
        assert!(!ids.contains("missing_drop"), "commented code must not count");
        assert_eq!(cf.hits.iter().find(|h| h.technique == "encode_frequency").unwrap().unit, "features");
        assert!(cf.unknown.iter().any(|(n, _)| n == "QuantileTransformer"));
    }
    #[test]
    fn notebook_skips_unrun_cells() {
        let nb = r##"{"cells":[
          {"cell_type":"code","execution_count":1,"source":["import pandas as pd\n","df = df.dropna()\n"],"outputs":[]},
          {"cell_type":"code","execution_count":null,"source":["X = PCA(5).fit_transform(X)\n"],"outputs":[]},
          {"cell_type":"markdown","source":["# Notes"]}
        ],"nbformat":4,"nbformat_minor":5}"##;
        let (src, map, never, skipped) = notebook_code(nb, true).unwrap();
        assert!(!never);
        assert_eq!(skipped, 1);
        let mut cf = CodeFile::default();
        analyse_source(&src, Some(&map), &cat(), &mut cf);
        let ids: Vec<&str> = cf.hits.iter().map(|h| h.technique.as_str()).collect();
        assert_eq!(ids, vec!["missing_drop"]);
        assert_eq!(cf.hits[0].cell, Some(0));
        assert_eq!(cf.hits[0].line, 2);
    }
}
