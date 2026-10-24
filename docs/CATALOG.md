# Extending the catalog

The catalog is the shared vocabulary between papers and code. Better catalog, better comparisons.

## Per project, from the app

In **Compare**, unrecognised steps in your code are listed under "Steps in your code ResearchTrail doesn't know". Select **Name it**, give it the name a paper would use, and pick its stage. This writes `.trail/catalog.local.json`, and the step is recognised in your code and in papers from then on. Local entries override built-in ones with the same `id`.

## For everyone, in `catalog/techniques.json`

```json
{
  "id": "scale_quantile",
  "name": "Quantile transform",
  "stage": "preprocessing",
  "phrases": ["quantile transform", "quantile transformation", "rank-gauss"],
  "code": ["QuantileTransformer"]
}
```

- **id:** stable, lowercase, with underscores. Never rename one that's in use, because cards refer to it.
- **stage:** one of `data`, `preprocessing`, `features`, `model`, `training`, `evaluation`.
- **phrases:** the words papers use. They're matched after light stemming, so "standardized" matches "standardization". Avoid very short or common words that would match everywhere.
- **code:** names used in a single statement. A pattern like `a&b` needs both names, for example `value_counts&map` for frequency encoding. Names are call names (`StandardScaler`), attribute names (`hour` in `.dt.hour`) or keyword arguments (`class_weight`).

After editing, run `cargo test` and rebuild. The catalog is compiled into the binary, so rebuild to see changes.

## Adding a domain

To cover computer vision or NLP, add their techniques (augmentations, backbones, tokenisers, losses, metrics) with the same six stages, plus their datasets in `"datasets"`. Test on 5 real papers and 2 real notebooks from that domain before shipping, using the method in [VALIDATION.md](VALIDATION.md).
