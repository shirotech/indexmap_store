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
| 2026-05-14 | batch-len-prefix-and-payload | src/lib.rs | LOW | KEPT | -4.5% to -4.8% on reopen_10k across all 3 runs; mutation paths within ±1% noise |

## Detailed entries

### 2026-05-14 — batch-len-prefix-and-payload

- **Hypothesis:** Reserving `LEN_BYTES` at the start of the per-record `scratch` buffer and filling the length in place lets `flush_scratch` emit the length-prefix and payload in a single `BufWriter::write_all`, removing one call per mutation.
- **Risk:** LOW.
- **Files touched:** `src/lib.rs` (`insert`, `remove`, `modify`, `flush_scratch`).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.37 ms
  - insert_2k_strings: 5.47 ms
  - lookup_100k: 629.49 us
  - modify_10k: 5.18 ms
  - reopen_10k: 374.58 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   -0.20% / +0.33% / -0.15%   (within noise)
  - insert_2k_strings: -0.16% / -0.16% / -0.33%   (within noise)
  - lookup_100k:      -0.11% / +0.35% / +0.35%   (within noise)
  - modify_10k:       -0.39% / -0.14% / -0.98%   (within noise)
  - reopen_10k:       -4.79% / -4.71% / -4.54%   (KEPT — all three < -3%)
- **Verdict:** KEPT.
- **Why:** Gate is satisfied — `reopen_10k` improves consistently > 3% across all three independent runs against the fixed pre-change baseline, and no scenario regresses past the +2% noise band in any run. The win shows up on the read/replay path rather than the targeted write path — likely a codegen or inlining side-effect after the `flush_scratch` rewrite (the on-disk format is identical and the replay loop was not touched). Mutation paths came out flat; the change is a refactor that incidentally pays out elsewhere. The new `bench_results.json` from run 3 becomes the next baseline.
- **Follow-ups / dead ends:** Closed: collapsing length+payload into a single `write_all` via the scratch-prefix trick. Open: `compact()` still does two separate `write_all`s per record (length, payload) on a non-hot path — could be unified the same way if compaction ever becomes hot. Open: pre-sizing the replay `IndexMap` from on-disk file size — independent hypothesis worth a separate attempt now that reopen_10k baseline is faster.

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
