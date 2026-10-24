//! Version history for people who don't use git.
//!
//! Every save of a Python file or notebook becomes a snapshot in `.trail/objects`
//! (content-addressed, so identical saves cost nothing). Files with messy names —
//! draft1.py, draft_final.py, final_final_v2.py — are grouped into one *lineage*
//! when two of three signals agree: similar name, similar content, later in time.
//! Your files are never moved, renamed or deleted.

use crate::catalog::Catalog;
use crate::code::{self, CodeFile};
use crate::util::{now, sha256_hex};
use regex::Regex;
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub time: u64,
    pub path: String,
    pub lineage: String,
    pub sha: String,
    pub size: u64,
    #[serde(default)]
    pub summary: Vec<String>,
    #[serde(default)]
    pub added: usize,
    #[serde(default)]
    pub removed: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lineage {
    pub id: String,
    pub name: String,
    pub members: Vec<String>,
    #[serde(default)]
    pub manual: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct LineageFile {
    lineages: Vec<Lineage>,
}

pub struct History {
    root: PathBuf,
    trail: PathBuf,
    pub snapshots: Vec<Snapshot>,
    pub lineages: Vec<Lineage>,
    seen: HashMap<String, (u64, u64)>,
    pub missing: BTreeSet<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub kind: String,
    pub text: String,
    pub snapshot: Option<String>,
}

impl History {
    pub fn load(root: &Path, trail: &Path) -> History {
        let mut snapshots = Vec::new();
        if let Ok(t) = fs::read_to_string(trail.join("changes.jsonl")) {
            for l in t.lines() { if let Ok(s) = serde_json::from_str::<Snapshot>(l) { snapshots.push(s); } }
        }
        let lineages = fs::read_to_string(trail.join("lineage.json")).ok().and_then(|t| serde_json::from_str::<LineageFile>(&t).ok()).unwrap_or_default().lineages;
        History { root: root.to_path_buf(), trail: trail.to_path_buf(), snapshots, lineages, seen: HashMap::new(), missing: BTreeSet::new() }
    }

    fn save_lineages(&self) {
        let f = LineageFile { lineages: self.lineages.clone() };
        let _ = fs::write(self.trail.join("lineage.json"), serde_json::to_string_pretty(&f).unwrap_or_default());
    }

    fn object_path(&self, sha: &str) -> PathBuf {
        self.trail.join("objects").join(&sha[..2]).join(sha)
    }

    pub fn object(&self, sha: &str) -> Option<Vec<u8>> {
        fs::read(self.object_path(sha)).ok()
    }

    /// Text used for diffs: notebooks without outputs and execution counts.
    pub fn view_text(&self, s: &Snapshot) -> String {
        let raw = self.object(&s.sha).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
        if s.path.ends_with(".ipynb") { code::strip_notebook(&raw) } else { raw }
    }

    /// Code that counts for technique changes: notebooks without never-run cells.
    pub fn run_code(&self, s: &Snapshot) -> String {
        let raw = self.object(&s.sha).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
        run_code_of(&s.path, &raw)
    }

    pub fn last_for_path(&self, path: &str) -> Option<&Snapshot> {
        self.snapshots.iter().rev().find(|s| s.path == path)
    }

    pub fn last_for_lineage(&self, lineage: &str) -> Option<&Snapshot> {
        self.snapshots.iter().rev().find(|s| s.lineage == lineage)
    }

    pub fn lineage_of(&self, path: &str) -> Option<&Lineage> {
        self.lineages.iter().find(|l| l.members.iter().any(|m| m == path))
    }

    /// The newest existing file in each lineage; everything else is superseded.
    pub fn current_files(&self) -> Vec<String> {
        let mut out = Vec::new();
        for l in &self.lineages {
            let best = l.members.iter().filter(|m| self.root.join(m).exists()).max_by_key(|m| mtime(&self.root.join(m)));
            if let Some(b) = best { out.push(b.clone()); }
        }
        out.sort();
        out
    }

    pub fn superseded(&self) -> BTreeSet<String> {
        let cur: BTreeSet<String> = self.current_files().into_iter().collect();
        self.lineages.iter().flat_map(|l| l.members.iter()).filter(|m| !cur.contains(*m)).cloned().collect()
    }

    /// Look for new or changed files and snapshot them. `settle` skips files saved in
    /// the last two seconds, so a burst of saves becomes one snapshot.
    pub fn scan(&mut self, cat: &Catalog, settle: bool) -> Vec<Event> {
        let mut events = Vec::new();
        let mut files: Vec<(PathBuf, u64)> = code::discover(&self.root).into_iter().map(|p| { let m = mtime(&self.root.join(&p)); (p, m) }).collect();
        files.sort_by_key(|(_, m)| *m);
        let present: BTreeSet<String> = files.iter().map(|(p, _)| rel(p)).collect();
        for (p, m) in files {
            let key = rel(&p);
            let full = self.root.join(&p);
            let size = fs::metadata(&full).map(|x| x.len()).unwrap_or(0);
            if self.seen.get(&key) == Some(&(m, size)) { continue; }
            if settle && now().saturating_sub(m) < 2 { continue; }
            let Ok(bytes) = fs::read(&full) else { continue };
            self.seen.insert(key.clone(), (m, size));
            let sha = sha256_hex(&bytes);
            if self.last_for_path(&key).map(|s| s.sha == sha).unwrap_or(false) { continue; }
            if let Some(ev) = self.snapshot(&key, &bytes, &sha, m, cat) { events.push(ev); }
        }
        let gone: BTreeSet<String> = self.lineages.iter().flat_map(|l| l.members.iter()).filter(|m| !present.contains(*m)).cloned().collect();
        for g in gone.difference(&self.missing.clone()) {
            events.push(Event { kind: "missing".into(), text: format!("{} is no longer in the folder; its history is kept", g), snapshot: None });
        }
        self.missing = gone;
        events
    }

    fn snapshot(&mut self, path: &str, bytes: &[u8], sha: &str, mtime: u64, cat: &Catalog) -> Option<Event> {
        let op = self.object_path(sha);
        if !op.exists() {
            fs::create_dir_all(op.parent()?).ok()?;
            fs::write(&op, bytes).ok()?;
        }
        let text = String::from_utf8_lossy(bytes).to_string();
        let view = if path.ends_with(".ipynb") { code::strip_notebook(&text) } else { text.clone() };
        let (lineage, joined) = match self.lineage_of(path) {
            Some(l) => (l.id.clone(), None),
            None => self.assign_lineage(path, &view, mtime),
        };
        let prev = self.last_for_lineage(&lineage).cloned();
        let mut summary = Vec::new();
        let (added, removed): (usize, usize);
        if let Some(prev) = &prev {
            let before = self.view_text(prev);
            if before == view { return None; }
            let (a, r) = line_stats(&before, &view);
            (added, removed) = (a, r);
            if prev.path != path { summary.push(format!("Saved as {} (continues {})", file_name(path), file_name(&prev.path))); }
            let before_run = self.run_code(prev);
            let after_run = run_code_of(path, &text);
            summary.extend(describe_change(&before_run, &after_run, cat));
        } else {
            summary.push(if joined.is_some() { "Joined an existing history".into() } else { "First version".into() });
            (added, removed) = (view.lines().count(), 0);
        }
        let id = format!("s{}", self.snapshots.len() + 1);
        let when = if mtime > 0 && mtime <= now() { mtime } else { now() };
        let s = Snapshot { id: id.clone(), time: when, path: path.to_string(), lineage: lineage.clone(), sha: sha.to_string(), size: bytes.len() as u64, summary: summary.clone(), added, removed };
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(self.trail.join("changes.jsonl")) {
            let _ = writeln!(f, "{}", serde_json::to_string(&s).unwrap_or_default());
        }
        self.snapshots.push(s);
        let text = if let Some(j) = joined { format!("{}: recognised as a newer version of {}", file_name(path), file_name(&j)) } else { format!("{}: {}", file_name(path), summary.join("; ")) };
        Some(Event { kind: "snapshot".into(), text, snapshot: Some(id) })
    }

    /// Decide which history a new file belongs to.
    fn assign_lineage(&mut self, path: &str, content: &str, file_mtime: u64) -> (String, Option<String>) {
        let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
        let n = normal_name(path);
        let mut best: Option<(usize, f64, String)> = None;
        for (i, l) in self.lineages.iter().enumerate() {
            let Some(last) = self.snapshots.iter().rev().find(|s| s.lineage == l.id) else { continue };
            if Path::new(&last.path).extension().and_then(|e| e.to_str()).unwrap_or("") != ext { continue; }
            if Path::new(&last.path).parent() != Path::new(path).parent() && normal_name(&last.path) != n { continue; }
            let name_sim = if normal_name(&last.path) == n { 1.0 } else { token_jaccard(&normal_name(&last.path), &n) };
            let content_sim = shingle_jaccard(&self.view_text(last), content);
            let later = file_mtime + 1 >= mtime(&self.root.join(&last.path)).min(last.time);
            let signals = (name_sim >= 0.6) as u8 + (content_sim >= 0.6) as u8 + later as u8;
            if signals >= 2 && (name_sim >= 0.6 || content_sim >= 0.6) {
                let score = name_sim + content_sim;
                if best.as_ref().map(|b| score > b.1).unwrap_or(true) { best = Some((i, score, last.path.clone())); }
            }
        }
        if let Some((i, _, from)) = best {
            self.lineages[i].members.push(path.to_string());
            self.save_lineages();
            return (self.lineages[i].id.clone(), Some(from));
        }
        let mut id = if n.is_empty() { "untitled".to_string() } else { n.clone() };
        if self.lineages.iter().any(|l| l.id == id) { id = (2..).map(|k| format!("{}-{}", n, k)).find(|c| !self.lineages.iter().any(|l| &l.id == c)).unwrap(); }
        self.lineages.push(Lineage { id: id.clone(), name: if n.is_empty() { file_name(path) } else { n.replace('_', " ") }, members: vec![path.to_string()], manual: false });
        self.save_lineages();
        (id, None)
    }

    /// Move one file into a history of its own (the "these aren't the same file" fix).
    pub fn split(&mut self, path: &str) -> Result<(), String> {
        let Some(pos) = self.lineages.iter().position(|l| l.members.iter().any(|m| m == path)) else { return Err("That file has no history yet.".into()) };
        if self.lineages[pos].members.len() == 1 { return Err("That file already has its own history.".into()); }
        self.lineages[pos].members.retain(|m| m != path);
        let base = normal_name(path);
        let id = (1..).map(|k| format!("{}-{}", base, k)).find(|c| !self.lineages.iter().any(|l| &l.id == c)).unwrap();
        self.lineages.push(Lineage { id: id.clone(), name: file_name(path), members: vec![path.to_string()], manual: true });
        for s in self.snapshots.iter_mut().filter(|s| s.path == path) { s.lineage = id.clone(); }
        self.rewrite_log();
        self.save_lineages();
        Ok(())
    }

    /// Join two histories that are really one file.
    pub fn merge(&mut self, into: &str, from: &str) -> Result<(), String> {
        if into == from { return Err("Pick two different histories.".into()); }
        let Some(fi) = self.lineages.iter().position(|l| l.id == from) else { return Err("Unknown history.".into()) };
        let moved = self.lineages.remove(fi);
        let Some(t) = self.lineages.iter_mut().find(|l| l.id == into) else { self.lineages.push(moved); return Err("Unknown history.".into()) };
        t.members.extend(moved.members);
        t.manual = true;
        for s in self.snapshots.iter_mut().filter(|s| s.lineage == from) { s.lineage = into.to_string(); }
        self.rewrite_log();
        self.save_lineages();
        Ok(())
    }

    fn rewrite_log(&self) {
        let body: String = self.snapshots.iter().map(|s| serde_json::to_string(s).unwrap_or_default() + "\n").collect();
        let _ = fs::write(self.trail.join("changes.jsonl"), body);
    }

    /// Put an old version back. The current file is snapshotted first, so nothing is lost.
    pub fn restore(&mut self, id: &str, cat: &Catalog) -> Result<String, String> {
        let s = self.snapshots.iter().find(|s| s.id == id).cloned().ok_or("Unknown version.")?;
        let bytes = self.object(&s.sha).ok_or("That version's content is missing from .trail/objects.")?;
        let full = self.root.join(&s.path);
        if let Ok(cur) = fs::read(&full) {
            let sha = sha256_hex(&cur);
            if self.last_for_path(&s.path).map(|x| x.sha != sha).unwrap_or(true) { self.snapshot(&s.path, &cur, &sha, mtime(&full), cat); }
        }
        if let Some(p) = full.parent() { fs::create_dir_all(p).map_err(|e| e.to_string())?; }
        fs::write(&full, &bytes).map_err(|e| format!("Couldn't write {}: {}", s.path, e))?;
        self.seen.remove(&s.path);
        Ok(s.path)
    }

    pub fn diff(&self, from: Option<&str>, to: &str) -> Result<Vec<(String, String)>, String> {
        let b = self.snapshots.iter().find(|s| s.id == to).ok_or("Unknown version.")?;
        let a = match from {
            Some(f) => self.snapshots.iter().find(|s| s.id == f).cloned(),
            None => self.snapshots.iter().take_while(|s| s.id != to).filter(|s| s.lineage == b.lineage).last().cloned(),
        };
        let before = a.map(|a| self.view_text(&a)).unwrap_or_default();
        let after = self.view_text(b);
        let d = TextDiff::from_lines(&before, &after);
        let mut out = Vec::new();
        for group in d.grouped_ops(3) {
            out.push(("hunk".into(), String::new()));
            for op in group {
                for ch in d.iter_changes(&op) {
                    let tag = match ch.tag() { ChangeTag::Delete => "del", ChangeTag::Insert => "add", ChangeTag::Equal => "same" };
                    out.push((tag.into(), ch.value().trim_end_matches('\n').to_string()));
                }
            }
        }
        Ok(out)
    }
}

fn run_code_of(path: &str, raw: &str) -> String {
    if path.ends_with(".ipynb") { code::notebook_code(raw, true).map(|x| x.0).unwrap_or_default() } else { raw.to_string() }
}

pub fn mtime(p: &Path) -> u64 {
    fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

fn rel(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

pub fn file_name(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).to_string()
}

static VERSION_TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(v\d+|\d+|final|finale|draft|copy|new|old|latest|updated?|rev\d*|version|ver|fixed|fix|backup|bak|temp|tmp|working|clean|edited|revised|\d{4,8})$").unwrap());

/// "final_final_v2 (1).py" -> "" ; "model_draft3.py" -> "model"; "Feature Eng v2.ipynb" -> "feature_eng"
pub fn normal_name(path: &str) -> String {
    let stem = Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let parts: Vec<String> = stem.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty() && !VERSION_TOKEN.is_match(t))
        .map(|t| t.trim_end_matches(|c: char| c.is_ascii_digit()).to_string())
        .filter(|t| !t.is_empty() && !VERSION_TOKEN.is_match(t)).collect();
    parts.join("_")
}

fn token_jaccard(a: &str, b: &str) -> f64 {
    let x: BTreeSet<&str> = a.split('_').filter(|s| !s.is_empty()).collect();
    let y: BTreeSet<&str> = b.split('_').filter(|s| !s.is_empty()).collect();
    if x.is_empty() && y.is_empty() { return 1.0; }
    let i = x.intersection(&y).count() as f64;
    let u = x.union(&y).count() as f64;
    if u == 0.0 { 0.0 } else { i / u }
}

/// Similarity of two files: Jaccard over 3-line shingles of non-empty lines.
pub fn shingle_jaccard(a: &str, b: &str) -> f64 {
    let la: Vec<&str> = a.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    let lb: Vec<&str> = b.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    let k = if la.len().min(lb.len()) < 6 { 1 } else { 3 };
    let sh = |v: &Vec<&str>| -> BTreeSet<String> { if v.len() < k { return v.iter().map(|s| s.to_string()).collect(); } v.windows(k).map(|w| w.join("\n")).collect() };
    let (x, y) = (sh(&la), sh(&lb));
    if x.is_empty() || y.is_empty() { return 0.0; }
    x.intersection(&y).count() as f64 / x.union(&y).count() as f64
}

fn line_stats(a: &str, b: &str) -> (usize, usize) {
    let d = TextDiff::from_lines(a, b);
    let (mut add, mut del) = (0, 0);
    for c in d.iter_all_changes() { match c.tag() { ChangeTag::Insert => add += 1, ChangeTag::Delete => del += 1, _ => {} } }
    (add, del)
}

static NUM_ASSIGN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)\b([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(-?\d+(?:\.\d+)?(?:[eE]-?\d+)?)\b").unwrap());
static DEF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*def\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap());

fn numbers(src: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for line in src.lines() {
        let code = line.split('#').next().unwrap_or("");
        for c in NUM_ASSIGN.captures_iter(code) { m.insert(c[1].to_string(), c[2].to_string()); }
    }
    m
}

fn techniques(src: &str, cat: &Catalog) -> BTreeSet<String> {
    let mut cf = CodeFile::default();
    code::analyse_source(src, None, cat, &mut cf);
    cf.hits.into_iter().map(|h| h.technique).collect()
}

/// Plain-language summary of what changed between two versions.
pub fn describe_change(before: &str, after: &str, cat: &Catalog) -> Vec<String> {
    let mut out = Vec::new();
    let (nb, na) = (numbers(before), numbers(after));
    let mut changed = Vec::new();
    for (k, v) in &na {
        match nb.get(k) { Some(old) if old != v => changed.push(format!("{} {} → {}", k, old, v)), None => {} _ => {} }
    }
    if !changed.is_empty() { out.push(format!("Changed {}", changed.into_iter().take(4).collect::<Vec<_>>().join(", "))); }
    let (tb, ta) = (techniques(before, cat), techniques(after, cat));
    let name = |id: &String| cat.get(id).map(|t| t.name.clone()).unwrap_or(id.clone());
    let added: Vec<String> = ta.difference(&tb).map(name).collect();
    let removed: Vec<String> = tb.difference(&ta).map(name).collect();
    if !added.is_empty() { out.push(format!("Added {}", added.join(", "))); }
    if !removed.is_empty() { out.push(format!("Removed {}", removed.join(", "))); }
    let fb: BTreeSet<String> = DEF.captures_iter(before).map(|c| c[1].to_string()).collect();
    let fa: BTreeSet<String> = DEF.captures_iter(after).map(|c| c[1].to_string()).collect();
    let newf: Vec<&String> = fa.difference(&fb).collect();
    let gone: Vec<&String> = fb.difference(&fa).collect();
    if !newf.is_empty() { out.push(format!("New function{} {}", if newf.len() > 1 { "s" } else { "" }, newf.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))); }
    if !gone.is_empty() { out.push(format!("Removed function{} {}", if gone.len() > 1 { "s" } else { "" }, gone.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))); }
    if out.is_empty() { out.push("Edited code".into()); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn messy_names_normalise() {
        assert_eq!(normal_name("model_draft3.py"), "model");
        assert_eq!(normal_name("model_final_final_v2.py"), "model");
        assert_eq!(normal_name("Feature Eng v2 (1).ipynb"), "feature_eng");
        assert_eq!(normal_name("final_final.py"), "");
        assert_eq!(normal_name("train.py"), "train");
    }
    #[test]
    fn change_summary_reads_like_a_person() {
        let cat = Catalog::load(Path::new("/nonexistent"));
        let a = "lr = 0.001\nmodel = RandomForestClassifier(n_estimators=100)\n";
        let b = "lr = 0.0005\nX, y = SMOTE().fit_resample(X, y)\nmodel = RandomForestClassifier(n_estimators=300)\ndef evaluate(m):\n    pass\n";
        let s = describe_change(a, b, &cat).join(" | ");
        assert!(s.contains("lr 0.001 → 0.0005"), "{}", s);
        assert!(s.contains("n_estimators 100 → 300"), "{}", s);
        assert!(s.contains("Added SMOTE oversampling"), "{}", s);
        assert!(s.contains("New function evaluate"), "{}", s);
    }
    #[test]
    fn lineage_groups_drafts_and_keeps_files() {
        let dir = std::env::temp_dir().join(format!("rt-lineage-{}", now()));
        let trail = dir.join(".trail");
        fs::create_dir_all(&trail).unwrap();
        let body: String = (0..30).map(|i| format!("x{} = {}\n", i, i)).collect();
        fs::write(dir.join("model_draft1.py"), &body).unwrap();
        let cat = Catalog::load(&trail);
        let mut h = History::load(&dir, &trail);
        h.scan(&cat, false);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(dir.join("model_final.py"), format!("{}y = 2\n", body)).unwrap();
        fs::write(dir.join("loader.py"), "import os\nprint('load')\n").unwrap();
        h.scan(&cat, false);
        assert_eq!(h.lineages.len(), 2, "{:?}", h.lineages);
        let model = h.lineages.iter().find(|l| l.id == "model").unwrap();
        assert_eq!(model.members.len(), 2);
        assert!(dir.join("model_draft1.py").exists(), "never delete user files");
        assert!(h.superseded().contains("model_draft1.py"));
        let last = h.snapshots.last().unwrap().clone();
        assert!(h.diff(None, &last.id).is_ok());
        fs::remove_dir_all(&dir).ok();
    }
}
