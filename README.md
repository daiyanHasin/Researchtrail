# ResearchTrail

**Keep what you read next to what you build.**

ResearchTrail is a local research notebook for students and independent researchers. Paste a link to a paper and it becomes a structured card. Point it at your project folder and every save of your code is kept, even with file names like `final_final_v2.ipynb`. Then, stage by stage, it shows **what each paper did next to what your code does**.

It runs on your own computer, opens in your browser at `http://127.0.0.1:4400`, and keeps everything as plain files in your project folder. No account, no cloud, no paid AI.

> **Status: working draft (v0.1).** The core loop works end to end and has tests: add papers, ask questions with citations, automatic code history, per-stage comparison, prompt packs. Read [Known limits](#known-limits) before relying on it for a thesis.

---

## Why it exists

A BSc or MSc researcher typically reads 10–20 papers before and while writing code. Three months later the questions are always the same:

- Which paper did this preprocessing step come from?
- What did the other papers do for feature engineering that I didn't?
- What changed between `model_v2.py` and `model_final_final.py`, and why did accuracy drop?

Free AI chats can't answer these, because your papers and your code don't fit in their limits and they forget between sessions. Reference managers know your papers but not your code; version control knows your code but not your papers, and most researchers outside CS don't use git anyway. ResearchTrail connects the two.

## What it does

| | |
|---|---|
| **Papers by link** | Paste an arXiv link or ID, a DOI, a journal page, a blog post or a GitHub repo. The paper is read once, its important parts are kept as a card, and the source is discarded. A PDF upload is there as a fallback and is deleted after reading. |
| **Paper cards** | Each card records what the paper did per stage (data, preprocessing, feature engineering, model, training, evaluation), plus datasets, reported results and settings. Every item points to the sentence it came from. |
| **Ask your papers** | Search across every kept passage. Results always show the paper and section, and nothing is filled in by guessing. |
| **Code history without git** | Every save of a `.py` file or notebook is kept. Drafts with messy names are grouped into one history. You get plain-language summaries ("learning_rate 0.1 → 0.05; added SMOTE oversampling"), diffs and one-click restore. Your files are never moved, renamed or deleted. |
| **Compare per stage** | A matrix with one row per paper plus **Mine**, built from your current code. Select any dot to see the sentence or the line of code behind it. It also points out what only you do, and what most papers do that you don't. |
| **Prompt packs** | One block of text with the comparison table, the relevant paper passages and your code, cut to a token budget, ready to paste into any free AI chat. |
| **Improve cards with any chat** | Copy a prompt, paste the chat's JSON answer back, and every claim is checked against the passage it cites before it reaches the card. An optional local model (Ollama) can do the same on your computer. |

## Quick start

You need **Rust 1.80 or newer**. If you don't have it, install it from [rustup.rs](https://rustup.rs) (one installer, default options are fine). The first build downloads libraries and takes a few minutes; later starts are instant.

**Windows:** double-click `start.bat`, or run it with a folder:

```bat
start.bat "C:\Users\you\thesis"
```

**macOS / Linux:**

```bash
./start.sh ~/thesis
```

Your browser opens at `http://127.0.0.1:4400`. Without a folder, you can choose one in the browser, or select **Open the demo project** to explore three invented example papers and some messy drafts.

You can also run it directly:

```bash
cargo run --release -- ~/thesis            # open a project
cargo run --release -- --port 4500         # another port
cargo run --release -- ~/thesis --no-open  # don't open a browser
```

Keep the terminal window open while you work; closing it stops ResearchTrail.

## Your first ten minutes

1. **Open your project folder:** the one with your notebooks and scripts. ResearchTrail creates a hidden `.trail` folder inside it and snapshots your code straight away.
2. **Add three papers** by pasting their links into the box at the top. Watch the bar under the header while each one is read.
3. **Open Compare → Feature engineering** (or whichever stage you're working on). Each dot is evidence: select it.
4. **Fix what's wrong.** Remove a wrong item from a card or from your row, add something you did by hand, or name a step in your code that ResearchTrail didn't recognise. It remembers.
5. **Build a prompt pack** for your next question and paste it into your favourite chat.

## How it works

```
 paper link ──► reader ──► sections ──► rules find techniques, datasets, results ──► paper card ┐
                (arXiv HTML, DOI via Unpaywall, journal tags,      source discarded            │
                 GitHub README, any page, PDF fallback)                                        ├─► Compare, Ask, Prompt packs
 your folder ──► watcher ──► snapshot ──► lineage ──► tree-sitter finds techniques in code ───┘
                 (every 2 s)  (.trail/objects)  (draft1 → final)  (only cells that ran)
```

The key idea is a **shared technique vocabulary**: a catalog of about 90 named techniques for machine learning on tabular data. Each has the phrases papers use ("frequency encoding", "count encoding") and the code patterns that implement it (`x.map(x.value_counts())`, `CountEncoder`). Papers and code are both translated into these IDs, so comparing them means comparing sets of IDs. That makes the comparison deterministic and explainable, and every match comes with its evidence. Steps the catalog doesn't know are listed so you can name them once. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## What is stored, and where

Everything lives in `your-project/.trail/`:

| Path | What |
|---|---|
| `cards/<id>.json` | One paper card per paper: metadata, key passages (at most ~2,000 words), techniques, facts, link, archive link, fingerprint |
| `notes/<id>.md` | Your notes on each paper, as Markdown |
| `objects/` | Snapshots of your code, content-addressed (identical saves cost nothing) |
| `changes.jsonl` | One line per snapshot, with its plain-language summary |
| `lineage.json` | Which file names belong to one history |
| `stages.json` | Your corrections to the Mine rows |
| `catalog.local.json` | Techniques you named yourself |
| `config.json` | Settings |

Full paper text and PDFs are **never** written to disk. Copy the project folder to another computer and everything comes with it. Add `.trail/objects/` to your `.gitignore` if you also use git (a `.gitignore` inside `.trail` already does this).

## Privacy

- The server listens on `127.0.0.1` only, so other devices on your network can't reach it. Writes coming from other websites are refused.
- The network is used for two things only: reading the links you paste and looking up their metadata (arXiv, Crossref, Unpaywall, GitHub). Optionally, it also asks the Wayback Machine to archive blog and web links, which you can switch off in Settings.
- Nothing about your code or notes is sent anywhere. Prompt packs are copied to your clipboard; you decide where to paste them.

## Known limits

This is an honest list for a draft.

- **Domain:** the built-in catalog covers classic machine learning on tabular data. Computer vision and NLP research will see fewer matches until those catalogs exist, so name unrecognised steps as you go.
- **Code:** Python files and Jupyter notebooks only.
- **Rules-only cards are partial.** Expect roughly the obvious techniques, datasets, metrics and hyperparameters. Use "Improve this card" with your chat or a local model for more; checked items are marked.
- **Paywalled papers** can't be read from a link. Upload the PDF if you have legal access.
- **PDF parsing** is a fallback. Two-column layouts and maths-heavy pages extract less cleanly than arXiv's HTML.
- **Search** is keyword-based (BM25 plus technique matching). Local embeddings are planned.
- **Link readers** are covered by offline tests on sample pages. Run them on 20 of your own links (spike A in [docs/VALIDATION.md](docs/VALIDATION.md)) before trusting them; sites change.
- **Single user.** No sharing or sync yet.

## Project layout

```
src/
  main.rs       start-up, server threads, background watcher
  server.rs     local HTTP server and JSON API
  readers.rs    link readers: arXiv, DOI, journal, GitHub, web, PDF
  extract.rs    paper cards, the chat/model prompt, claim verification
  catalog.rs    the technique catalog and its matchers
  code.rs       Python and notebook analysis with tree-sitter
  history.rs    snapshots, lineage, change summaries, restore
  compare.rs    per-stage comparison and coverage
  search.rs     BM25 search over kept passages
  pack.rs       prompt packs
  store.rs      project state and the .trail folder
  demo.rs       the demo project
catalog/techniques.json   the built-in catalog (edit to extend)
web/                      the browser app (plain JS + compiled Tailwind CSS)
docs/                     product and technical documents
```

## Development

```bash
cargo test                 # 17 tests: stemming, catalog, readers, extraction, verification, code analysis, lineage
RT_WEB_DIR=web cargo run   # serve web/ from disk, so UI edits show on reload
npm install && npm run css # rebuild web/app.css after changing Tailwind classes
```

The compiled `web/app.css` is committed, so you only need Node to change styles. The UI is plain JavaScript with no build step.

## Documents

- [Product requirements](docs/PRD.md): goals, non-goals, constraints, success metrics
- [Features](docs/FEATURES.md): what's in this draft and what comes later
- [User stories](docs/USER_STORIES.md): with acceptance tests and current status
- [Architecture](docs/ARCHITECTURE.md): how each hard problem is solved
- [Tech stack](docs/TECH_STACK.md): choices, reasons, and where the draft differs from the plan
- [Validation plan](docs/VALIDATION.md): the four spikes that decide whether to keep building
- [Roadmap](docs/ROADMAP.md): milestones with gates
- [Extending the catalog](docs/CATALOG.md): adding techniques and domains

## License

MIT. See [LICENSE](LICENSE).
