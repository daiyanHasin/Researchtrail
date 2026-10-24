//! Link readers. Given a link the user pasted, find the cleanest legal copy of the
//! paper and return its sections. Readers are tried in a fixed order:
//!
//!   1. arXiv link or ID  -> arXiv's HTML version, then its PDF
//!   2. DOI               -> Crossref metadata, Unpaywall's open copy, then the landing page
//!   3. GitHub repo       -> README through the GitHub API (+ any paper it links)
//!   4. any web page      -> citation meta tags, then the main text
//!   5. PDF               -> text with heading detection (fallback only)
//!
//! Nothing read here is written to disk. The caller turns it into a paper card.

use crate::util::{clean_space, word_count};
use regex::Regex;
use scraper::{ElementRef, Html, Node, Selector};
use std::io::Read;
use std::sync::LazyLock;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct Section {
    pub heading: String,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Source {
    pub title: String,
    pub authors: Vec<String>,
    pub year: Option<i32>,
    pub link: String,
    pub canonical: Option<String>,
    pub kind: String,
    pub sections: Vec<Section>,
    pub related: Vec<String>,
    pub commit: Option<String>,
    pub persistent: bool,
    pub notes: Vec<String>,
}

impl Source {
    pub fn words(&self) -> usize {
        self.sections.iter().map(|s| word_count(&s.text)).sum()
    }
}

const UA: &str = "ResearchTrail/0.1 (local research notebook; reads one page a user chose)";
const MAX_BYTES: u64 = 40 * 1024 * 1024;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout(Duration::from_secs(60)).user_agent(UA).redirects(8).build()
}

struct Fetched {
    url: String,
    content_type: String,
    body: Vec<u8>,
}

fn fetch(url: &str, accept: Option<&str>) -> Result<Fetched, String> {
    let mut req = agent().get(url);
    if let Some(a) = accept { req = req.set("Accept", a); }
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, _)) => return Err(format!("{} answered {}", host(url), code)),
        Err(e) => return Err(format!("Could not reach {}: {}", host(url), short_err(&e.to_string()))),
    };
    let final_url = resp.get_url().to_string();
    let ct = resp.header("content-type").unwrap_or("").to_lowercase();
    let mut body = Vec::new();
    resp.into_reader().take(MAX_BYTES).read_to_end(&mut body).map_err(|e| format!("Reading {} failed: {}", host(url), e))?;
    Ok(Fetched { url: final_url, content_type: ct, body })
}

fn fetch_json(url: &str) -> Result<serde_json::Value, String> {
    let f = fetch(url, Some("application/json"))?;
    serde_json::from_slice(&f.body).map_err(|_| format!("{} did not return JSON", host(url)))
}

fn host(url: &str) -> String {
    url.split("//").nth(1).unwrap_or(url).split('/').next().unwrap_or(url).to_string()
}

fn short_err(s: &str) -> String {
    s.split(':').last().unwrap_or(s).trim().to_string()
}

static ARXIV: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?:arxiv\.org/(?:abs|pdf|html)/|arxiv:\s*|10\.48550/arxiv\.)(\d{4}\.\d{4,5})(v\d+)?").unwrap());
static ARXIV_BARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(\d{4}\.\d{4,5})(v\d+)?\s*$").unwrap());
static DOI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(10\.\d{4,9}/[^\s?#]+)").unwrap());
static GITHUB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)github\.com/([A-Za-z0-9_.\-]+)/([A-Za-z0-9_.\-]+)").unwrap());

/// What kind of link this is, without fetching anything.
pub fn classify(input: &str) -> &'static str {
    let s = input.trim();
    if ARXIV.is_match(s) || ARXIV_BARE.is_match(s) { return "arxiv"; }
    if GITHUB.is_match(s) { return "github"; }
    if DOI.is_match(s) && (s.contains("doi.org") || s.starts_with("10.") || s.to_lowercase().starts_with("doi:")) { return "doi"; }
    if s.starts_with("http://") || s.starts_with("https://") { return "web"; }
    "unknown"
}

pub fn read_link(input: &str, unpaywall_email: &str) -> Result<Source, String> {
    let s = input.trim();
    match classify(s) {
        "arxiv" => {
            let caps = ARXIV.captures(s).or_else(|| ARXIV_BARE.captures(s)).unwrap();
            read_arxiv(&caps[1], caps.get(2).map(|m| m.as_str()))
        }
        "github" => {
            let caps = GITHUB.captures(s).unwrap();
            read_github(&caps[1], caps[2].trim_end_matches(".git"))
        }
        "doi" => {
            let doi = DOI.captures(s).unwrap()[1].trim_end_matches(['.', ')', ',']).to_string();
            read_doi(&doi, unpaywall_email)
        }
        "web" => read_web(s),
        _ => Err("That doesn't look like a link. Paste a full address starting with https://, an arXiv ID like 2401.01234, or a DOI like 10.1000/xyz.".into()),
    }
}

// ---------- arXiv ----------

fn read_arxiv(id: &str, version: Option<&str>) -> Result<Source, String> {
    let mut src = Source { kind: "arxiv".into(), persistent: true, ..Default::default() };
    let mut latest = None;
    if let Ok(f) = fetch(&format!("https://export.arxiv.org/api/query?id_list={}", id), None) {
        let xml = String::from_utf8_lossy(&f.body).to_string();
        if let Some(entry) = xml.split("<entry>").nth(1) {
            src.title = tag(entry, "title").map(|t| clean_space(&t)).unwrap_or_default();
            src.authors = all_tags(entry, "name");
            src.year = tag(entry, "published").and_then(|p| p.get(0..4).and_then(|y| y.parse().ok()));
            latest = tag(entry, "id").and_then(|u| u.rsplit('/').next().map(|x| x.to_string())).and_then(|x| x.find('v').map(|p| x[p..].to_string()));
        }
    }
    let v = version.map(|x| x.to_string()).or(latest).unwrap_or_default();
    let full = format!("{}{}", id, v);
    src.link = format!("https://arxiv.org/abs/{}", full);
    src.canonical = Some(format!("arXiv:{}", full));

    match fetch(&format!("https://arxiv.org/html/{}", full), Some("text/html")) {
        Ok(f) if f.content_type.contains("html") => {
            let doc = Html::parse_document(&String::from_utf8_lossy(&f.body));
            src.sections = html_sections(&doc);
            if src.title.is_empty() { src.title = meta_title(&doc); }
        }
        _ => {}
    }
    if src.words() < 300 {
        src.notes.push("arXiv has no HTML version of this paper, so the PDF was read instead. Section detection may be less precise.".into());
        let f = fetch(&format!("https://arxiv.org/pdf/{}", full), Some("application/pdf"))?;
        src.sections = pdf_sections(&f.body)?;
    }
    if src.title.is_empty() { src.title = format!("arXiv {}", full); }
    Ok(src)
}

fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{}", name);
    let start = xml.find(&open)?;
    let body_start = start + xml[start..].find('>')? + 1;
    let end = body_start + xml[body_start..].find(&format!("</{}>", name))?;
    Some(decode_entities(xml[body_start..end].trim()))
}

fn all_tags(xml: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(v) = tag(rest, name) {
        out.push(v);
        let close = format!("</{}>", name);
        match rest.find(&close) { Some(p) => rest = &rest[p + close.len()..], None => break }
    }
    out
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'")
}

// ---------- DOI ----------

fn read_doi(doi: &str, email: &str) -> Result<Source, String> {
    if let Some(c) = ARXIV.captures(doi) { return read_arxiv(&c[1].to_string(), c.get(2).map(|m| m.as_str())); }
    let mut meta = Source { kind: "doi".into(), persistent: true, canonical: Some(format!("doi:{}", doi)), link: format!("https://doi.org/{}", doi), ..Default::default() };
    if let Ok(v) = fetch_json(&format!("https://api.crossref.org/works/{}", doi)) {
        let m = &v["message"];
        meta.title = m["title"][0].as_str().unwrap_or("").to_string();
        meta.authors = m["author"].as_array().map(|a| a.iter().map(|p| format!("{} {}", p["given"].as_str().unwrap_or(""), p["family"].as_str().unwrap_or("")).trim().to_string()).collect()).unwrap_or_default();
        meta.year = m["issued"]["date-parts"][0][0].as_i64().map(|y| y as i32);
    }
    let mut candidates: Vec<String> = Vec::new();
    if !email.trim().is_empty() {
        if let Ok(v) = fetch_json(&format!("https://api.unpaywall.org/v2/{}?email={}", doi, email.trim())) {
            let best = &v["best_oa_location"];
            for k in ["url_for_landing_page", "url", "url_for_pdf"] {
                if let Some(u) = best[k].as_str() { if !candidates.iter().any(|c| c == u) { candidates.push(u.to_string()); } }
            }
            if best.is_null() { meta.notes.push("Unpaywall found no free copy of this paper. Trying the publisher's page.".into()); }
        }
    } else {
        meta.notes.push("Tip: add an email for Unpaywall in Settings to find free copies of DOI papers more often.".into());
    }
    candidates.push(format!("https://doi.org/{}", doi));
    let mut last_err = String::new();
    for c in candidates {
        match read_web(&c) {
            Ok(mut s) if s.words() >= 300 => {
                s.kind = "doi".into();
                s.persistent = true;
                s.canonical = meta.canonical.clone();
                s.link = meta.link.clone();
                if !meta.title.is_empty() { s.title = meta.title.clone(); }
                if !meta.authors.is_empty() { s.authors = meta.authors.clone(); }
                if meta.year.is_some() { s.year = meta.year; }
                s.notes.extend(meta.notes.clone());
                return Ok(s);
            }
            Ok(_) => last_err = "The page only showed an abstract; the full text may be paywalled.".into(),
            Err(e) => last_err = e,
        }
    }
    Err(format!("No free full text found for this DOI. {} If you have the PDF, add it with Upload PDF.", last_err))
}

// ---------- GitHub ----------

fn read_github(owner: &str, repo: &str) -> Result<Source, String> {
    let api = format!("https://api.github.com/repos/{}/{}", owner, repo);
    let info = fetch_json(&api).map_err(|e| format!("GitHub: {}", e))?;
    let branch = info["default_branch"].as_str().unwrap_or("main").to_string();
    let readme = fetch(&format!("{}/readme", api), Some("application/vnd.github.raw+json")).map_err(|e| format!("GitHub README: {}", e))?;
    let md = String::from_utf8_lossy(&readme.body).to_string();
    let commit = fetch(&format!("{}/commits/{}", api, branch), Some("application/vnd.github.sha")).ok().map(|f| String::from_utf8_lossy(&f.body).trim().to_string()).filter(|s| s.len() == 40);
    let mut src = Source {
        kind: "github".into(),
        title: info["description"].as_str().filter(|d| !d.is_empty()).map(|d| format!("{}/{}: {}", owner, repo, d)).unwrap_or(format!("{}/{}", owner, repo)),
        authors: vec![owner.to_string()],
        year: info["created_at"].as_str().and_then(|d| d.get(0..4)).and_then(|y| y.parse().ok()),
        link: format!("https://github.com/{}/{}", owner, repo),
        canonical: commit.as_ref().map(|c| format!("github:{}/{}@{}", owner, repo, &c[..12])),
        sections: markdown_sections(&md),
        commit,
        ..Default::default()
    };
    for c in ARXIV.captures_iter(&md) { let id = format!("arXiv:{}{}", &c[1], c.get(2).map(|m| m.as_str()).unwrap_or("")); if !src.related.contains(&id) { src.related.push(id); } }
    for c in DOI.captures_iter(&md) {
        let d = c[1].trim_end_matches(['.', ')', ',', ']']).to_string();
        if !d.to_lowercase().contains("arxiv") && !src.related.contains(&d) { src.related.push(d); }
    }
    if !src.related.is_empty() { src.notes.push(format!("This README links {} paper{}; you can add {} too.", src.related.len(), if src.related.len() == 1 { "" } else { "s" }, if src.related.len() == 1 { "it" } else { "them" })); }
    Ok(src)
}

pub fn markdown_sections(md: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut cur = Section { heading: "Overview".into(), text: String::new() };
    let mut in_code = false;
    for line in md.lines() {
        let t = line.trim();
        if t.starts_with("```") { in_code = !in_code; continue; }
        if in_code { continue; }
        if t.starts_with('#') {
            if !cur.text.trim().is_empty() { out.push(std::mem::take(&mut cur)); }
            cur.heading = t.trim_start_matches('#').trim().to_string();
            continue;
        }
        let t = strip_md(t);
        if t.is_empty() { cur.text.push_str("\n\n"); } else { cur.text.push_str(&t); cur.text.push(' '); }
    }
    if !cur.text.trim().is_empty() { out.push(cur); }
    for s in &mut out { s.text = s.text.split("\n\n").map(clean_space).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n"); }
    out
}

fn strip_md(s: &str) -> String {
    static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!?\[([^\]]*)\]\([^)]*\)").unwrap());
    static HTML: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
    let s = LINK.replace_all(s, "$1");
    let s = HTML.replace_all(&s, "");
    s.trim_start_matches(['-', '*', '>', ' ']).replace("**", "").replace('`', "").trim().to_string()
}

// ---------- any web page ----------

fn read_web(url: &str) -> Result<Source, String> {
    let f = fetch(url, Some("text/html,application/xhtml+xml,application/pdf;q=0.9,*/*;q=0.5"))?;
    if f.content_type.contains("pdf") || f.body.starts_with(b"%PDF") {
        return Ok(Source { kind: "pdf".into(), link: f.url.clone(), title: String::new(), sections: pdf_sections(&f.body)?, ..Default::default() });
    }
    let doc = Html::parse_document(&String::from_utf8_lossy(&f.body));
    let mut src = Source { kind: "web".into(), link: url.to_string(), ..Default::default() };
    src.title = meta(&doc, "citation_title").into_iter().next().unwrap_or_else(|| meta_title(&doc));
    src.authors = meta(&doc, "citation_author");
    src.year = meta(&doc, "citation_publication_date").into_iter().chain(meta(&doc, "citation_date")).next().and_then(|d| d.get(0..4).and_then(|y| y.parse().ok()));
    if let Some(d) = meta(&doc, "citation_doi").into_iter().next() {
        src.canonical = Some(format!("doi:{}", d.trim_start_matches("https://doi.org/")));
        src.persistent = true;
        src.kind = "journal".into();
    }
    src.sections = html_sections(&doc);
    if src.words() < 800 {
        if let Some(pdf) = meta(&doc, "citation_pdf_url").into_iter().next() {
            if let Ok(p) = fetch(&pdf, Some("application/pdf")) {
                if p.content_type.contains("pdf") || p.body.starts_with(b"%PDF") {
                    if let Ok(secs) = pdf_sections(&p.body) {
                        src.sections = secs;
                        src.notes.push("The page linked a free PDF, so the full text came from that PDF.".into());
                    }
                }
            }
        }
    }
    if src.words() < 150 { return Err("That page has almost no readable text. It may need a login, or load its content with JavaScript.".into()); }
    if src.title.is_empty() { src.title = host(url); }
    Ok(src)
}

fn meta(doc: &Html, name: &str) -> Vec<String> {
    let sel = Selector::parse(&format!("meta[name=\"{}\"], meta[property=\"{}\"]", name, name)).unwrap();
    doc.select(&sel).filter_map(|m| m.value().attr("content")).map(|s| clean_space(s)).filter(|s| !s.is_empty()).collect()
}

fn meta_title(doc: &Html) -> String {
    if let Some(t) = meta(doc, "og:title").into_iter().next() { return t; }
    let sel = Selector::parse("title").unwrap();
    doc.select(&sel).next().map(|t| clean_space(&t.text().collect::<String>())).unwrap_or_default()
}

const SKIP_TAGS: &[&str] = &["nav", "header", "footer", "aside", "figure", "script", "style", "noscript", "form", "button", "table"];
const SKIP_CLASSES: &[&str] = &["ltx_bibliography", "ltx_note", "ltx_page_footer", "ltx_page_header", "ltx_authors", "sidebar", "comments", "share", "related"];

fn skipped(el: &ElementRef) -> bool {
    let mut cur = Some(*el);
    while let Some(e) = cur {
        let v = e.value();
        if SKIP_TAGS.contains(&v.name()) { return true; }
        if let Some(c) = v.attr("class") { if SKIP_CLASSES.iter().any(|s| c.split_whitespace().any(|x| x == *s)) { return true; } }
        cur = e.parent().and_then(ElementRef::wrap);
    }
    false
}

/// Text of an element with maths replaced by its plain-text form.
fn text_of(el: ElementRef) -> String {
    let mut out = String::new();
    collect(el, &mut out);
    clean_space(&out)
}

fn collect(el: ElementRef, out: &mut String) {
    for child in el.children() {
        match child.value() {
            Node::Text(t) => out.push_str(t),
            Node::Element(e) => {
                let name = e.name();
                if name == "math" {
                    if let Some(alt) = e.attr("alttext") { out.push(' '); out.push_str(alt); out.push(' '); }
                    continue;
                }
                if matches!(name, "script" | "style" | "annotation" | "button") { continue; }
                if e.attr("class").map(|c| c.split_whitespace().any(|x| x == "ltx_note" || x == "ltx_note_mark")).unwrap_or(false) { continue; }
                if let Some(c) = ElementRef::wrap(child) { collect(c, out); }
                if matches!(name, "br" | "p" | "div") { out.push(' '); }
            }
            _ => {}
        }
    }
}

pub fn html_sections(doc: &Html) -> Vec<Section> {
    let roots = ["article", "main", "[role=main]", ".ltx_page_main", "body"];
    let root = roots.iter().find_map(|r| doc.select(&Selector::parse(r).unwrap()).next());
    let Some(root) = root else { return Vec::new() };
    let sel = Selector::parse("h1, h2, h3, h4, p, li").unwrap();
    let psel = Selector::parse("p").unwrap();
    let mut out: Vec<Section> = Vec::new();
    let mut cur = Section { heading: "Abstract".into(), text: String::new() };
    let abs_sel = Selector::parse(".ltx_abstract, .abstract, #abstract, section.abstract").unwrap();
    let abstract_text: Option<String> = doc.select(&abs_sel).next().map(|a| a.select(&psel).map(text_of).collect::<Vec<_>>().join("\n\n")).filter(|t| word_count(t) > 20);
    if let Some(a) = &abstract_text { out.push(Section { heading: "Abstract".into(), text: a.clone() }); cur.heading = "Introduction".into(); }
    for el in root.select(&sel) {
        if skipped(&el) { continue; }
        let name = el.value().name();
        if let Some(c) = el.value().attr("class") { if c.contains("ltx_abstract") { continue; } }
        if abstract_text.is_some() && el.ancestors().filter_map(ElementRef::wrap).any(|a| a.value().attr("class").map(|c| c.contains("ltx_abstract")).unwrap_or(false)) { continue; }
        if name.starts_with('h') {
            let h = text_of(el);
            if h.is_empty() || h.len() > 160 { continue; }
            let hl = h.to_lowercase();
            if !cur.text.trim().is_empty() { out.push(std::mem::take(&mut cur)); }
            if hl.contains("reference") || hl.contains("bibliograph") || hl.starts_with("acknowledg") { cur.heading = "References".into(); continue; }
            cur.heading = h;
            continue;
        }
        if cur.heading == "References" { continue; }
        if name == "li" && el.select(&psel).next().is_some() { continue; }
        let t = text_of(el);
        if word_count(&t) < 4 { continue; }
        if !cur.text.is_empty() { cur.text.push_str("\n\n"); }
        cur.text.push_str(&t);
    }
    if !cur.text.trim().is_empty() && cur.heading != "References" { out.push(cur); }
    out
}

// ---------- PDF (fallback) ----------

pub fn pdf_sections(bytes: &[u8]) -> Result<Vec<Section>, String> {
    let owned = bytes.to_vec();
    let text = std::panic::catch_unwind(move || pdf_extract::extract_text_from_mem(&owned))
        .map_err(|_| "This PDF couldn't be read. It may be scanned images or use an unusual format.".to_string())?
        .map_err(|e| format!("This PDF couldn't be read: {}", e))?;
    let secs = text_sections(&text);
    if secs.iter().map(|s| word_count(&s.text)).sum::<usize>() < 150 {
        return Err("This PDF has almost no extractable text; it's probably scanned images.".into());
    }
    Ok(secs)
}

static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:(?:\d+(?:\.\d+)*\.?)|(?:[IVX]{1,5}\.))\s+([A-Z][A-Za-z][A-Za-z0-9 ,:&/\-]{1,70})$").unwrap());
const KNOWN: &[&str] = &["abstract", "introduction", "related work", "background", "method", "methods", "methodology", "approach", "proposed method", "data", "dataset", "datasets", "data preprocessing", "preprocessing", "feature engineering", "features", "model", "experiments", "experimental setup", "experimental results", "results", "evaluation", "discussion", "conclusion", "conclusions", "limitations", "references", "acknowledgments", "acknowledgements"];

pub fn text_sections(text: &str) -> Vec<Section> {
    let text = text.replace("-\n", "").replace('\r', "");
    let mut out: Vec<Section> = Vec::new();
    let mut cur = Section { heading: "Front matter".into(), text: String::new() };
    let mut refs = false;
    for line in text.lines() {
        let t = line.trim();
        let tl = t.to_lowercase();
        let tl_clean = tl.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ').to_string();
        let is_heading = t.len() < 80 && (HEADING.is_match(t) || KNOWN.contains(&tl_clean.as_str()));
        if is_heading {
            if !cur.text.trim().is_empty() && !refs { out.push(std::mem::take(&mut cur)); } else { cur.text.clear(); }
            refs = tl_clean.starts_with("reference") || tl_clean.starts_with("acknowledg") || tl_clean.starts_with("bibliograph");
            cur.heading = t.to_string();
            continue;
        }
        if refs { continue; }
        if t.is_empty() { cur.text.push_str("\n\n"); } else { cur.text.push_str(t); cur.text.push(' '); }
    }
    if !cur.text.trim().is_empty() && !refs { out.push(cur); }
    for s in &mut out { s.text = s.text.split("\n\n").map(clean_space).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n\n"); }
    out
}

/// Ask the Wayback Machine to keep a copy of a page. Returns the archive link.
pub fn archive(url: &str) -> Option<String> {
    let a = ureq::AgentBuilder::new().timeout(Duration::from_secs(90)).user_agent(UA).build();
    match a.get(&format!("https://web.archive.org/save/{}", url)).call() {
        Ok(_) => Some(format!("https://web.archive.org/web/{}", url)),
        Err(ureq::Error::Status(code, _)) if code < 500 && code != 429 => Some(format!("https://web.archive.org/web/{}", url)),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_links() {
        assert_eq!(classify("https://arxiv.org/abs/2401.01234v2"), "arxiv");
        assert_eq!(classify("2401.01234"), "arxiv");
        assert_eq!(classify("arXiv:2401.01234"), "arxiv");
        assert_eq!(classify("https://doi.org/10.1016/j.eswa.2023.1"), "doi");
        assert_eq!(classify("10.1145/3292500.3330701"), "doi");
        assert_eq!(classify("https://github.com/owner/repo"), "github");
        assert_eq!(classify("https://someone.blog/post"), "web");
        assert_eq!(classify("hello"), "unknown");
    }
    #[test]
    fn html_sections_skip_nav_and_references() {
        let html = r#"<html><body><nav><p>Home About Contact us today now</p></nav><article>
          <h1>My Post</h1><p>Intro text about fraud detection with several words here.</p>
          <h2>Data preprocessing</h2><p>We removed duplicates and standardized all numeric features before training.</p>
          <h2>References</h2><p>[1] Some paper that should not be indexed at all.</p></article></body></html>"#;
        let doc = Html::parse_document(html);
        let s = html_sections(&doc);
        assert!(s.iter().any(|x| x.heading == "Data preprocessing"), "{:?}", s);
        assert!(!s.iter().any(|x| x.text.contains("should not be indexed")));
        assert!(!s.iter().any(|x| x.text.contains("Contact")));
    }
    #[test]
    fn pdf_text_headings() {
        let t = "Title\n\n1 Introduction\nFraud is costly and growing.\n\n3.1 Data Preprocessing\nWe applied SMOTE to the training set only.\n\nReferences\n[1] X.";
        let s = text_sections(t);
        assert!(s.iter().any(|x| x.heading.contains("Data Preprocessing")));
        assert!(!s.iter().any(|x| x.text.contains("[1] X")));
    }
    #[test]
    fn markdown_readme() {
        let s = markdown_sections("# Repo\nA model.\n## Training\nWe train with early stopping.\n```\ncode\n```");
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].heading, "Training");
    }
}
