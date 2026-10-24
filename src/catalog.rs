//! The technique catalog: the shared vocabulary that papers and code are both
//! translated into. Comparing papers with your code then means comparing sets of
//! technique IDs, which is deterministic and explainable.

use crate::util::{stem, stems};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

pub const BUILTIN: &str = include_str!("../catalog/techniques.json");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Technique {
    pub id: String,
    pub name: String,
    pub stage: String,
    #[serde(default)]
    pub phrases: Vec<String>,
    #[serde(default)]
    pub code: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stage {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    stages: Vec<Stage>,
    #[serde(default)]
    datasets: Vec<String>,
    #[serde(default)]
    techniques: Vec<Technique>,
}

pub struct Catalog {
    pub stages: Vec<Stage>,
    pub techniques: Vec<Technique>,
    pub datasets: Vec<String>,
    phrase_index: Vec<(Vec<String>, usize)>,
    code_index: Vec<(Vec<String>, usize)>,
    by_id: HashMap<String, usize>,
}

impl Catalog {
    pub fn load(trail: &Path) -> Catalog {
        let base: File = serde_json::from_str(BUILTIN).expect("built-in catalog is valid JSON");
        let mut techniques = base.techniques;
        let local = trail.join("catalog.local.json");
        if let Ok(text) = std::fs::read_to_string(&local) {
            if let Ok(f) = serde_json::from_str::<File>(&text) {
                for mut t in f.techniques {
                    t.local = true;
                    if let Some(pos) = techniques.iter().position(|x| x.id == t.id) { techniques[pos] = t; } else { techniques.push(t); }
                }
            }
        }
        Catalog::build(base.stages, techniques, base.datasets)
    }

    pub fn build(stages: Vec<Stage>, techniques: Vec<Technique>, datasets: Vec<String>) -> Catalog {
        let mut phrase_index = Vec::new();
        let mut code_index = Vec::new();
        let mut by_id = HashMap::new();
        for (i, t) in techniques.iter().enumerate() {
            by_id.insert(t.id.clone(), i);
            for p in &t.phrases {
                let s = stems(p);
                if !s.is_empty() { phrase_index.push((s, i)); }
            }
            for c in &t.code {
                let parts: Vec<String> = c.split('&').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                if !parts.is_empty() { code_index.push((parts, i)); }
            }
        }
        // Longer phrases first so "random forest" wins over shorter overlaps.
        phrase_index.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        Catalog { stages, techniques, datasets, phrase_index, code_index, by_id }
    }

    pub fn get(&self, id: &str) -> Option<&Technique> {
        self.by_id.get(id).map(|i| &self.techniques[*i])
    }

    pub fn stage_name(&self, id: &str) -> String {
        self.stages.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or_else(|| id.to_string())
    }

    /// Technique IDs whose phrases occur in this text (stemmed, whole-word, in order).
    pub fn match_text(&self, text: &str) -> Vec<String> {
        let toks = stems(text);
        let mut found = BTreeSet::new();
        let mut used = vec![false; toks.len()];
        for (phrase, idx) in &self.phrase_index {
            if phrase.len() > toks.len() { continue; }
            for start in 0..=toks.len() - phrase.len() {
                if used[start..start + phrase.len()].iter().any(|u| *u) { continue; }
                if toks[start..start + phrase.len()] == phrase[..] {
                    found.insert(self.techniques[*idx].id.clone());
                    for u in &mut used[start..start + phrase.len()] { *u = true; }
                }
            }
        }
        found.into_iter().collect()
    }

    /// True if this text supports a technique: one of its phrases occurs in it.
    pub fn text_supports(&self, text: &str, id: &str) -> bool {
        self.match_text(text).iter().any(|x| x == id)
    }

    /// Technique IDs for one line of code, given the names used on that line
    /// (calls, attributes, keyword arguments, imports).
    pub fn match_code(&self, names: &BTreeSet<String>) -> Vec<String> {
        let mut found = BTreeSet::new();
        for (parts, idx) in &self.code_index {
            if parts.iter().all(|p| names.contains(p)) { found.insert(self.techniques[*idx].id.clone()); }
        }
        found.into_iter().collect()
    }

    pub fn datasets_in(&self, text: &str) -> Vec<String> {
        let low = text.to_lowercase();
        let mut out = Vec::new();
        for d in &self.datasets {
            let dl = d.to_lowercase();
            if let Some(pos) = low.find(&dl) {
                let before = low[..pos].chars().last().map(|c| !c.is_alphanumeric()).unwrap_or(true);
                let after = low[pos + dl.len()..].chars().next().map(|c| !c.is_alphanumeric()).unwrap_or(true);
                if before && after { out.push(d.clone()); }
            }
        }
        out
    }

    /// Add or replace a project-local technique (the "name this unrecognised step" flow).
    pub fn save_local(trail: &Path, t: Technique) -> std::io::Result<()> {
        let path = trail.join("catalog.local.json");
        let mut list: Vec<Technique> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| serde_json::from_value(v["techniques"].clone()).ok())
            .unwrap_or_default();
        list.retain(|x| x.id != t.id);
        list.push(Technique { local: false, ..t });
        let v = serde_json::json!({ "techniques": list });
        std::fs::write(path, serde_json::to_string_pretty(&v)?)
    }
}

/// A plain-name to ID helper for techniques typed by a person or a chat model.
pub fn normalise_name(s: &str) -> String {
    s.split_whitespace().map(stem).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cat() -> Catalog {
        let f: File = serde_json::from_str(BUILTIN).unwrap();
        Catalog::build(f.stages, f.techniques, f.datasets)
    }
    #[test]
    fn paper_phrases_match_with_inflections() {
        let c = cat();
        let ids = c.match_text("Features were standardised and missing values were imputed with the median.");
        assert!(ids.contains(&"scale_standard".to_string()), "{:?}", ids);
        assert!(ids.contains(&"impute_simple".to_string()), "{:?}", ids);
        let ids = c.match_text("We balance classes with SMOTE and train a Random Forest.");
        assert!(ids.contains(&"oversample_smote".to_string()));
        assert!(ids.contains(&"model_rf".to_string()));
    }
    #[test]
    fn code_patterns_need_all_parts() {
        let c = cat();
        let names: BTreeSet<String> = ["map", "value_counts"].iter().map(|s| s.to_string()).collect();
        assert_eq!(c.match_code(&names), vec!["encode_frequency".to_string()]);
        let names: BTreeSet<String> = ["map"].iter().map(|s| s.to_string()).collect();
        assert!(c.match_code(&names).is_empty());
    }
    #[test]
    fn datasets_whole_word() {
        let c = cat();
        assert_eq!(c.datasets_in("We evaluate on the Titanic data."), vec!["Titanic".to_string()]);
        assert!(c.datasets_in("Irises are flowers").is_empty());
    }
}
