# Product requirements

## One line

A local research notebook that connects what you read to what you code, and remembers every version of both.

## Problem

An independent BSc or MSc researcher reads 10–20 papers, then writes code in scattered files and notebooks. Months later they can't say which paper a step came from, what other papers did differently, or what changed between two drafts. Free AI chats can't help: the papers and code don't fit their limits, and they forget between sessions. Reference managers don't know about code. Version control doesn't know about papers, and most researchers outside CS and EEE don't use git.

## Users

- **Primary:** final-year BSc and MSc students doing a thesis or first paper, alone or with a busy supervisor, on a personal laptop (8 GB RAM, no GPU).
- **Secondary:** early PhD students and independent researchers reproducing a paper.

## Goals for v1

1. Set up a project with papers and a code folder in under 10 minutes.
2. Ask questions across the papers and get answers with citations, offline.
3. Keep every code change; group messy file names into one history automatically.
4. Compare, stage by stage, what each paper did against your own code.
5. Copy a small, cited prompt for any AI chat in one click.

## Non-goals for v1

- Searching or recommending papers beyond the ones the user adds
- Built-in AI answers (generation goes through the user's chat or an optional local model)
- Figures, tables and images from papers
- Collaboration, sharing, cloud sync
- Writing support (LaTeX, related-work drafting)
- Code languages other than Python and notebooks
- Domains beyond classic ML on tabular data in the built-in catalog

## Constraints

- Windows, macOS, Linux; 8 GB RAM; no GPU.
- Everything in plain files inside the project folder; deleting the app loses nothing.
- **Full paper text and PDFs are never stored:** only cards, key passages (≤ ~2,000 words), the link, an archive link and a fingerprint.
- The user's files are never moved, renamed or deleted.
- No account, no telemetry. The network is used only to read pasted links, look up metadata, and (optionally) request an archive copy.
- Comfortable at 50 papers and 500 code files per project.

## Success metrics

| Metric | v1 target |
|---|---|
| Setup time for a new project | under 10 min |
| Read and card 20 papers on an 8 GB laptop | under 5 min (rules tier) |
| Questions answered with a correct citation | 80% of a 30-question test set |
| File lineage grouped correctly | 90% on real student folders |
| "Mine" row correct | 7 in 10 entries |
| Weekly use after 4 weeks of beta | 5 of 10 students |

## Why not just upload papers to a chat?

A chat forgets, caps uploads, and can't see your code or its history. ResearchTrail keeps a lasting, structured record of your papers next to your code, so every comparison and prompt starts from what you already know, and every claim points to its source.
