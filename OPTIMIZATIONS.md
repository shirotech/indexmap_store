# Optimization Log

This file records every optimization attempt on `index_map_store`, kept or not.
Future `/optimize` runs read the **Index** table first and skip any hypothesis
already attempted (regardless of verdict).

**Verdict legend**

- `KEPT` — change is in the codebase; bench gate passed.
- `REVERTED` — change broke tests/clippy, or bench gate failed; code restored.
- `INCONCLUSIVE` — change neither improved nor regressed beyond noise; reverted, treat as a closed dead-end so we don't retry it.

## Index

| Date (UTC) | Hypothesis | Files touched | Risk | Verdict | Notes |
|---|---|---|---|---|---|

## Detailed entries

<!--
Append entries below in reverse-chronological order. Template:

### YYYY-MM-DD — hypothesis-slug

- **Hypothesis:** one-sentence claim.
- **Risk:** LOW / MEDIUM / HIGH.
- **Files touched:** `path/a.rs`, `path/b.rs`.
- **Baseline (pre-change) p50:**
  - scenario_a: 1.23 ms
  - scenario_b: 4.56 us
- **Δ p50 across 3 confirming runs:**
  - scenario_a: -5.1% / -4.8% / -5.4%   (KEPT — all three < -3%)
  - scenario_b: +0.2% / -0.4% / +0.1%   (within noise; not a regression)
- **Verdict:** KEPT / REVERTED / INCONCLUSIVE
- **Why:** explanation.
- **Follow-ups / dead ends:** anything a future attempt should NOT retry, or a related idea worth a separate hypothesis.
-->
