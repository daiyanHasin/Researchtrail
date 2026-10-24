//! Turns a read source into a paper card: the "important things" we keep.
//!
//! Tier 1 (always): extractive rules. Sentences that mention a catalog technique,
//! a dataset, a metric with a value or a hyperparameter become key passages.
//! Nothing is generated, so nothing is invented.
//!
//! Tiers 2 and 3 (optional): a local model or the user's own chat reads the key
//! passages and answers in a fixed JSON shape. Every answer must cite a passage;
//! `verify` checks the passage really says it before anything reaches the card.

use crate::catalog::{normalise_name, Catalog};
use crate::readers::Source;
use crate::util::{clip, now, sentences, sha256_hex, slug, stems, word_count};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

pub const EXTRACTOR_VERSION: u32 = 1;
pub const PASSAGE_WORD_CAP: usize = 2000;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Passage {
    pub id: String,
    pub section: String,
    pub stage: String,
    pub text: String,
    #[serde(default)]
    pub techniques: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Fact {
    pub label: String,
    #[serde(default)]
    pub value: String,
    pub passage: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct StageEntry {
    pub technique: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub how: String,
    #[serde(default)]
    pub passages: Vec<String>,
    /// rules | chat | model | you
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Card {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub authors: Vec<String>,
    pub year: Option<i32>,
    pub link: String,
    pub canonical: Option<String>,
    pub kind: String,
    pub archive: Option<String>,
    pub commit: Option<String>,
    pub fingerprint: String,
    pub added: u64,
    pub updated: u64,
    pub extractor: u32,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub passages: Vec<Passage>,
    #[serde(default)]
    pub stages: BTreeMap<String, Vec<StageEntry>>,
    #[serde(default)]
    pub datasets: Vec<Fact>,
    #[serde(default)]
    pub metrics: Vec<Fact>,
    #[serde(default)]
    pub hyperparams: Vec<Fact>,
    #[serde(default)]
    pub unverified: Vec<String>,
    #[serde(default)]
    pub reader_notes: Vec<String>,
    #[serde(default)]
    pub related: Vec<String>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub source_words: usize,
    #[serde(default)]
    pub enriched_by: Vec<String>,
}

impl Card {
    pub fn passage(&self, id: &str) -> Option<&Passage> {
        self.passages.iter().find(|p| p.id == id)
    }
    pub fn first_author(&self) -> String {
        self.authors.first().map(|a| a.split_whitespace().last().unwrap_or(a).to_string()).unwrap_or_default()
    }
    pub fn cite(&self) -> String {
        let a = self.first_author();
        match (a.is_empty(), self.year) {
            (false, Some(y)) => format!("{} {}{}", a, if self.authors.len() > 1 { "et al. " } else { "" }, y),
            (false, None) => a,
            (true, Some(y)) => format!("{} ({})", clip(&self.title, 40), y),
            _ => clip(&self.title, 40),
        }
    }
}

/// Which pipeline stage a section heading talks about. None = context (intro,
/// related work, conclusion): techniques mentioned there are usually other people's.
pub fn section_stage(heading: &str) -> Option<&'static str> {
    let h = heading.to_lowercase();
    let h = h.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ');
    const CONTEXT: &[&str] = &["introduction", "related", "background", "literature", "conclusion", "future work", "limitation", "motivation", "overview", "front matter"];
    if CONTEXT.iter().any(|c| h.contains(c)) { return None; }
    let rules: &[(&[&str], &str)] = &[
        (&["preprocess", "pre-process", "cleaning", "data preparation", "imbalanc", "resampl"], "preprocessing"),
        (&["feature"], "features"),
        (&["dataset", "data set", "data collection", "data description", "data"], "data"),
        (&["training", "implementation", "hyperparameter", "hyper-parameter", "experimental setup", "setup", "optimi"], "training"),
        (&["evaluation", "metric", "result", "experiment", "discussion", "ablation", "comparison", "performance"], "evaluation"),
        (&["model", "method", "approach", "architecture", "proposed", "framework", "algorithm"], "model"),
    ];
    for (keys, stage) in rules {
        if keys.iter().any(|k| h.contains(k)) { return Some(stage); }
    }
    Some("method")
}

static METRIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(accuracy|precision|recall|f1(?:[- ]score)?|f-measure|roc[- ]auc|auc(?:[- ]roc)?|auprc|pr[- ]auc|average precision|rmse|mae|mse|r-squared|mcc|log[- ]loss|g-mean|specificity|sensitivity|brier score)\b[^.\d]{0,40}?(\d+(?:\.\d+)?\s?%?)").unwrap());
static HPARAM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(learning rate|batch size|epochs?|dropout(?: rate)?|weight decay|momentum|max(?:imum)? depth|n_estimators|number of (?:trees|estimators|layers|epochs|neighbou?rs)|hidden (?:size|units|layers)|embedding (?:size|dimension))\b[^.\d]{0,25}?(\d+(?:\.\d+)?(?:e-?\d+)?)").unwrap());
static DATASET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b([A-Z][A-Za-z0-9\-]+(?:\s[A-Z0-9][A-Za-z0-9\-]+){0,3})\s(?:dataset|data set|benchmark)\b").unwrap());

fn metrics_in(s: &str) -> Vec<(String, String)> {
    METRIC.captures_iter(s).filter_map(|c| {
        let v = c[2].trim().to_string();
        let num: f64 = v.trim_end_matches('%').trim().parse().ok()?;
        if num > 100.0 || (num >= 1900.0) { return None; }
        Some((c[1].to_string(), v))
    }).collect()
}

fn hparams_in(s: &str) -> Vec<(String, String)> {
    HPARAM.captures_iter(s).map(|c| (c[1].to_lowercase(), c[2].to_string())).collect()
}

fn datasets_in(cat: &Catalog, s: &str) -> Vec<String> {
    let mut out = cat.datasets_in(s);
    for c in DATASET.captures_iter(s) {
        let d = c[1].to_string();
        let first = d.split_whitespace().next().unwrap_or("");
        if ["The", "This", "Our", "Each", "A", "An", "In", "Both", "These", "Its", "Their"].contains(&first) { continue; }
        if !out.iter().any(|x| x.eq_ignore_ascii_case(&d)) { out.push(d); }
    }
    out
}

/// Build a card from a source using the rules tier.
pub fn build_card(src: &Source, cat: &Catalog, existing_ids: &BTreeSet<String>) -> Card {
    let all_text: String = src.sections.iter().map(|s| format!("{}\n{}\n", s.heading, s.text)).collect();
    let mut card = Card {
        title: if src.title.is_empty() { "Untitled paper".into() } else { src.title.clone() },
        authors: src.authors.clone(),
        year: src.year,
        link: src.link.clone(),
        canonical: src.canonical.clone(),
        kind: src.kind.clone(),
        commit: src.commit.clone(),
        fingerprint: format!("sha256:{}", sha256_hex(all_text.as_bytes())),
        added: now(),
        updated: now(),
        extractor: EXTRACTOR_VERSION,
        reader_notes: src.notes.clone(),
        related: src.related.clone(),
        source_words: src.words(),
        ..Default::default()
    };
    card.id = unique_id(&card, existing_ids);

    struct Cand { section: String, stage: String, counts: bool, text: String, techniques: Vec<String>, metrics: Vec<(String, String)>, hparams: Vec<(String, String)>, datasets: Vec<String>, score: usize, order: usize }
    let mut cands: Vec<Cand> = Vec::new();
    let mut order = 0;
    for sec in &src.sections {
        let hint = section_stage(&sec.heading);
        let is_abstract = sec.heading.to_lowercase().contains("abstract");
        let counts = hint.is_some() || is_abstract;
        for (i, s) in sentences(&sec.text).into_iter().enumerate() {
            order += 1;
            if word_count(&s) < 5 || word_count(&s) > 90 { continue; }
            let techniques = cat.match_text(&s);
            let metrics = metrics_in(&s);
            let hparams = hparams_in(&s);
            let datasets = datasets_in(cat, &s);
            let facts = techniques.len() * 2 + metrics.len() + hparams.len() + datasets.len();
            let summary = is_abstract && i < 2;
            if facts == 0 && !summary { continue; }
            if is_abstract && i < 2 && card.summary.split_whitespace().count() < 80 {
                if !card.summary.is_empty() { card.summary.push(' '); }
                card.summary.push_str(&s);
            }
            let stage = if let Some(t) = techniques.first().and_then(|t| cat.get(t)) { t.stage.clone() }
                else if !metrics.is_empty() { "evaluation".into() }
                else if !hparams.is_empty() { "training".into() }
                else if !datasets.is_empty() { "data".into() }
                else { hint.unwrap_or("context").to_string() };
            let stage = if counts { stage } else { "context".into() };
            cands.push(Cand { section: sec.heading.clone(), stage, counts, text: s, techniques, metrics, hparams, datasets, score: facts + if counts { 2 } else { 0 }, order });
        }
    }
    // Keep the most informative sentences within the word cap, then restore reading order.
    let mut ranked: Vec<usize> = (0..cands.len()).collect();
    ranked.sort_by(|a, b| cands[*b].score.cmp(&cands[*a].score).then(cands[*a].order.cmp(&cands[*b].order)));
    let mut keep = BTreeSet::new();
    let mut words = 0;
    for i in ranked {
        let w = word_count(&cands[i].text);
        if words + w > PASSAGE_WORD_CAP { continue; }
        words += w;
        keep.insert(i);
    }
    let mut n = 0;
    for (i, c) in cands.iter().enumerate() {
        if !keep.contains(&i) { continue; }
        n += 1;
        let pid = format!("k{}", n);
        if c.counts {
            for t in &c.techniques {
                let tech = cat.get(t).unwrap();
                let list = card.stages.entry(tech.stage.clone()).or_default();
                if let Some(e) = list.iter_mut().find(|e| &e.technique == t) {
                    if e.passages.len() < 4 { e.passages.push(pid.clone()); }
                } else {
                    list.push(StageEntry { technique: t.clone(), name: tech.name.clone(), how: c.text.clone(), passages: vec![pid.clone()], source: "rules".into() });
                }
            }
            for (label, value) in &c.metrics { card.metrics.push(Fact { label: label.clone(), value: value.clone(), passage: pid.clone() }); }
            for (label, value) in &c.hparams { card.hyperparams.push(Fact { label: label.clone(), value: value.clone(), passage: pid.clone() }); }
            for d in &c.datasets { if !card.datasets.iter().any(|x| x.label.eq_ignore_ascii_case(d)) { card.datasets.push(Fact { label: d.clone(), value: String::new(), passage: pid.clone() }); } }
        }
        card.passages.push(Passage { id: pid, section: c.section.clone(), stage: c.stage.clone(), text: c.text.clone(), techniques: c.techniques.clone() });
    }
    if card.summary.is_empty() {
        card.summary = src.sections.iter().find(|s| !s.text.is_empty()).map(|s| sentences(&s.text).into_iter().take(2).collect::<Vec<_>>().join(" ")).unwrap_or_default();
    }
    card
}

fn unique_id(card: &Card, existing: &BTreeSet<String>) -> String {
    let base = {
        let a = card.first_author();
        let first_word = card.title.split_whitespace().find(|w| w.len() > 3).unwrap_or("paper");
        slug(&format!("{} {} {}", a, card.year.map(|y| y.to_string()).unwrap_or_default(), first_word), 40)
    };
    if !existing.contains(&base) { return base; }
    (2..).map(|i| format!("{}-{}", base, i)).find(|c| !existing.contains(c)).unwrap()
}

// ---------- tiers 2 and 3: model or chat, then verification ----------

pub fn enrich_prompt(card: &Card, cat: &Catalog) -> String {
    let mut p = String::new();
    p.push_str("You are helping a student build a structured card for one research paper.\n");
    p.push_str("Read ONLY the numbered passages below. Do not use outside knowledge.\n\n");
    p.push_str("Return ONLY a JSON object, no other text, in this shape:\n");
    p.push_str("{\"stages\": {\"<stage id>\": [{\"technique\": \"<technique id from the list, or a short new name>\", \"how\": \"<one sentence: how the paper applied it>\", \"passage\": \"<passage id like k3>\"}]},\n");
    p.push_str(" \"datasets\": [{\"name\": \"...\", \"passage\": \"k1\"}],\n \"metrics\": [{\"name\": \"...\", \"value\": \"<number as written>\", \"passage\": \"k9\"}],\n \"hyperparameters\": [{\"name\": \"...\", \"value\": \"...\", \"passage\": \"k5\"}]}\n\n");
    p.push_str("Rules: every item must cite exactly one passage that states it. Only include what THIS paper did, not prior work. Skip anything you are unsure about.\n\n");
    p.push_str("Stage ids: data, preprocessing, features, model, training, evaluation.\n\nKnown technique ids:\n");
    for s in &cat.stages {
        let ids: Vec<String> = cat.techniques.iter().filter(|t| t.stage == s.id).map(|t| t.id.clone()).collect();
        p.push_str(&format!("- {}: {}\n", s.id, ids.join(", ")));
    }
    p.push_str(&format!("\nPaper: {}\n\nPassages:\n", card.title));
    for ps in &card.passages {
        p.push_str(&format!("[{}] ({}) {}\n", ps.id, ps.section, ps.text));
    }
    p
}

#[derive(Debug, Serialize, Default)]
pub struct VerifyReport {
    pub added: usize,
    pub already: usize,
    pub unverified: Vec<String>,
}

fn extract_json(text: &str) -> Option<serde_json::Value> {
    let t = text.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    if let Ok(v) = serde_json::from_str(t) { return Some(v); }
    let start = t.find('{')?;
    let end = t.rfind('}')?;
    serde_json::from_str(&t[start..=end]).ok()
}

/// Words of a name appear in the passage (stemmed). Used for new technique names and datasets.
fn words_supported(name: &str, passage: &str) -> bool {
    let pw: BTreeSet<String> = stems(passage).into_iter().collect();
    let nw: Vec<String> = stems(name).into_iter().filter(|w| w.len() > 2).collect();
    if nw.is_empty() { return false; }
    let hit = nw.iter().filter(|w| pw.contains(*w)).count();
    hit * 10 >= nw.len() * 6
}

pub fn verify_and_merge(card: &mut Card, cat: &Catalog, answer: &str, source: &str) -> Result<VerifyReport, String> {
    let v = extract_json(answer).ok_or("That doesn't look like the JSON the prompt asked for. Paste only the JSON object the chat returned.")?;
    let mut rep = VerifyReport::default();
    let valid_stages: Vec<String> = cat.stages.iter().map(|s| s.id.clone()).collect();
    if let Some(stages) = v["stages"].as_object() {
        for (stage, items) in stages {
            if !valid_stages.contains(stage) { rep.unverified.push(format!("Unknown stage \"{}\"", stage)); continue; }
            for it in items.as_array().cloned().unwrap_or_default() {
                let tech = it["technique"].as_str().unwrap_or("").trim().to_string();
                let pid = it["passage"].as_str().unwrap_or("").trim().to_string();
                let how = it["how"].as_str().unwrap_or("").trim().to_string();
                if tech.is_empty() { continue; }
                let Some(ps) = card.passage(&pid).cloned() else { rep.unverified.push(format!("{}: cites passage \"{}\", which doesn't exist", tech, pid)); continue; };
                let (id, name, ok) = if let Some(t) = cat.get(&tech) {
                    let ok = cat.text_supports(&ps.text, &t.id) || words_supported(&t.name, &ps.text);
                    (t.id.clone(), t.name.clone(), ok && &t.stage == stage)
                } else if let Some(t) = cat.techniques.iter().find(|t| normalise_name(&t.name) == normalise_name(&tech)) {
                    (t.id.clone(), t.name.clone(), cat.text_supports(&ps.text, &t.id) || words_supported(&t.name, &ps.text))
                } else {
                    (format!("new:{}", slug(&tech, 40)), tech.clone(), words_supported(&tech, &ps.text))
                };
                if !ok { rep.unverified.push(format!("{} ({}): passage {} doesn't say this", name, stage, pid)); continue; }
                let list = card.stages.entry(stage.clone()).or_default();
                if let Some(e) = list.iter_mut().find(|e| e.technique == id) {
                    rep.already += 1;
                    if !e.passages.contains(&pid) { e.passages.push(pid); }
                } else {
                    list.push(StageEntry { technique: id, name, how: if how.is_empty() { ps.text.clone() } else { how }, passages: vec![pid], source: source.into() });
                    rep.added += 1;
                }
            }
        }
    }
    for (key, target) in [("datasets", 0), ("metrics", 1), ("hyperparameters", 2)] {
        for it in v[key].as_array().cloned().unwrap_or_default() {
            let name = it["name"].as_str().unwrap_or("").trim().to_string();
            let value = it["value"].as_str().map(|s| s.to_string()).or_else(|| it["value"].as_f64().map(|f| f.to_string())).unwrap_or_default();
            let pid = it["passage"].as_str().unwrap_or("").to_string();
            if name.is_empty() { continue; }
            let Some(ps) = card.passage(&pid) else { rep.unverified.push(format!("{}: cites a missing passage", name)); continue; };
            let ok = words_supported(&name, &ps.text) && (value.is_empty() || ps.text.contains(value.trim_end_matches('%').trim()));
            if !ok { rep.unverified.push(format!("{} {}: passage {} doesn't state it", name, value, pid)); continue; }
            let list = match target { 0 => &mut card.datasets, 1 => &mut card.metrics, _ => &mut card.hyperparams };
            if list.iter().any(|f| f.label.eq_ignore_ascii_case(&name) && f.value == value) { rep.already += 1; continue; }
            list.push(Fact { label: name, value, passage: pid });
            rep.added += 1;
        }
    }
    for u in &rep.unverified { if !card.unverified.contains(u) { card.unverified.push(u.clone()); } }
    if !card.enriched_by.contains(&source.to_string()) { card.enriched_by.push(source.into()); }
    card.updated = now();
    Ok(rep)
}

/// Ask a local Ollama model (tier 2). Returns the raw answer for `verify_and_merge`.
pub fn ask_ollama(model: &str, prompt: &str) -> Result<String, String> {
    let body = serde_json::json!({ "model": model, "prompt": prompt, "stream": false, "format": "json", "options": { "temperature": 0 } });
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(900)).build();
    let resp = agent.post("http://127.0.0.1:11434/api/generate").send_json(body)
        .map_err(|e| format!("Couldn't reach Ollama on this computer ({}). Install it from ollama.com and run `ollama pull {}`.", e, model))?;
    let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
    v["response"].as_str().map(|s| s.to_string()).ok_or("Ollama returned no answer.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::readers::Section;
    fn cat() -> Catalog { Catalog::load(std::path::Path::new("/nonexistent")) }
    fn src() -> Source {
        Source {
            title: "Fraud detection with boosting".into(),
            authors: vec!["Ana Smith".into(), "B Jones".into()],
            year: Some(2024),
            link: "https://example.org/p".into(),
            kind: "web".into(),
            sections: vec![
                Section { heading: "Abstract".into(), text: "We study fraud detection on card transactions. Our XGBoost model reaches an AUC of 0.94.".into() },
                Section { heading: "2 Related Work".into(), text: "Earlier studies used a support vector machine and logistic regression.".into() },
                Section { heading: "3.1 Data Preprocessing".into(), text: "Missing values were imputed with the median. We applied SMOTE to the training set only.".into() },
                Section { heading: "3.2 Feature Engineering".into(), text: "We created frequency encoding features for card identifiers and aggregation features per user.".into() },
                Section { heading: "4 Experiments".into(), text: "The learning rate was 0.05 with early stopping. We report precision of 0.81 and recall of 0.77 using 5-fold cross-validation.".into() },
            ],
            ..Default::default()
        }
    }
    #[test]
    fn rules_card_keeps_own_methods_not_related_work() {
        let c = build_card(&src(), &cat(), &BTreeSet::new());
        let has = |stage: &str, id: &str| c.stages.get(stage).map(|l| l.iter().any(|e| e.technique == id)).unwrap_or(false);
        assert!(has("preprocessing", "oversample_smote"));
        assert!(has("preprocessing", "impute_simple"));
        assert!(has("features", "encode_frequency"));
        assert!(has("features", "feat_aggregation"));
        assert!(has("model", "model_xgboost"));
        assert!(has("evaluation", "cv_kfold"));
        assert!(!has("model", "model_svm"), "related-work SVM must not count as this paper's method");
        assert!(c.metrics.iter().any(|m| m.value == "0.94"));
        assert!(c.hyperparams.iter().any(|h| h.label == "learning rate" && h.value == "0.05"));
        assert_eq!(c.id, "smith-2024-fraud");
    }
    #[test]
    fn verification_rejects_unsupported_claims() {
        let cat = cat();
        let mut c = build_card(&src(), &cat, &BTreeSet::new());
        let smote = c.passages.iter().find(|p| p.text.contains("SMOTE")).unwrap().id.clone();
        let answer = format!(r#"{{"stages": {{"preprocessing": [
            {{"technique": "undersample", "how": "x", "passage": "{0}"}},
            {{"technique": "Training-only resampling", "how": "SMOTE on train only", "passage": "{0}"}}
        ]}}, "metrics": [{{"name": "precision", "value": "0.99", "passage": "{0}"}}]}}"#, smote);
        let rep = verify_and_merge(&mut c, &cat, &answer, "chat").unwrap();
        assert_eq!(rep.added, 1, "{:?}", rep);
        assert_eq!(rep.unverified.len(), 2, "{:?}", rep);
    }
}
