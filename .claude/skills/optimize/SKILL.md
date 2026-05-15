---
description: Attempt ONE optimization hypothesis on the indexmap_store crate. Test-gates the change, runs 3 confirming bench rounds against a fixed pre-change baseline, keeps the change if either (a) at least one scenario reliably improves ≤ -1.5% with no scenario regressing ≥ +1.5%, OR (b) all scenarios are improvements (Δ trim_mean < 0 in all 3 runs) with at most 2 scenarios allowed to fall short of the ≤ -0.5% bar, and no scenario drifting > +0.1%. Records the attempt in OPTIMIZATIONS.md regardless of outcome and commits the log so future invocations skip dead ends.
---

# /optimize

You are running **ONE** optimization attempt on the `indexmap_store` crate, end-to-end, autonomously. Do not bundle multiple ideas; the user runs `/optimize` again for the next hypothesis.

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

## 3. Snapshot the pre-change SHA

```bash
mkdir -p /tmp/optimize
git rev-parse HEAD > /tmp/optimize/baseline-sha
```

The pre-change `bench_results.json` IS the baseline; `--verify` reads it
without overwriting, so no file backup is needed. Only the SHA is recorded
here so §8 can do a clean revert.

---

## 4. Implement the change

Minimal diff, one hypothesis, no drive-by refactors or comment cleanups. If the change requires a new dependency, add it; if it's something the bench harness exercises, you're done.

---

## 4a. Capture the diff (mandatory, before any gate runs)

Save the implemented change as a patch file under `optimization-diffs/<NNN>-<slug>.patch` at the crate root, **before** running tests or benches. The patch must survive the §8 revert so the attempt can be resurfaced later (bulks, retries, partial-stack experiments). The revert in §8 explicitly leaves this directory untouched.

**The saved patch is a REFERENCE artifact, not a replay script.** Do not `git apply` it on retry — the codebase will have drifted (line numbers, surrounding code, dependency versions, even renamed symbols) and a clean apply is neither expected nor desired. When resurfacing an attempt, read the patch to understand the *intent and shape* of the change, then re-implement it against the current code. Treat it like a design sketch, not a binary.

`<NNN>` is a zero-padded 3-digit index in the order attempts were tried. Compute it as `max(existing) + 1`:

```bash
mkdir -p optimization-diffs
# Next index = highest existing NNN + 1 (001 if empty)
NEXT=$(ls optimization-diffs/ 2>/dev/null | grep -oE '^[0-9]{3}' | sort -n | tail -1)
NEXT=$(printf '%03d' $(( ${NEXT:-0} + 1 )))
PATCH="optimization-diffs/${NEXT}-<slug>.patch"
git diff -- src/ tests/ benches/ Cargo.toml Cargo.lock > "$PATCH"
# Sanity: must be non-empty (a no-op attempt is a bug)
test -s "$PATCH" || { echo "empty diff — abort"; exit 1; }
echo "saved $PATCH"
```

Use `$PATCH` (or the literal `optimization-diffs/<NNN>-<slug>.patch` path) in §9 (markdown links) and §10 (`git add`).

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

## 6. Bench gate — 3 confirming runs (single command)

The bench harness has built-in verify mode: it runs the full pipeline N times against the current `bench_results.json` baseline and emits all per-scenario deltas in one structured file. Do **not** capture per-run snapshots to /tmp anymore.

```bash
cargo bench --bench store_bench -- --verify 3
```

This produces:

* `verify_results.json` at the crate root — the canonical source of truth for this verification. Parseable JSON with this shape:

  ```json
  {
    "schema_version": 2,
    "baseline_timestamp_unix": 1778869482,
    "scenarios": {
      "insert_10k_u64": {
        "baseline_trim_mean_ns": 206492,
        "runs_trim_mean_ns": [205144, 205988, 205861],
        "deltas_pct": [-0.65, -0.24, -0.31]
      },
      ...
    }
  }
  ```

* A `VERIFY-JSON: { ... }` single line on stdout carrying the same payload (for one-shot `grep`-and-parse).
* A human-readable table on stdout with one column per run and signed Δ%.

The baseline is `bench_results.json` (pre-change) by default; override with `BENCH_BASELINE=<path>` if you want to compare against a saved snapshot instead. Verify mode does **NOT** touch `bench_results.json`, so the baseline stays intact across the verification cycle — no /tmp backup needed.

Read `verify_results.json` and pull `deltas_pct` per scenario; those three values per scenario feed §7 directly.

---

## 7. Verdict logic

Default bench config is `BENCH_INVOKES=3`, `BENCH_ROUNDS=20`, `BENCH_PER_ROUND=10`, `BENCH_WARMUP_ROUNDS=3`. The headline anchor is `trimmed_mean_ns` (20% trim over the pooled N×R round medians) — chosen over `p50_ns`/`min_ns` because it's robust to both per-sample outliers AND to the ~5% inter-invocation regime bimodality observed on shared AMD Zen hardware. Inter-run noise floor on this anchor is ~0.2-0.5% on most scenarios; lookup-heavy scenarios can still swing ~1-2% when the per-invocation fast/slow regime split flips between runs. Thresholds unchanged:

| Verdict              | Criterion                                                                                                                                                                                                                                          |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **KEPT (deep-win)**  | ≥1 scenario shows Δ trim_mean ≤ **-1.5%** in **all 3 runs**, AND no scenario shows Δ trim_mean ≥ **+1.5%** in **any** run                                                                                                                                      |
| **KEPT (broad-win)** | **ALL** scenarios show Δ trim_mean **< 0%** in **all 3 runs** (every run is a measured improvement), AND **at most 2** scenarios fall short of the strict ≤ **-0.5%** bar in any run, AND no scenario shows Δ trim_mean > **+0.1%** in **any** run             |
| **REVERTED**         | Any scenario shows Δ trim_mean ≥ **+1.5%** in any run (and broad-win is not satisfied)                                                                                                                                                                   |
| **INCONCLUSIVE**     | Neither KEPT path satisfied and no regression past +1.5%                                                                                                                                                                                           |

Apply in this order:

1. **REVERTED**: any scenario shows Δ trim_mean ≥ +1.5% in any run → revert.
2. **KEPT (deep-win)**: ≥1 scenario shows Δ trim_mean ≤ -1.5% in all 3 runs (regression guard already cleared by step 1) → keep.
3. **KEPT (broad-win)**: ALL scenarios show Δ trim_mean < 0% in all 3 runs (no run is flat-or-positive) AND no scenario shows Δ trim_mean > +0.1% in any run AND **at most 2** scenarios have any run that falls in the weak band `(-0.5%, 0%)` (the remaining scenarios must clear ≤ -0.5% in all 3 runs) → keep. This captures "many small wins everywhere" diffs that don't move any single scenario by -1.5%; the +0.1% sub-guard prevents shipping a broad-but-jittery change, and the "≤2 weak scenarios" tolerance lets uniformly-negative drifts pass even when one or two scenarios hover just under the -0.5% bar.
4. **INCONCLUSIVE**: otherwise → revert.

---

## 8. Revert (REVERTED and INCONCLUSIVE only)

```bash
git checkout -- src/ tests/ benches/ Cargo.toml Cargo.lock
git clean -fd src/ tests/ benches/
```

`optimization-diffs/` is deliberately NOT in the revert paths — the saved `<slug>.patch` from §4a survives so the attempt can be resurfaced.

`bench_results.json` is untouched by `--verify`, so it already holds the pre-change baseline — no restore step needed.

For KEPT verdicts: do **not** revert. Refresh the baseline so subsequent /optimize invocations measure against the post-change numbers:

```bash
cargo bench --bench store_bench
```

That single (non-verify) run regenerates `bench_results.json` from the kept code.

---

## 9. Update OPTIMIZATIONS.md

Two updates:

**(a) Index table — add one row at the top of the table body** (most-recent-first). The Diff column links to the patch saved in §4a:

```
| 2026-05-14 | preallocate-replay-payload | src/lib.rs | LOW | KEPT | -4.1% on reopen_10k, others noise | [diff](optimization-diffs/026-preallocate-replay-payload.patch) |
```

**(b) Detailed entry — append below the Index table** using the template comment in OPTIMIZATIONS.md. Include:

- Hypothesis (one sentence)
- Risk tag
- Files touched
- **Diff:** `[optimization-diffs/<NNN>-<slug>.patch](optimization-diffs/<NNN>-<slug>.patch)` — reference only. Must be present even when REVERTED/INCONCLUSIVE so a future retry can read the intent and re-implement against the current codebase. Do **not** `git apply` it on retry; the surrounding code will have drifted.
- Baseline trim_mean for every scenario
- Three Δ trim_mean values per scenario, comma-separated, with the verdict reasoning visible at a glance
- Verdict + why
- Follow-ups / dead ends (anything closed off, anything worth a separate next attempt)

---

## 10. Commit

```bash
git add OPTIMIZATIONS.md bench_results.json optimization-diffs/<NNN>-<slug>.patch
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
- Never reduce `BENCH_INVOKES` below 3 or `BENCH_ROUNDS × BENCH_PER_ROUND` below 200 during the 3-run validation.
- Never overwrite `bench_results.json` mid-cycle. `--verify` reads it as the baseline and must see the pre-change values; refresh it only after a KEPT verdict via a non-verify `cargo bench` run.
- Never skip §4a (diff capture) or include `optimization-diffs/` in the §8 revert paths — the patch must survive for future resurfacing.
- If you cannot find a plausible untried hypothesis, say so explicitly and exit — do not propose something already in the Index table.
