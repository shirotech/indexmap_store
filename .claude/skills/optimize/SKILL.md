---
description: Attempt ONE optimization hypothesis on the index_map_store crate. Test-gates the change, runs 3 confirming bench rounds against a fixed pre-change baseline, keeps the change if either (a) at least one scenario reliably improves ≤ -1.5% with no scenario regressing ≥ +1.5%, OR (b) all scenarios broadly improve ≤ -0.5% with no scenario drifting > +0.1%. Records the attempt in OPTIMIZATIONS.md regardless of outcome and commits the log so future invocations skip dead ends.
---

# /optimize

You are running **ONE** optimization attempt on the `index_map_store` crate, end-to-end, autonomously. Do not bundle multiple ideas; the user runs `/optimize` again for the next hypothesis.

The whole point of this skill is **reliable iteration**: every attempt — kept or rejected — gets logged so the next invocation cannot accidentally retry it.

---

## 0. Prerequisites (abort with explanation if any fail)

Run in order; if any check fails, print why and stop.

1. `git status --porcelain` must be empty (no uncommitted changes — needed for safe revert).
2. `bench_results.json` exists at the crate root (the calibrated baseline).
3. `OPTIMIZATIONS.md` exists at the crate root.
4. `cargo --version` works.

If `OPTIMIZATIONS.md` shows the **last 3 attempts** all `REVERTED` or `INCONCLUSIVE`, **stop and ask the user** whether to continue — easy wins are likely exhausted and remaining hypotheses are risky.

---

## 1. Read the optimization log

Read `OPTIMIZATIONS.md` and extract every hypothesis slug from the Index table. **Do not propose any slug already listed**, regardless of verdict — `REVERTED` and `INCONCLUSIVE` are closed dead ends, `KEPT` is already done.

---

## 2. Pick ONE hypothesis

Pick the highest-value untried hypothesis from the list below (or invent one in the same shape). State it as a one-sentence claim with a risk level.

**LOW risk** (proceed without asking):
- pre-size IndexMap with `with_capacity` at open time using the on-disk file size as a hint
- pre-size the replay `payload` Vec on the largest record observed so far
- batch length-prefix + payload into a single `write_all` via a single buffer (avoid two BufWriter writes per record)
- inline `flush_scratch` callsite that's only used once
- replace `Vec::resize` with `Vec::clear` + `Vec::reserve` + unsafe `set_len` (audit — pushes into MEDIUM)
- use `IndexMap::with_capacity_and_hasher` with a faster hasher (e.g., `foldhash`/`ahash`) — adds a dep, MEDIUM
- skip the `path.exists()` probe and rely on `OpenOptions::read` errors

**MEDIUM risk** (proceed but flag clearly in the log):
- swap bincode for a hand-rolled fixed-prefix codec for primitives
- add a snapshot-file path (separate from WAL) for faster cold reopen
- coalesce compaction into background after release of the write lock

**HIGH risk** (STOP and ask user before implementing):
- change public API signatures or exported types
- introduce `unsafe`
- swap a dependency (bincode → rkyv, indexmap → custom)
- add mmap

---

## 3. Snapshot the pre-change state

```bash
mkdir -p /tmp/optimize
git rev-parse HEAD > /tmp/optimize/baseline-sha
cp bench_results.json /tmp/optimize/baseline.json
```

The baseline file is what every bench run compares against — do **not** let it be overwritten.

---

## 4. Implement the change

Minimal diff, one hypothesis, no drive-by refactors or comment cleanups. If the change requires a new dependency, add it; if it's something the bench harness exercises, you're done.

---

## 4a. Capture the diff (mandatory, before any gate runs)

Save the implemented change as a patch file under `optimization-diffs/<slug>.patch` at the crate root, **before** running tests or benches. The patch must survive the §8 revert so the attempt can be resurfaced later (bulks, retries, partial-stack experiments). The revert in §8 explicitly leaves this directory untouched.

```bash
mkdir -p optimization-diffs
git diff -- src/ tests/ benches/ Cargo.toml Cargo.lock > optimization-diffs/<slug>.patch
# Sanity: must be non-empty (a no-op attempt is a bug)
test -s optimization-diffs/<slug>.patch || { echo "empty diff — abort"; exit 1; }
```

If the change adds a brand-new source file, `git add -N <new-file>` first so `git diff` includes it (the `-- src/ tests/ benches/` pathspec already covers the directories).

---

## 5. Test gate (mandatory)

Run both:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

If **either** fails: revert (see §8), record verdict `REVERTED` with reason `"test gate failed: <summary>"`, commit (§9), exit.

---

## 6. Bench gate — 3 confirming runs

Use the **same** pre-change baseline for each comparison. The bench harness reads `BENCH_BASELINE` if set and uses that as the "previous" reference; it still writes the current run to `bench_results.json`.

Run three independent rounds:

```bash
BENCH_BASELINE=/tmp/optimize/baseline.json cargo bench
cp bench_results.json /tmp/optimize/run1.json

BENCH_BASELINE=/tmp/optimize/baseline.json cargo bench
cp bench_results.json /tmp/optimize/run2.json

BENCH_BASELINE=/tmp/optimize/baseline.json cargo bench
cp bench_results.json /tmp/optimize/run3.json
```

Each printed Δ p50 column compares that run to the pre-change baseline. Capture all three values per scenario.

---

## 7. Verdict logic

Default bench config is `BENCH_SAMPLES=1001`, `BENCH_WARMUP=5` — noise floor is ≈1%. The gates sit just above noise on both sides; tighter than the original ±2/-3% but still tolerant of single-run jitter on the improvement side. Thresholds:

| Verdict | Criterion |
|---|---|
| **KEPT (deep-win)** | ≥1 scenario shows Δ p50 ≤ **-1.5%** in **all 3 runs**, AND no scenario shows Δ p50 ≥ **+1.5%** in **any** run |
| **KEPT (broad-win)** | **ALL** scenarios show Δ p50 ≤ **-0.5%** in **all 3 runs**, AND no scenario shows Δ p50 > **+0.1%** in **any** run |
| **REVERTED** | Any scenario shows Δ p50 ≥ **+1.5%** in any run (and broad-win is not satisfied) |
| **INCONCLUSIVE** | Neither KEPT path satisfied and no regression past +1.5% |

Apply in this order:

1. **REVERTED**: any scenario shows Δ p50 ≥ +1.5% in any run → revert.
2. **KEPT (deep-win)**: ≥1 scenario shows Δ p50 ≤ -1.5% in all 3 runs (regression guard already cleared by step 1) → keep.
3. **KEPT (broad-win)**: ALL scenarios show Δ p50 ≤ -0.5% in all 3 runs AND no scenario shows Δ p50 > +0.1% in any run → keep. This captures "many small wins everywhere" diffs that don't move any single scenario by -1.5%; the tight +0.1% regression sub-guard prevents shipping a broad-but-jittery change.
4. **INCONCLUSIVE**: otherwise → revert.

---

## 8. Revert (REVERTED and INCONCLUSIVE only)

```bash
git checkout -- src/ tests/ benches/ Cargo.toml Cargo.lock
git clean -fd src/ tests/ benches/
```

`optimization-diffs/` is deliberately NOT in the revert paths — the saved `<slug>.patch` from §4a survives so the attempt can be resurfaced.

Restore the pre-change baseline so it stays the canonical reference:

```bash
cp /tmp/optimize/baseline.json bench_results.json
```

For KEPT verdicts: do **not** revert. The post-change `bench_results.json` from run 3 becomes the new baseline — keep it.

---

## 9. Update OPTIMIZATIONS.md

Two updates:

**(a) Index table — add one row at the top of the table body** (most-recent-first). The Diff column links to the patch saved in §4a:

```
| 2026-05-14 | preallocate-replay-payload | src/lib.rs | LOW | KEPT | -4.1% on reopen_10k, others noise | [diff](optimization-diffs/preallocate-replay-payload.patch) |
```

**(b) Detailed entry — append below the Index table** using the template comment in OPTIMIZATIONS.md. Include:

- Hypothesis (one sentence)
- Risk tag
- Files touched
- **Diff:** `[optimization-diffs/<slug>.patch](optimization-diffs/<slug>.patch)` — must be present even when REVERTED/INCONCLUSIVE so the change can be replayed in a future bulk/retry
- Baseline p50 for every scenario
- Three Δ p50 values per scenario, comma-separated, with the verdict reasoning visible at a glance
- Verdict + why
- Follow-ups / dead ends (anything closed off, anything worth a separate next attempt)

---

## 10. Commit

```bash
git add OPTIMIZATIONS.md bench_results.json optimization-diffs/<slug>.patch
# If KEPT, also stage source changes:
git add src/ tests/ benches/ Cargo.toml Cargo.lock
git commit -m "optimize: <slug> [<VERDICT>]"
```

The `<slug>.patch` is committed for every verdict, so REVERTED/INCONCLUSIVE attempts that exist only in the markdown log still have a replayable source diff in the repo.

Suggested message bodies:

- `optimize: preallocate-replay-payload [KEPT] -4.1% on reopen_10k`
- `optimize: foldhash-indexmap [REVERTED] +3.2% regression on insert_2k_strings`
- `optimize: inline-flush-scratch [INCONCLUSIVE] all scenarios within noise`

Do **not** push.

---

## Hard rules

- ONE hypothesis per invocation. No bundling.
- Never modify the public API, introduce `unsafe`, or swap a dependency without explicit user approval (HIGH risk — stop and ask).
- Never disable tests, weaken assertions, or skip the clippy gate.
- Never reduce `BENCH_SAMPLES` below 501 during the 3-run validation.
- Never overwrite `/tmp/optimize/baseline.json` mid-run.
- Never skip §4a (diff capture) or include `optimization-diffs/` in the §8 revert paths — the patch must survive for future resurfacing.
- If you cannot find a plausible untried hypothesis, say so explicitly and exit — do not propose something already in the Index table.
