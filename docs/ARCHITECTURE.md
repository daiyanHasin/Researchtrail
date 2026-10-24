# Architecture

Two pipelines feed one local store; the technique catalog connects papers and code; every view reads from that store.

```
 paper link ─► readers.rs ─► Source (sections, in memory only)
                               │
                               ▼
                         extract.rs ─► Card ──────────────┐   .trail/cards/*.json
                         (rules tier; chat / model tiers   │
                          verified against passages)       │
                                                           ▼
                                                  catalog.rs (technique IDs)
                                                           ▲
 project folder ─► history.rs ─► snapshots, lineage        │   .trail/objects, changes.jsonl, lineage.json
   (watcher, 2 s)        │                                 │
                         └─► code.rs (tree-sitter) ─► hits ┘
                                                           │
                    compare.rs · search.rs · pack.rs ◄─────┘ ─► server.rs ─► web/app.js
```

## 1. The bridge: one technique vocabulary

Paper text and code use different words ("removed incomplete records" vs `df.dropna()`), so matching text to code directly is unreliable. Both are translated into the IDs of one catalog (`catalog/techniques.json`, about 90 entries for tabular ML):

```json
{ "id": "encode_frequency", "name": "Frequency / count encoding", "stage": "features",
  "phrases": ["frequency encoding", "count encoding", "frequency encoded"],
  "code": ["value_counts&map", "CountEncoder"] }
```

- **Papers → IDs** (`Catalog::match_text`): phrases are stemmed with a small stemmer, so "standardised", "standardization" and "standardized" meet. They're matched as whole-word sequences, longest phrase first.
- **Code → IDs** (`Catalog::match_code`): tree-sitter collects, per statement, the names used in calls, attributes and keyword arguments. A pattern like `value_counts&map` needs all its parts in the same statement, including statements spread across lines. Comments and strings never count.
- **Comparison** is set comparison over IDs: deterministic, testable, and every cell carries its evidence.
- **Unknown code**: class-like calls the catalog doesn't know (for example `QuantileTransformer`) are listed. Naming one writes it to `.trail/catalog.local.json`, and it's recognised from then on in code and in papers.

## 2. Extracting "the important things" without a paid model

`extract.rs` has three tiers.

1. **Rules (always):** sections are labelled by heading. Sentences in introduction, related-work and conclusion sections are kept only as context, because techniques mentioned there are usually other people's. Sentences in the other sections are kept when they contain a catalog technique, a dataset (known list plus "the X dataset" patterns), a metric with a value, or a hyperparameter with a value. The highest-scoring sentences are kept up to about 2,000 words, in reading order. Nothing is generated.
2. **Your chat:** `enrich_prompt` builds a prompt with the kept passages, the stage IDs, the technique IDs and a fixed JSON shape. You paste it into any chat and paste the answer back.
3. **Local model:** the same prompt goes to Ollama at `127.0.0.1:11434` with JSON output and temperature 0.

**Verification** (`verify_and_merge`): every item must cite a passage ID that exists. A catalog technique must be supported by its passage (one of its phrases, or most words of its name). A new technique name must have at least 60% of its words in the passage, and a number must appear verbatim. Anything else is set aside as "unchecked" with a reason, never shown as fact.

## 3. Reading links (`readers.rs`)

Readers are tried in a fixed order:

1. **arXiv** (link, `arXiv:` ID, bare ID or arXiv DOI): metadata comes from the arXiv API, and the latest version is pinned (`2401.01234v2`). The text comes from `arxiv.org/html/<id>`, with the PDF if there's no HTML.
2. **DOI**: metadata comes from Crossref. Unpaywall finds the legal open copy if you set an email. Otherwise the `doi.org` landing page is read.
3. **GitHub**: the README comes through the API and the commit hash is stored. arXiv IDs and DOIs in the README are offered as papers to add.
4. **Any page**: citation meta tags (`citation_title`, `citation_author`, `citation_doi`, `citation_pdf_url`) are read. The main text comes from `article` or `main`, skipping navigation, footers, figures, tables and references, with maths replaced by its alt text. A free PDF the page links is followed if the page itself is short.
5. **PDF**: text comes from `pdf-extract`, with headings detected by numbering and known section names. References are dropped.

The source lives only inside the ingest function and is never written. Each card stores the link, a persistent ID where one exists, a Wayback Machine link for non-permanent links, and a SHA-256 fingerprint of the text. Re-fetching compares the fingerprint and the extractor version and skips if neither changed.

## 4. Code history without git (`history.rs`)

- **Snapshots:** the watcher scans every 2 seconds and skips files saved in the last 2 seconds, so a burst of saves becomes one version. Content goes to `.trail/objects/<sha[0..2]>/<sha>`, and identical content is stored once. The snapshot time is the file's save time.
- **Lineage:** when a new file appears, it is compared with existing histories of the same type. A history counts as the same file when at least two of three signals agree and one of them is name or content:
  - its name, once version words and numbers are stripped (`v2`, `final`, `draft`, `copy`, dates);
  - its content, by Jaccard similarity over 3-line shingles of at least 0.6;
  - its time, by being newer.

  The newest existing file in a history is *current*. The others are *older drafts*, kept and never touched. You can split and join histories by hand.
- **Notebooks:** diffs use a stripped view (cell sources only). Technique changes and the Mine row use only cells that ran, according to their execution counts. A notebook with no execution counts at all is treated as never run, and all its cells count.
- **Summaries:** these list changed numeric assignments and keyword arguments, added or removed catalog techniques, and added or removed functions.
- **Restore:** the current file is snapshotted first, then the old content is written back.

## 5. Comparison (`compare.rs`)

For a stage:

- **Paper rows** come from each card's entries for that stage, each with its passages.
- **The Mine row** comes from the techniques found in the current file of each history, minus the ones you removed, plus the ones you added by hand.
- **Columns** are ordered by how many papers use them.
- **Insights:** "Only in your code" lists columns with no paper. "Most papers do this; your code doesn't" lists columns used by at least two papers and at least half of them, but absent from your code.

## 6. Search and prompt packs

- **Search** (`search.rs`): BM25 (k1 1.2, b 0.75) runs over the stemmed kept passages. Passages that share catalog techniques with the question get +2.5 each, and an optional stage filter is applied.
- **Packs** (`pack.rs`): a pack contains the instructions, the task, the paper list, the comparison table or tables, and the top passages (up to about 60% of the budget). After that come code excerpts for your techniques: the whole function or cell where it's small, otherwise ±4 lines. Whatever doesn't fit is listed as left out. Tokens are estimated at 4 bytes each.

## 7. Server and safety (`server.rs`)

- `tiny_http` serves on `127.0.0.1` only, with 6 worker threads and one shared, mutex-guarded project state.
- Non-GET requests with a foreign `Origin` are refused, so websites can't drive the local API.
- Long jobs (reading a link, asking a local model) run on their own threads. The UI polls `/api/pulse` every 1.5 seconds and redraws when the state version changes.

## Data formats

See the README table for the files in `.trail/`. A card looks like this (abridged):

```json
{ "id": "smith-2024-fraud", "title": "…", "authors": ["…"], "year": 2024,
  "link": "https://arxiv.org/abs/2401.01234v2", "canonical": "arXiv:2401.01234v2",
  "kind": "arxiv", "archive": null, "fingerprint": "sha256:…", "extractor": 1,
  "passages": [{ "id": "k3", "section": "3.1 Data Preprocessing", "stage": "preprocessing",
                 "text": "We applied SMOTE to the training set only.", "techniques": ["oversample_smote"] }],
  "stages": { "preprocessing": [{ "technique": "oversample_smote", "name": "SMOTE oversampling",
              "how": "…", "passages": ["k3"], "source": "rules" }] },
  "datasets": [], "metrics": [{ "label": "AUC", "value": "0.94", "passage": "k1" }], "hyperparams": [] }
```
