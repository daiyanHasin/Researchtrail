# Tech stack

| Layer | Choice in this draft | Why |
|---|---|---|
| Core | Rust (1.80+) | One fast binary; reuses the CodeBase design; low memory |
| HTTP | `tiny_http` | Small, synchronous, enough for one local user |
| UI | Plain JavaScript + Tailwind CSS (compiled, committed) | No build step to run; offline; fast |
| Fetching | `ureq` (rustls) | Blocking HTTP client with TLS, no OpenSSL to install |
| HTML | `scraper` (html5ever) | Robust parsing of arXiv HTML, journals and blogs |
| PDF | `pdf-extract` | Pure Rust; fallback only |
| Code parsing | `tree-sitter` + `tree-sitter-python` | Real syntax tree: comments and strings never count |
| Diffs | `similar` | Line diffs and grouped hunks |
| Hashing | `sha2` | Content-addressed snapshots, paper fingerprints |
| Storage | JSON and JSONL files in `.trail/` | Readable, editable, easy to copy; fine at 50 papers |
| Search | BM25 + catalog boost | Strong on technical terms; no model download |
| Optional AI | Ollama on localhost | Free, private card improvement |
| Metadata | arXiv API, Crossref, Unpaywall, GitHub API, Wayback Machine | Free and legal; no scraping |

## Where the draft differs from the plan, and why

- **Snapshots use a built-in content-addressed store instead of git.** The target users mostly don't have git installed, and bundling `gix` added weight without benefit for single-file history. The store is already git-like (content-addressed, append-only log), so a "push this history to GitHub" export can come later.
- **JSON files instead of SQLite.** With 10–50 papers, everything fits in memory. Plain files are easier to inspect and back up. Move to SQLite with `sqlite-vec` when embeddings arrive.
- **No embeddings yet.** BM25 plus catalog matching answers the common questions. Add `fastembed-rs` (bge-small) as the next search improvement and measure it against the 30-question test set.
- **Local web app instead of a Tauri desktop shell.** The browser UI works today on every OS. Tauri can wrap the same server and UI for installers in the v1.0 release.

## Dependency pinning

`Cargo.lock` pins versions that build on Rust 1.80, the version in Ubuntu 24.04's packages, and they also build on newer Rust. If `cargo update` pulls a crate that needs a newer compiler, either update Rust (`rustup update`) or keep the lockfile as shipped.
