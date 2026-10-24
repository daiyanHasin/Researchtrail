//! One open project: its `.trail/` folder, paper cards, code history and analysis.
//!
//!   .trail/config.json        settings (code folder, Unpaywall email, local model)
//!   .trail/cards/<id>.json    one paper card per paper (readable, editable)
//!   .trail/notes/<id>.md      your notes per paper
//!   .trail/objects/           code snapshots, content-addressed
//!   .trail/changes.jsonl      one line per snapshot, with a plain-language summary
//!   .trail/lineage.json       which file names belong to one history
//!   .trail/stages.json        your corrections to the "Mine" rows
//!   .trail/catalog.local.json techniques you named yourself

use crate::catalog::Catalog;
use crate::code::{self, CodeFile};
use crate::extract::Card;
use crate::history::History;
use crate::util::now;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "dot")]
    pub code_dir: String,
    #[serde(default)]
    pub unpaywall_email: String,
    #[serde(default)]
    pub ollama_model: String,
    #[serde(default = "budget")]
    pub budget: usize,
    #[serde(default = "yes")]
    pub archive_links: bool,
}
fn dot() -> String { ".".into() }
fn budget() -> usize { 8000 }
fn yes() -> bool { true }
impl Default for Config {
    fn default() -> Self { Config { code_dir: dot(), unpaywall_email: String::new(), ollama_model: String::new(), budget: budget(), archive_links: true } }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Overrides {
    /// "technique|path" pairs the user removed from their own row.
    #[serde(default)]
    pub removed: BTreeSet<String>,
    /// Techniques the user added to their row by hand: stage -> (technique, note)
    #[serde(default)]
    pub added: Vec<ManualEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManualEntry {
    pub stage: String,
    pub technique: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Job {
    pub id: String,
    pub input: String,
    pub status: String,
    pub message: String,
    pub card: Option<String>,
    pub started: u64,
    pub finished: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TimelineItem {
    pub time: u64,
    pub kind: String,
    pub text: String,
    pub link: Option<String>,
}

pub struct State {
    pub root: PathBuf,
    pub trail: PathBuf,
    pub config: Config,
    pub catalog: Catalog,
    pub cards: Vec<Card>,
    pub history: History,
    pub code: BTreeMap<String, (u64, CodeFile)>,
    pub overrides: Overrides,
    pub jobs: Vec<Job>,
    pub activity: Vec<TimelineItem>,
    pub version: u64,
}

impl State {
    pub fn open(root: &Path) -> Result<State, String> {
        let root = fs::canonicalize(root).map_err(|_| format!("Folder not found: {}", root.display()))?;
        if !root.is_dir() { return Err(format!("{} is not a folder", root.display())); }
        let trail = root.join(".trail");
        for d in ["cards", "notes", "objects"] { fs::create_dir_all(trail.join(d)).map_err(|e| format!("Can't write to {}: {}", trail.display(), e))?; }
        let config: Config = fs::read_to_string(trail.join("config.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let _ = fs::write(trail.join("config.json"), serde_json::to_string_pretty(&config).unwrap());
        let gi = trail.join(".gitignore");
        if !gi.exists() { let _ = fs::write(&gi, "objects/\n"); }
        let catalog = Catalog::load(&trail);
        let mut cards = Vec::new();
        if let Ok(rd) = fs::read_dir(trail.join("cards")) {
            for e in rd.flatten() {
                if let Ok(t) = fs::read_to_string(e.path()) { if let Ok(mut c) = serde_json::from_str::<Card>(&t) {
                    c.note = fs::read_to_string(trail.join("notes").join(format!("{}.md", c.id))).unwrap_or_default();
                    cards.push(c);
                } }
            }
        }
        cards.sort_by_key(|c| c.added);
        let overrides = fs::read_to_string(trail.join("stages.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let code_root = if config.code_dir.trim() == "." || config.code_dir.trim().is_empty() { root.clone() } else { root.join(&config.code_dir) };
        let history = History::load(&code_root, &trail);
        let mut s = State { root, trail, config, catalog, cards, history, code: BTreeMap::new(), overrides, jobs: Vec::new(), activity: Vec::new(), version: 1 };
        s.scan_code(false);
        Ok(s)
    }

    pub fn name(&self) -> String {
        self.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| self.root.display().to_string())
    }

    pub fn code_root(&self) -> PathBuf {
        let d = self.config.code_dir.trim();
        if d.is_empty() || d == "." { self.root.clone() } else { self.root.join(d) }
    }

    pub fn bump(&mut self) { self.version += 1; }

    pub fn log(&mut self, kind: &str, text: String, link: Option<String>) {
        self.activity.push(TimelineItem { time: now(), kind: kind.into(), text, link });
        if self.activity.len() > 300 { self.activity.remove(0); }
    }

    /// Snapshot changed files and refresh the analysis of anything that changed.
    pub fn scan_code(&mut self, settle: bool) -> bool {
        let events = self.history.scan(&self.catalog, settle);
        let root = self.code_root();
        let files = code::discover(&root);
        let mut changed = !events.is_empty();
        let present: BTreeSet<String> = files.iter().map(|p| p.to_string_lossy().replace('\\', "/")).collect();
        let before = self.code.len();
        self.code.retain(|k, _| present.contains(k));
        if self.code.len() != before { changed = true; }
        for p in files {
            let key = p.to_string_lossy().replace('\\', "/");
            let m = crate::history::mtime(&root.join(&p));
            if self.code.get(&key).map(|(t, _)| *t == m).unwrap_or(false) { continue; }
            let cf = code::analyse(&root, &p, &self.catalog);
            self.code.insert(key, (m, cf));
            changed = true;
        }
        for e in events { self.log(&e.kind, e.text, e.snapshot); }
        if changed { self.bump(); }
        changed
    }

    pub fn reanalyse_all(&mut self) {
        self.catalog = Catalog::load(&self.trail);
        self.code.clear();
        self.scan_code(false);
        self.bump();
    }

    pub fn save_card(&self, c: &Card) -> Result<(), String> {
        let mut copy = c.clone();
        copy.note = String::new();
        fs::write(self.trail.join("cards").join(format!("{}.json", c.id)), serde_json::to_string_pretty(&copy).unwrap()).map_err(|e| e.to_string())?;
        let np = self.trail.join("notes").join(format!("{}.md", c.id));
        if !c.note.is_empty() { fs::write(np, &c.note).map_err(|e| e.to_string())?; } else { let _ = fs::remove_file(np); }
        Ok(())
    }

    pub fn delete_card(&mut self, id: &str) -> Result<String, String> {
        let pos = self.cards.iter().position(|c| c.id == id).ok_or("Unknown paper.")?;
        let c = self.cards.remove(pos);
        let _ = fs::remove_file(self.trail.join("cards").join(format!("{}.json", id)));
        let _ = fs::remove_file(self.trail.join("notes").join(format!("{}.md", id)));
        self.bump();
        Ok(c.title)
    }

    pub fn save_overrides(&self) {
        let _ = fs::write(self.trail.join("stages.json"), serde_json::to_string_pretty(&self.overrides).unwrap());
    }

    pub fn save_config(&self) {
        let _ = fs::write(self.trail.join("config.json"), serde_json::to_string_pretty(&self.config).unwrap());
    }

    pub fn card_ids(&self) -> BTreeSet<String> { self.cards.iter().map(|c| c.id.clone()).collect() }

    pub fn current_files(&self) -> BTreeSet<String> { self.history.current_files().into_iter().collect() }

    pub fn find_card_by_canonical(&self, canonical: &Option<String>, link: &str) -> Option<String> {
        self.cards.iter().find(|c| (canonical.is_some() && &c.canonical == canonical) || c.link == link).map(|c| c.id.clone())
    }
}
