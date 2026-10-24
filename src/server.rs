//! The local web server: static UI plus a small JSON API on 127.0.0.1 only.

use crate::catalog::{Catalog, Technique};
use crate::extract::{self, Card};
use crate::readers;
use crate::store::{Job, ManualEntry, State};
use crate::util::{clip, now, slug, url_decode};
use crate::{compare, demo, pack, search};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Request, Response};

pub struct App {
    pub state: Option<State>,
    pub recent: Vec<String>,
    pub home: PathBuf,
    pub jobs_seq: u64,
}

pub type Shared = Arc<Mutex<App>>;

impl App {
    pub fn config_dir(&self) -> PathBuf { self.home.join(".researchtrail") }
    pub fn load_recent(&mut self) {
        self.recent = std::fs::read_to_string(self.config_dir().join("recent.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    }
    pub fn remember(&mut self, p: &Path) {
        let s = p.display().to_string();
        self.recent.retain(|r| r != &s);
        self.recent.insert(0, s);
        self.recent.truncate(8);
        let _ = std::fs::create_dir_all(self.config_dir());
        let _ = std::fs::write(self.config_dir().join("recent.json"), serde_json::to_string_pretty(&self.recent).unwrap());
    }
    pub fn open(&mut self, p: &Path) -> Result<(), String> {
        let st = State::open(p)?;
        self.remember(&st.root.clone());
        self.state = Some(st);
        Ok(())
    }
}

const INDEX: &str = include_str!("../web/index.html");
const APP_JS: &str = include_str!("../web/app.js");
const APP_CSS: &str = include_str!("../web/app.css");
const FAVICON: &str = include_str!("../web/favicon.svg");

fn header(k: &str, v: &str) -> Header { Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap() }

fn respond_text(req: Request, code: u16, ctype: &str, body: String) {
    let r = Response::from_string(body).with_status_code(code).with_header(header("Content-Type", ctype)).with_header(header("Cache-Control", "no-store"));
    let _ = req.respond(r);
}

fn ok(req: Request, v: Value) { respond_text(req, 200, "application/json; charset=utf-8", v.to_string()); }
fn fail(req: Request, code: u16, msg: &str) { respond_text(req, code, "application/json; charset=utf-8", json!({ "error": msg }).to_string()); }

fn query(url: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    if let Some(q) = url.split_once('?').map(|x| x.1) {
        for kv in q.split('&') {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            m.insert(url_decode(k), url_decode(v));
        }
    }
    m
}

fn body_json(req: &mut Request) -> Value {
    let mut s = String::new();
    let _ = req.as_reader().take(8 * 1024 * 1024).read_to_string(&mut s);
    serde_json::from_str(&s).unwrap_or(Value::Null)
}

pub fn handle(app: Shared, mut req: Request) {
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("/").to_string();
    let q = query(&url);
    let method = req.method().clone();
    if !matches!(method, Method::Get) {
        // Only accept writes from our own page: blocks other websites poking the local API.
        let origin = req.headers().iter().find(|h| h.field.equiv("Origin")).map(|h| h.value.as_str().to_string());
        if let Some(o) = origin { if !(o.starts_with("http://127.0.0.1") || o.starts_with("http://localhost")) { return fail(req, 403, "Requests from other sites are blocked."); } }
    }
    match (method, path.as_str()) {
        (Method::Get, "/") | (Method::Get, "/index.html") => respond_text(req, 200, "text/html; charset=utf-8", web("index.html").unwrap_or_else(|| INDEX.to_string())),
        (Method::Get, "/app.js") => respond_text(req, 200, "text/javascript; charset=utf-8", web("app.js").unwrap_or_else(|| APP_JS.to_string())),
        (Method::Get, "/app.css") => respond_text(req, 200, "text/css; charset=utf-8", web("app.css").unwrap_or_else(|| APP_CSS.to_string())),
        (Method::Get, "/favicon.svg") => respond_text(req, 200, "image/svg+xml", FAVICON.to_string()),
        (Method::Get, "/api/pulse") => {
            let a = app.lock().unwrap();
            let v = match &a.state { Some(s) => json!({ "open": true, "version": s.version, "jobs": active_jobs(s) }), None => json!({ "open": false, "version": 0 }) };
            ok(req, v)
        }
        (Method::Get, "/api/projects") => {
            let a = app.lock().unwrap();
            ok(req, json!({ "current": a.state.as_ref().map(|s| s.root.display().to_string()), "recent": a.recent, "demo": demo::demo_dir(&a.home).display().to_string() }))
        }
        (Method::Post, "/api/project/open") => {
            let b = body_json(&mut req);
            let p = b["path"].as_str().unwrap_or("").trim().trim_matches('"').to_string();
            if p.is_empty() { return fail(req, 400, "Type or paste a folder path."); }
            let mut a = app.lock().unwrap();
            match a.open(Path::new(&p)) { Ok(()) => ok(req, json!({ "ok": true })), Err(e) => fail(req, 400, &e) }
        }
        (Method::Post, "/api/project/demo") => {
            let home = app.lock().unwrap().home.clone();
            match demo::create(&home) {
                Ok(dir) => { let mut a = app.lock().unwrap(); match a.open(&dir) { Ok(()) => ok(req, json!({ "ok": true })), Err(e) => fail(req, 500, &e) } }
                Err(e) => fail(req, 500, &e),
            }
        }
        (Method::Post, "/api/project/pick") => match pick_folder() {
            Some(p) => ok(req, json!({ "path": p })),
            None => fail(req, 400, "No folder picker on this computer. Paste the folder path instead."),
        },
        (Method::Post, "/api/project/close") => { app.lock().unwrap().state = None; ok(req, json!({ "ok": true })) }
        (m, p) => {
            let mut a = app.lock().unwrap();
            if a.state.is_none() { drop(a); return fail(req, 409, "Open a project folder first."); }
            let seq = { a.jobs_seq += 1; a.jobs_seq };
            let st = a.state.as_mut().unwrap();
            let r = api(st, &app, m, p, &q, &mut req, seq);
            drop(a);
            match r { Ok(v) => ok(req, v), Err((code, msg)) => fail(req, code, &msg) }
        }
    }
}

/// While developing the UI, set RT_WEB_DIR to serve files from disk.
fn web(name: &str) -> Option<String> {
    let dir = std::env::var("RT_WEB_DIR").ok()?;
    std::fs::read_to_string(Path::new(&dir).join(name)).ok()
}

fn active_jobs(s: &State) -> Vec<&Job> {
    s.jobs.iter().filter(|j| j.finished.map(|f| now() - f < 20).unwrap_or(true)).collect()
}

type ApiResult = Result<Value, (u16, String)>;
fn bad(msg: impl Into<String>) -> (u16, String) { (400, msg.into()) }

fn card_light(c: &Card, cat: &Catalog) -> Value {
    let techniques: usize = c.stages.values().map(|l| l.len()).sum();
    json!({
        "id": c.id, "title": c.title, "cite": c.cite(), "authors": c.authors, "year": c.year, "link": c.link, "kind": c.kind,
        "canonical": c.canonical, "archive": c.archive, "added": c.added, "summary": c.summary, "techniques": techniques,
        "stages": c.stages.iter().map(|(k, v)| (k.clone(), v.iter().map(|e| cat.get(&e.technique).map(|t| t.name.clone()).unwrap_or(e.name.clone())).collect::<Vec<_>>())).collect::<BTreeMap<_, _>>(),
        "datasets": c.datasets.iter().map(|d| d.label.clone()).collect::<Vec<_>>(),
        "unverified": c.unverified.len(), "enriched_by": c.enriched_by, "related": c.related,
    })
}

fn api(st: &mut State, app: &Shared, m: Method, p: &str, q: &BTreeMap<String, String>, req: &mut Request, seq: u64) -> ApiResult {
    let parts: Vec<&str> = p.trim_start_matches('/').split('/').collect();
    match (m, parts.as_slice()) {
        (Method::Get, ["api", "state"]) => Ok(state_json(st)),
        (Method::Get, ["api", "papers"]) => Ok(json!(st.cards.iter().rev().map(|c| card_light(c, &st.catalog)).collect::<Vec<_>>())),
        (Method::Post, ["api", "papers"]) => {
            let b = body_json(req);
            let link = b["link"].as_str().unwrap_or("").trim().to_string();
            if link.is_empty() { return Err(bad("Paste a paper link first.")); }
            if readers::classify(&link) == "unknown" { return Err(bad("That doesn't look like a link. Paste a full address starting with https://, an arXiv ID like 2401.01234, or a DOI.")); }
            Ok(json!({ "job": start_ingest(st, app, seq, Input::Link(link), None) }))
        }
        (Method::Post, ["api", "papers", "pdf"]) => {
            let mut bytes = Vec::new();
            let _ = req.as_reader().take(60 * 1024 * 1024).read_to_end(&mut bytes);
            if !bytes.starts_with(b"%PDF") { return Err(bad("That file isn't a PDF.")); }
            let name = q.get("name").cloned().unwrap_or_else(|| "uploaded.pdf".into());
            Ok(json!({ "job": start_ingest(st, app, seq, Input::Pdf(bytes, name), None) }))
        }
        (Method::Get, ["api", "papers", id]) => {
            let c = st.cards.iter().find(|c| c.id == *id).ok_or((404, "Unknown paper.".into()))?;
            let mut v = serde_json::to_value(c).unwrap();
            v["cite"] = json!(c.cite());
            v["stage_names"] = json!(st.catalog.stages);
            v["technique_names"] = json!(c.stages.values().flatten().map(|e| (e.technique.clone(), st.catalog.get(&e.technique).map(|t| t.name.clone()).unwrap_or(e.name.clone()))).collect::<BTreeMap<_, _>>());
            Ok(v)
        }
        (Method::Delete, ["api", "papers", id]) => { let t = st.delete_card(id).map_err(bad)?; st.log("paper", format!("Removed \"{}\"", clip(&t, 60)), None); Ok(json!({ "ok": true })) }
        (Method::Post, ["api", "papers", id, "refetch"]) => {
            let c = st.cards.iter().find(|c| c.id == *id).ok_or((404, "Unknown paper.".into()))?;
            if c.kind == "demo" { return Err(bad("Demo papers are invented, so there's nothing to re-fetch.")); }
            if c.kind == "pdf-upload" { return Err(bad("This paper came from an uploaded PDF, which wasn't kept. Upload it again to reprocess.")); }
            let link = c.canonical.clone().map(|x| x.trim_start_matches("doi:").to_string()).filter(|x| !x.starts_with("github:")).unwrap_or(c.link.clone());
            Ok(json!({ "job": start_ingest(st, app, seq, Input::Link(link), Some(id.to_string())) }))
        }
        (Method::Post, ["api", "papers", id, "note"]) => {
            let b = body_json(req);
            let c = st.cards.iter_mut().find(|c| c.id == *id).ok_or((404, "Unknown paper.".into()))?;
            c.note = b["note"].as_str().unwrap_or("").to_string();
            let c = c.clone();
            st.save_card(&c).map_err(bad)?;
            Ok(json!({ "ok": true }))
        }
        (Method::Post, ["api", "papers", id, "entry"]) => {
            let b = body_json(req);
            let stage = b["stage"].as_str().unwrap_or("");
            let tech = b["technique"].as_str().unwrap_or("");
            let c = st.cards.iter_mut().find(|c| c.id == *id).ok_or((404, "Unknown paper.".into()))?;
            if let Some(l) = c.stages.get_mut(stage) { l.retain(|e| e.technique != tech); }
            c.updated = now();
            let c = c.clone();
            st.save_card(&c).map_err(bad)?;
            st.bump();
            Ok(json!({ "ok": true }))
        }
        (Method::Get, ["api", "papers", id, "prompt"]) => {
            let c = st.cards.iter().find(|c| c.id == *id).ok_or((404, "Unknown paper.".into()))?;
            let p = extract::enrich_prompt(c, &st.catalog);
            Ok(json!({ "prompt": p, "tokens": crate::util::tokens(&p) }))
        }
        (Method::Post, ["api", "papers", id, "answer"]) => {
            let b = body_json(req);
            let answer = b["answer"].as_str().unwrap_or("");
            let cat = &st.catalog;
            let c = st.cards.iter_mut().find(|c| c.id == *id).ok_or((404, "Unknown paper.".into()))?;
            let rep = extract::verify_and_merge(c, cat, answer, "chat").map_err(bad)?;
            let c = c.clone();
            st.save_card(&c).map_err(bad)?;
            st.log("paper", format!("Added {} checked item{} to \"{}\" from your chat", rep.added, if rep.added == 1 { "" } else { "s" }, clip(&c.title, 50)), Some(c.id.clone()));
            st.bump();
            Ok(serde_json::to_value(rep).unwrap())
        }
        (Method::Post, ["api", "papers", id, "local"]) => {
            if st.config.ollama_model.trim().is_empty() { return Err(bad("Set a local model name in Settings first (for example qwen2.5:3b).")); }
            let c = st.cards.iter().find(|c| c.id == *id).ok_or((404, "Unknown paper.".into()))?.clone();
            let prompt = extract::enrich_prompt(&c, &st.catalog);
            let model = st.config.ollama_model.clone();
            let jid = format!("j{}", seq);
            st.jobs.push(Job { id: jid.clone(), input: c.title.clone(), status: "thinking".into(), message: format!("Asking {} on this computer…", model), card: Some(c.id.clone()), started: now(), finished: None });
            st.bump();
            let app = app.clone();
            let cid = c.id.clone();
            let j2 = jid.clone();
            std::thread::spawn(move || {
                let res = extract::ask_ollama(&model, &prompt);
                let mut a = app.lock().unwrap();
                let Some(st) = a.state.as_mut() else { return };
                let msg = match res {
                    Ok(ans) => {
                        let cat = &st.catalog;
                        match st.cards.iter_mut().find(|c| c.id == cid) {
                            Some(card) => match extract::verify_and_merge(card, cat, &ans, "model") {
                                Ok(rep) => { let cc = card.clone(); let _ = st.save_card(&cc); format!("Added {} checked items; {} didn't match their passage", rep.added, rep.unverified.len()) }
                                Err(e) => e,
                            },
                            None => "The paper was removed.".into(),
                        }
                    }
                    Err(e) => e,
                };
                if let Some(j) = st.jobs.iter_mut().find(|j| j.id == j2) { j.status = if msg.starts_with("Added") { "done".into() } else { "failed".into() }; j.message = msg; j.finished = Some(now()); }
                st.bump();
            });
            Ok(json!({ "job": jid }))
        }
        (Method::Get, ["api", "search"]) => {
            let text = q.get("q").cloned().unwrap_or_default();
            let stage = q.get("stage").filter(|s| !s.is_empty()).map(|s| s.as_str());
            Ok(json!(search::search(&text, &st.cards, &st.catalog, stage, 30)))
        }
        (Method::Get, ["api", "compare"]) => {
            let stage = q.get("stage").cloned().unwrap_or_else(|| "features".into());
            Ok(serde_json::to_value(compare::build(st, &stage)).unwrap())
        }
        (Method::Post, ["api", "mine", "remove"]) | (Method::Post, ["api", "mine", "restore"]) => {
            let b = body_json(req);
            let key = format!("{}|{}", b["technique"].as_str().unwrap_or(""), b["path"].as_str().unwrap_or(""));
            if p.ends_with("remove") { st.overrides.removed.insert(key); } else { st.overrides.removed.remove(&key); }
            st.save_overrides();
            st.bump();
            Ok(json!({ "ok": true }))
        }
        (Method::Post, ["api", "mine", "add"]) => {
            let b = body_json(req);
            let e = ManualEntry { stage: b["stage"].as_str().unwrap_or("").into(), technique: b["technique"].as_str().unwrap_or("").into(), note: b["note"].as_str().unwrap_or("").into() };
            if st.catalog.get(&e.technique).is_none() { return Err(bad("Pick a technique from the list.")); }
            st.overrides.added.retain(|x| !(x.stage == e.stage && x.technique == e.technique));
            st.overrides.added.push(e);
            st.save_overrides();
            st.bump();
            Ok(json!({ "ok": true }))
        }
        (Method::Post, ["api", "catalog", "name"]) => {
            let b = body_json(req);
            let code_name = b["code"].as_str().unwrap_or("").trim().to_string();
            let name = b["name"].as_str().unwrap_or("").trim().to_string();
            let stage = b["stage"].as_str().unwrap_or("").trim().to_string();
            if code_name.is_empty() || name.is_empty() || !st.catalog.stages.iter().any(|s| s.id == stage) { return Err(bad("Give the step a name and pick its stage.")); }
            let t = Technique { id: format!("local_{}", slug(&name, 40).replace('-', "_")), name: name.clone(), stage, phrases: vec![name.to_lowercase()], code: vec![code_name.clone()], local: true };
            Catalog::save_local(&st.trail, t).map_err(|e| bad(e.to_string()))?;
            st.reanalyse_all();
            st.log("catalog", format!("Named {} as \"{}\"; it's recognised from now on", code_name, name), None);
            Ok(json!({ "ok": true }))
        }
        (Method::Get, ["api", "catalog"]) => Ok(json!({ "stages": st.catalog.stages, "techniques": st.catalog.techniques })),
        (Method::Get, ["api", "code"]) => {
            let current = st.current_files();
            let sup = st.history.superseded();
            let files: Vec<Value> = st.code.iter().map(|(path, (_, cf))| {
                let lin = st.history.lineage_of(path).map(|l| l.id.clone());
                let versions = st.history.snapshots.iter().filter(|s| &s.path == path).count();
                json!({ "path": path, "kind": cf.kind, "lines": cf.lines, "lineage": lin, "current": current.contains(path), "superseded": sup.contains(path),
                        "versions": versions, "never_run": cf.never_run, "skipped_cells": cf.skipped_cells, "error": cf.error,
                        "techniques": cf.hits.iter().map(|h| h.technique.clone()).collect::<std::collections::BTreeSet<_>>() })
            }).collect();
            let lineages: Vec<Value> = st.history.lineages.iter().map(|l| {
                let snaps: Vec<&crate::history::Snapshot> = st.history.snapshots.iter().filter(|s| s.lineage == l.id).collect();
                json!({ "id": l.id, "name": l.name, "members": l.members, "manual": l.manual, "versions": snaps.len(), "last": snaps.last().map(|s| s.time) })
            }).collect();
            Ok(json!({ "files": files, "lineages": lineages, "root": st.code_root().display().to_string() }))
        }
        (Method::Get, ["api", "history"]) => {
            let lin = q.get("lineage").cloned().unwrap_or_default();
            let snaps: Vec<&crate::history::Snapshot> = st.history.snapshots.iter().filter(|s| lin.is_empty() || s.lineage == lin).collect();
            Ok(json!(snaps.into_iter().rev().take(200).collect::<Vec<_>>()))
        }
        (Method::Get, ["api", "diff"]) => {
            let to = q.get("to").cloned().unwrap_or_default();
            let d = st.history.diff(q.get("from").map(|s| s.as_str()), &to).map_err(bad)?;
            Ok(json!(d.into_iter().map(|(k, t)| json!({ "k": k, "t": t })).collect::<Vec<_>>()))
        }
        (Method::Post, ["api", "restore"]) => {
            let b = body_json(req);
            let id = b["id"].as_str().unwrap_or("").to_string();
            let cat = Catalog::load(&st.trail);
            let path = st.history.restore(&id, &cat).map_err(bad)?;
            st.log("restore", format!("Restored {} to version {}", path, id), Some(id.clone()));
            st.scan_code(false);
            Ok(json!({ "ok": true, "path": path }))
        }
        (Method::Post, ["api", "lineage", "split"]) => {
            let b = body_json(req);
            st.history.split(b["path"].as_str().unwrap_or("")).map_err(bad)?;
            st.bump();
            Ok(json!({ "ok": true }))
        }
        (Method::Post, ["api", "lineage", "merge"]) => {
            let b = body_json(req);
            st.history.merge(b["into"].as_str().unwrap_or(""), b["from"].as_str().unwrap_or("")).map_err(bad)?;
            st.bump();
            Ok(json!({ "ok": true }))
        }
        (Method::Post, ["api", "pack"]) => {
            let b = body_json(req);
            let budget = b["budget"].as_u64().map(|x| x as usize).unwrap_or(st.config.budget);
            let stage = b["stage"].as_str().filter(|s| !s.is_empty());
            Ok(serde_json::to_value(pack::build(st, b["task"].as_str().unwrap_or(""), stage, budget)).unwrap())
        }
        (Method::Get, ["api", "settings"]) => Ok(serde_json::to_value(&st.config).unwrap()),
        (Method::Post, ["api", "settings"]) => {
            let b = body_json(req);
            let mut c = st.config.clone();
            if let Some(v) = b["unpaywall_email"].as_str() { c.unpaywall_email = v.trim().into(); }
            if let Some(v) = b["ollama_model"].as_str() { c.ollama_model = v.trim().into(); }
            if let Some(v) = b["budget"].as_u64() { c.budget = v as usize; }
            if let Some(v) = b["archive_links"].as_bool() { c.archive_links = v; }
            let code_changed = b["code_dir"].as_str().map(|v| v.trim().to_string()).filter(|v| !v.is_empty() && *v != c.code_dir);
            if let Some(d) = &code_changed {
                if !st.root.join(d).is_dir() { return Err(bad(format!("There's no folder \"{}\" inside this project.", d))); }
                c.code_dir = d.clone();
            }
            st.config = c;
            st.save_config();
            if code_changed.is_some() { let root = st.root.clone(); *st = State::open(&root).map_err(bad)?; }
            st.bump();
            Ok(json!({ "ok": true }))
        }
        _ => Err((404, "Not found.".into())),
    }
}

fn state_json(st: &State) -> Value {
    let cov = compare::coverage(st);
    let mut growth: Vec<Value> = Vec::new();
    let mut events: Vec<(u64, &str)> = st.cards.iter().map(|c| (c.added, "paper")).collect();
    events.extend(st.history.snapshots.iter().map(|s| (s.time, "version")));
    events.sort();
    let (mut p, mut v) = (0, 0);
    for (t, k) in events { if k == "paper" { p += 1 } else { v += 1 }; growth.push(json!({ "t": t, "papers": p, "versions": v })); }
    let mut timeline: Vec<Value> = st.cards.iter().map(|c| json!({ "time": c.added, "kind": "paper", "text": format!("Added \"{}\"", clip(&c.title, 70)), "link": c.id })).collect();
    timeline.extend(st.history.snapshots.iter().rev().take(80).map(|s| json!({ "time": s.time, "kind": "version", "text": format!("{}: {}", crate::history::file_name(&s.path), s.summary.join("; ")), "link": s.id })));
    timeline.extend(st.activity.iter().filter(|a| a.kind != "snapshot").map(|a| json!({ "time": a.time, "kind": a.kind, "text": a.text, "link": a.link })));
    timeline.sort_by(|a, b| b["time"].as_u64().cmp(&a["time"].as_u64()));
    timeline.truncate(60);
    let current = st.current_files();
    json!({
        "project": { "name": st.name(), "root": st.root.display().to_string(), "code_root": st.code_root().display().to_string() },
        "version": st.version,
        "counts": { "papers": st.cards.len(), "passages": st.cards.iter().map(|c| c.passages.len()).sum::<usize>(), "files": st.code.len(), "current": current.len(), "versions": st.history.snapshots.len(), "histories": st.history.lineages.len() },
        "coverage": cov,
        "stages": st.catalog.stages,
        "growth": growth,
        "timeline": timeline,
        "jobs": active_jobs(st),
        "config": { "ollama": !st.config.ollama_model.is_empty(), "unpaywall": !st.config.unpaywall_email.is_empty() },
    })
}

pub enum Input { Link(String), Pdf(Vec<u8>, String) }

fn start_ingest(st: &mut State, app: &Shared, seq: u64, input: Input, replace: Option<String>) -> String {
    let jid = format!("j{}", seq);
    let label = match &input { Input::Link(l) => l.clone(), Input::Pdf(_, n) => n.clone() };
    st.jobs.push(Job { id: jid.clone(), input: label, status: "reading".into(), message: "Reading the link…".into(), card: replace.clone(), started: now(), finished: None });
    st.bump();
    let email = st.config.unpaywall_email.clone();
    let archive = st.config.archive_links;
    let app = app.clone();
    let j = jid.clone();
    std::thread::spawn(move || ingest(app, j, input, replace, email, archive));
    jid
}

fn set_job(app: &Shared, id: &str, status: &str, message: &str, card: Option<String>) {
    let mut a = app.lock().unwrap();
    if let Some(st) = a.state.as_mut() {
        if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
            j.status = status.into();
            j.message = message.into();
            if card.is_some() { j.card = card; }
            if status == "done" || status == "failed" { j.finished = Some(now()); }
        }
        st.bump();
    }
}

fn ingest(app: Shared, jid: String, input: Input, replace: Option<String>, email: String, archive: bool) {
    let res = match input {
        Input::Link(l) => readers::read_link(&l, &email),
        Input::Pdf(bytes, name) => readers::pdf_sections(&bytes).map(|secs| readers::Source {
            title: name.trim_end_matches(".pdf").replace(['_', '-'], " "), kind: "pdf-upload".into(), link: String::new(), sections: secs,
            notes: vec!["From an uploaded PDF. The PDF itself was not kept.".into()], ..Default::default()
        }),
        // The source (bytes, HTML, text) is dropped at the end of this function: never written to disk.
    };
    let src = match res { Ok(s) => s, Err(e) => return set_job(&app, &jid, "failed", &e, None) };
    set_job(&app, &jid, "extracting", &format!("Read {} words. Pulling out the important parts…", src.words()), None);
    let mut a = app.lock().unwrap();
    let Some(st) = a.state.as_mut() else { return };
    if replace.is_none() {
        if let Some(existing) = st.find_card_by_canonical(&src.canonical, &src.link) {
            drop(a);
            return set_job(&app, &jid, "failed", "This paper is already in your library.", Some(existing));
        }
    }
    let mut ids = st.card_ids();
    if let Some(r) = &replace { ids.remove(r); }
    let mut card = extract::build_card(&src, &st.catalog, &ids);
    if let Some(r) = &replace {
        if let Some(old) = st.cards.iter().find(|c| &c.id == r).cloned() {
            if old.fingerprint == card.fingerprint && old.extractor == card.extractor {
                drop(a);
                return set_job(&app, &jid, "done", "Already up to date: the paper and the extractor haven't changed.", Some(old.id));
            }
            card.id = old.id.clone();
            card.added = old.added;
            card.note = old.note.clone();
            card.archive = old.archive.clone();
            card.reader_notes.push("Re-fetched and reprocessed. Items you added from a chat or model need to be re-run, because passage numbers changed.".into());
        }
        st.cards.retain(|c| &c.id != r);
    }
    if card.kind == "pdf-upload" { card.link = String::new(); }
    let _ = st.save_card(&card);
    let found: usize = card.stages.values().map(|l| l.len()).sum();
    let msg = format!("Kept {} passages and found {} technique{}. The source was not stored.", card.passages.len(), found, if found == 1 { "" } else { "s" });
    st.log("paper", format!("{} \"{}\"", if replace.is_some() { "Re-fetched" } else { "Added" }, clip(&card.title, 70)), Some(card.id.clone()));
    let needs_archive = archive && !src.persistent && !card.link.is_empty() && card.archive.is_none();
    let (cid, link) = (card.id.clone(), card.link.clone());
    st.cards.push(card);
    drop(a);
    set_job(&app, &jid, "done", &msg, Some(cid.clone()));
    if needs_archive {
        if let Some(arch) = readers::archive(&link) {
            let mut a = app.lock().unwrap();
            if let Some(st) = a.state.as_mut() {
                if let Some(c) = st.cards.iter_mut().find(|c| c.id == cid) { c.archive = Some(arch); let c = c.clone(); let _ = st.save_card(&c); }
                st.bump();
            }
        }
    }
}

/// A native folder picker where one exists (Windows, macOS, Linux with zenity).
fn pick_folder() -> Option<String> {
    use std::process::Command;
    let out = if cfg!(windows) {
        Command::new("powershell").args(["-NoProfile", "-STA", "-Command", "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description = 'Choose your research project folder'; if ($d.ShowDialog() -eq 'OK') { $d.SelectedPath }"]).output().ok()?
    } else if cfg!(target_os = "macos") {
        Command::new("osascript").args(["-e", "POSIX path of (choose folder with prompt \"Choose your research project folder\")"]).output().ok()?
    } else {
        Command::new("zenity").args(["--file-selection", "--directory", "--title=Choose your research project folder"]).output().ok()?
    };
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}
