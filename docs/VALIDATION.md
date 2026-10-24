# Validation plan: be sure before building further

This draft already works end to end. These four spikes decide whether its core holds up on real students' material. Each takes a few days, and together they take about three weeks. Build the test sets first; every gate below is measured, not felt.

## Test sets to collect (week 1)

- **20 real paper links** from 5 students: a mix of arXiv, DOI and journal pages, at least 3 blogs and 2 GitHub repos.
- **10 hand-made gold cards:** for 10 of those papers, write down by hand the techniques per stage, the datasets, the key results and the settings.
- **3 real code folders** (notebooks and scripts), with the techniques in them hand-labelled and the true file histories noted (which drafts belong together).
- **30 questions** students would ask of their papers, each with the paper and section that answers it.

## The spikes

| Spike | What you run | Go if… | If it fails |
|---|---|---|---|
| **A. Readers** | Add the 20 links in the app | 18 of 20 give clean sections (check "Key passages kept") | Add a site-specific reader for the failing sources; prefer HTML over PDF |
| **B. Extraction** | Compare rules-only cards, then rules + one chat round, with the gold cards | Rules get 60% of gold fields; rules + chat get 85%, with no unchecked item shown as fact | Extend catalog phrases and fact patterns; make the chat round part of adding a paper |
| **C. Code detection** | Open the 3 folders; compare each Mine row and history with the labels | 80% of labelled techniques found, under 10% wrong; lineage right for 90% of files | Add code patterns; tune lineage thresholds (0.6) on these folders |
| **D. Users** | Show 5 students the Compare view on their own topic | 3 of 5 say they'd use it this month | Watch where they get lost; change the view before adding features |

## Measuring

- **Readers:** count a link as clean when the main sections (method, experiments) appear and the references don't.
- **Extraction:** a field is right when the same technique, dataset or number appears on the gold card. Count both misses and extras.
- **Code:** use the Compare view's evidence drawer, which shows the exact line behind every dot.
- **Questions:** right when the top 3 results contain the passage that answers it (target 80%).

Write the numbers down per spike. If all four pass, continue with the roadmap. If B or C fail, the comparison table can't be trusted yet, so fix that before anything else.
