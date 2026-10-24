# Features

Status: **Draft** = in this repository and tested; **Next** = planned for the stated release.

| Area | Feature | What it does | Status |
|---|---|---|---|
| Papers | Add by link | arXiv link or ID, DOI, journal page, GitHub repo, blog or any page | Draft |
| Papers | Read once, then discard | Source is processed in memory; only the card is written | Draft |
| Papers | Paper card | Techniques per stage, datasets, results, settings, each tied to its sentence | Draft |
| Papers | PDF fallback | Upload a PDF; it's read and not kept | Draft |
| Papers | Re-fetch | Reprocess from the link when the extractor improves; skips if nothing changed | Draft |
| Papers | Archive links | Wayback Machine snapshot for non-permanent links (toggle) | Draft |
| Papers | README links | GitHub READMEs that cite papers offer to add them | Draft |
| Papers | Notes | Per-paper Markdown notes | Draft |
| Papers | Correct a card | Remove a wrong technique from a card | Draft |
| Cards | Improve with your chat | Copy prompt, paste JSON back; each claim checked against its passage | Draft |
| Cards | Improve with a local model | Same through Ollama on your computer | Draft |
| Ask | Search with citations | BM25 over kept passages + technique boost + stage filter | Draft |
| Ask | Local embeddings | Hybrid keyword + vector search | Next (v1.1) |
| Code | Automatic snapshots | Every save of `.py` / `.ipynb`, content-addressed, no git needed | Draft |
| Code | File lineage | `draft1` → `final_final_v2` grouped by name, content and time; split and join by hand | Draft |
| Code | Notebook aware | Outputs ignored in diffs; never-run cells not counted as yours | Draft |
| Code | Plain-language changes | Changed numbers, added/removed techniques and functions | Draft |
| Code | Diff and restore | See changes; restore any version (current one is saved first) | Draft |
| Compare | Matrix per stage | Papers + Mine, matched through catalog IDs, evidence on every dot | Draft |
| Compare | Mine from code | Techniques found in the newest file of each history | Draft |
| Compare | Differences | "Only in your code" and "most papers do this; you don't" | Draft |
| Compare | Corrections | Remove from your row, put back, add by hand | Draft |
| Compare | Name unknown steps | Unrecognised classes in your code become catalog entries | Draft |
| Overview | Research trail | Six stages with paper and code coverage, live updates | Draft |
| Overview | Growth and activity | Papers and versions over time; recent activity | Draft |
| Prompts | Prompt packs | Task + comparison + passages + your code, cut to 4k–32k tokens | Draft |
| Results | Run log, reported vs mine | Metrics per run next to paper-reported numbers | Next (v1.1) |
| Papers | Zotero import | Links and notes from a Zotero collection | Next (v1.2) |
| AI | Own API key | Optional OpenAI / Anthropic / Gemini for card improvement | Next (v1.2) |
| Catalogs | Computer vision, NLP | Domain catalogs beyond tabular ML | Next (v1.2) |
| App | Desktop shell | Tauri wrapper with installers | Next (v1.0 release) |
| Papers | Figures and tables | Extraction and caption search | Later (v2) |
