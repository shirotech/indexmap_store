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
| 2026-05-14 | inline-always-flush-scratch | src/lib.rs | LOW | INCONCLUSIVE | Promoted `#[inline]` → `#[inline(always)]` on `flush_scratch` (targeted follow-up from inline-hot-path-functions) — all scenarios within ±1% noise; reopen_10k drifts -0.21% / -0.65% / -0.70% (directional but under -1.5% gate); ThinLTO already inlined flush_scratch as expected, the `always` only forces what was already happening |
| 2026-05-14 | lazy-bufwriter-allocation | src/lib.rs | LOW | REVERTED | Deferred the 1MB BufWriter mmap until first mutation via `log: Option<BufWriter>` + `file: Option<File>` — reopen_10k regressed +0.95% / +1.56% / +2.89% (run 1 over +1.5% guard); the extra struct field + per-write `is_none()` branch + Drop-time discrimination outweighed the saved mmap, and the struct grew enough that codegen layout shifted unfavorably on the read path |
| 2026-05-14 | bufwriter-capacity-2mb | src/lib.rs | LOW | INCONCLUSIVE | Bumped default `buf_capacity` from 1MB to 2MB — all scenarios within ±1% noise (reopen_10k -0.68% / +0.61% / -0.61%); 1MB already past the mmap-threshold ceiling, diminishing returns confirmed; reverted |
| 2026-05-14 | mmap-slurp-buffer-min-1mb | src/lib.rs | LOW | REVERTED | `Vec::with_capacity(total_on_disk).max(1MB)` regressed reopen_10k +4.86% to +5.04% across all 3 runs — the larger mmap region the kernel has to track (1MB) costs more on every reopen than the 240KB slurp it replaced, while only the first 240KB ever gets touched |
| 2026-05-14 | bundle-inconclusive-attempts | src/lib.rs | MEDIUM | KEPT | Stacked the 4 still-applicable INCONCLUSIVE attempts (inline-enum-tag-u8 + inline-hot-path-functions + single-open-for-replay-and-append + skip-path-exists-probe (subsumed)) — reopen_10k -1.56% to -2.02% across all 3 runs (consistently above -1.5% gate); modify_10k drifts -1.24% to -1.69% (directional but not gate-clearing); presize-replay-payload-vec excluded as obsolete after slurp-log-into-vec landed |
| 2026-05-14 | mark-compact-as-cold | src/lib.rs | LOW | REVERTED | `#[cold]` on `compact()` regressed reopen_10k +3.6% to +4.0% across all 3 runs — binary-layout side effect hurt icache locality on the read path |
| 2026-05-14 | inline-enum-tag-u8 | src/lib.rs | MEDIUM | INCONCLUSIVE | Replaced bincode 4-byte enum tag with manual 1-byte tag + bincode tuple; saved 3 bytes/record but all scenarios within ±2% noise; modify_10k -1.1% to -1.4% under -3% gate; reverted |
| 2026-05-14 | bufwriter-capacity-1mb | src/lib.rs | LOW | KEPT | -29.4% to -30.0% on reopen_10k across all 3 runs; 256K→1MB stays consistently above glibc dynamic mmap_threshold |
| 2026-05-14 | larger-default-bufwriter-capacity | src/lib.rs | LOW | KEPT | -8.4% to -9.0% on reopen_10k across all 3 runs; 64K→256K bumps allocation above glibc mmap_threshold |
| 2026-05-14 | inline-hot-path-functions | src/lib.rs | LOW | INCONCLUSIVE | Added #[inline] to thin accessors, mutation entry points, flush_scratch, maybe_compact, serialize_err; all scenarios within ±1% noise across 3 runs; reverted |
| 2026-05-14 | single-open-for-replay-and-append | src/lib.rs | LOW | INCONCLUSIVE | Saved 1–2 open syscalls per open_with; reopen_10k drift -0.7% to +0.5%, all within noise; reverted |
| 2026-05-14 | bincode-varint-encoding | src/lib.rs | MEDIUM | REVERTED | +65% regression on reopen_10k — varint decode overhead dwarfs disk-size savings (data is page-cached) |
| 2026-05-14 | slurp-log-into-vec | src/lib.rs | LOW | KEPT | -10.4% to -11.0% on reopen_10k across all 3 runs; mutation paths within ±1% noise |
| 2026-05-14 | skip-path-exists-probe | src/lib.rs | LOW | INCONCLUSIVE | Saved one stat syscall per open; reopen_10k drift -0.2% to -0.8%, all within noise; reverted |
| 2026-05-14 | presize-replay-payload-vec | src/lib.rs | LOW | INCONCLUSIVE | All scenarios within ±1.5% noise; reverted |
| 2026-05-14 | presize-indexmap-from-file-size | src/lib.rs | LOW | KEPT | -41.2% on reopen_10k across all 3 runs; mutation paths within ±1% noise |
| 2026-05-14 | batch-len-prefix-and-payload | src/lib.rs | LOW | KEPT | -4.5% to -4.8% on reopen_10k across all 3 runs; mutation paths within ±1% noise |

## Detailed entries

### 2026-05-14 — inline-always-flush-scratch

- **Hypothesis:** Targeted follow-up from `inline-hot-path-functions` ("targeted `#[inline(always)]` on a single specific callee"). Promoting `flush_scratch` from `#[inline]` to `#[inline(always)]` would force inlining at every callsite (`insert`/`remove`/`modify`) even where ThinLTO's size heuristic might pass on it — possibly merging the per-call setup into the bench loop body and exposing loop-invariant code-motion opportunities for LLVM.
- **Risk:** LOW (one-line annotation change; no behavior, API, format, or dep impact).
- **Files touched:** `src/lib.rs` (`flush_scratch`).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   -0.06% / -0.22% / +0.22%   (within noise)
  - insert_2k_strings: +0.31% / -0.14% / +0.25%   (within noise)
  - lookup_100k:      -0.16% / +0.18% / -0.01%   (within noise)
  - modify_10k:       -0.13% / +0.28% / -0.17%   (within noise)
  - reopen_10k:       -0.21% / -0.65% / -0.70%   (within noise — directionally positive but well under -1.5% gate)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** `flush_scratch` is small (~20 lines, mostly straight-line stores) and was already `#[inline]`. With `lto = "thin"` + `codegen-units = 1` in the bench profile, ThinLTO's inliner uses MIR cost to decide cross-crate inlines, and a function this small clears any heuristic threshold — promoting to `#[inline(always)]` is essentially redundant in this build profile. The slight directional improvement on `reopen_10k` (-0.21% to -0.70%) is interesting but reopen doesn't call `flush_scratch` (no mutations on the read path), so this must be a codegen-layout side effect (different symbol ordering after the annotation change shifted something on the read path closer in icache). Under the -1.5% gate either way.
- **Follow-ups / dead ends:** Closed: `#[inline(always)]` on `flush_scratch`. Closed (by extension): `#[inline(always)]` on the other already-`#[inline]`d helpers (`maybe_compact`, the accessors) — same logic, the inliner already inlines them. Open: `#[inline(always)]` on `insert`/`remove`/`modify` (the public mutation entry points) — might let LLVM see the whole 10k-iteration bench loop as a single function and apply tighter optimization. Open: `#[cold]` placed on the slow-path branch *inside* `maybe_compact` (the `compact()` invocation) via an outlined helper — different shape from the failed `mark-compact-as-cold`, and the bench doesn't exercise it so this is purely a codegen-layout knob with low expected payoff.

### 2026-05-14 — lazy-bufwriter-allocation

- **Hypothesis:** Deferring the 1MB `BufWriter::with_capacity(...)` mmap until the first mutation (by replacing `log: BufWriter<File>` with the pair `log: Option<BufWriter<File>>` + `file: Option<File>`, maintaining the invariant that exactly one is `Some`) would skip the allocation entirely on read-only opens. On the `reopen_10k` hot path the bench opens-then-drops without mutating, so the buffer is allocated-and-never-touched today — a clear waste.
- **Risk:** LOW (internal struct change; no API, format, or dep change; all 12 integration tests still pass).
- **Files touched:** `src/lib.rs` (`IndexMapStore` struct + `Drop`, `open_with`, `flush`, `compact`, `flush_scratch`).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   +0.10% / +0.40% / +0.22%   (within noise; consistent positive drift, probably layout-induced)
  - insert_2k_strings: -0.18% / -0.18% / +0.03%   (within noise)
  - lookup_100k:      -0.05% / +0.26% / +0.03%   (within noise)
  - modify_10k:       +0.02% / -0.02% / +0.38%   (within noise)
  - reopen_10k:       +2.89% / +0.95% / +1.56%   (REVERTED — run 1 firmly over +1.5% guard, runs 2 and 3 sit on or above the line)
- **Verdict:** REVERTED.
- **Why:** The expected win — saving the 1MB BufWriter mmap on read-only opens — exists but is small (~1–2us out of 119us). Three offsetting costs eat it and tip the balance negative on reopen_10k: (1) the struct grew by one `Option<File>` (~16 bytes including discriminant + padding), pushing later fields like `scratch` onto a different cache line and shifting where the struct lives in malloc's chunk space; (2) `Drop` now has to discriminate the Option each time a store goes out of scope (the reopen bench drops 1001 stores in the timed loop, so even cheap branches accumulate); (3) the `flush` path the bench calls indirectly through the harness has to discriminate too. On `insert_10k_u64` the same struct grew but the per-call overhead of `flush_scratch`'s extra `is_none()` is amortised across 10k inserts and the lazy upgrade happens only once — small but consistent positive drift (+0.1% to +0.4%) suggests a real but sub-noise cost. The mechanism that paid out at +30% for `bufwriter-capacity-1mb` (mmap allocation is *fast* in this regime) is the same mechanism that makes "skip the mmap entirely" worth less than expected: the alloc was already cheap. Combined with codegen layout effects, net regression.
- **Follow-ups / dead ends:** Closed: lazy-init via `Option<BufWriter>` + `Option<File>` pair pattern. Closed (by extension): any reformulation that puts another Option-discriminant on the hot mutation path — codegen layout shifts on this struct hurt reopen more than the saved alloc helps. Open: lazy-init via a `BufWriter::with_capacity(0, file)` placeholder + in-place upgrade (avoids the Option pair, keeps the struct shape the same, costs an `unsafe` `mem::replace` or a dummy-File constructor) — different shape, MEDIUM risk because of the unsafe. Open: making `BufWriter::with_capacity(cap, file)` truly zero-alloc by passing a file-pre-sized capacity — std doesn't expose this, would need a custom writer, separate hypothesis. Open: `mallopt(M_MMAP_THRESHOLD, 131072)` global anchor — same family but different mechanism, adds libc dep (MEDIUM).

### 2026-05-14 — bufwriter-capacity-2mb

- **Hypothesis:** Bumping `StoreConfig::default().buf_capacity` from 1MB → 2MB might widen the gap above glibc's dynamic `M_MMAP_THRESHOLD` ceiling further, giving an additional small step on `reopen_10k` (which gained -30% going 256KB→1MB). Explicit open follow-up from `bufwriter-capacity-1mb`'s "investigate whether 2MB or 4MB helps further — diminishing returns expected".
- **Risk:** LOW (one-constant change; field stays configurable; no API or semantic impact).
- **Files touched:** `src/lib.rs` (`StoreConfig::default`).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   +0.16% / +0.05% / -0.21%   (within noise)
  - insert_2k_strings: +0.15% / -0.15% / -0.16%   (within noise)
  - lookup_100k:      -0.12% / -0.17% / +0.19%   (within noise)
  - modify_10k:       +0.09% / +0.15% / -0.39%   (within noise)
  - reopen_10k:       -0.68% / +0.61% / -0.61%   (within noise — no consistent direction; one run was tiny improvement, next tiny regression)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** The mechanism that paid out for 256KB→1MB (crossing the dynamic mmap-threshold ceiling consistently) is already saturated at 1MB — going 1MB→2MB doesn't change whether the allocator routes through mmap (it does either way), it just allocates a larger lazily-zeroed VMA. The bench harness exercises a hot path where the buffer is allocated but never written into on `reopen_10k`, so the extra 1MB of capacity is pure overhead per VMA insert, and the cost is small enough to disappear under the ±1% noise band. Confirms the diminishing-returns prediction from `bufwriter-capacity-1mb`'s follow-ups.
- **Follow-ups / dead ends:** Closed: bumping default `buf_capacity` further (the threshold-crossing mechanism is exhausted at 1MB). Closed (by extension): 4MB or 8MB defaults — same logic, slightly worse VMA overhead. Open: dropping the default to a lazy/zero-cost initial state and only allocating the buffer on first mutation (would save the 1MB mmap entirely on read-only opens) — requires struct refactor to `Option<BufWriter>` or enum, LOW-MEDIUM risk. Open: `mallopt(M_MMAP_THRESHOLD, 131072)` at lib init to anchor the threshold globally — adds libc dep, MEDIUM risk.

### 2026-05-14 — mmap-slurp-buffer-min-1mb

- **Hypothesis:** Rounding the replay slurp Vec's capacity to at least 1MB (`Vec::with_capacity((total_on_disk as usize).max(1 << 20))`) would force the allocation consistently through `mmap` the way `bufwriter-capacity-1mb` did — for our 240KB log the static glibc threshold is crossed but the dynamic `M_MMAP_THRESHOLD` can drift higher under repeated bench allocations, occasionally routing the slurp through the heap. Forcing ≥1MB should pin it on the mmap path the same way the BufWriter trick did.
- **Risk:** LOW (one-line capacity bump in `open_with`; no API change, no new deps, unused pages stay lazy).
- **Files touched:** `src/lib.rs` (`open_with` — the `Vec::with_capacity(total_on_disk as usize)` site only).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   +0.13% / -0.35% / -0.15%   (within noise)
  - insert_2k_strings: -0.06% / +0.15% / +0.15%   (within noise)
  - lookup_100k:      -0.23% / -0.17% / +0.14%   (within noise)
  - modify_10k:       -0.07% / +0.09% / +0.04%   (within noise)
  - reopen_10k:       +4.86% / +5.04% / +4.94%   (REVERTED — all three over +1.5% guard, ~5–6us regression on a 119us baseline)
- **Verdict:** REVERTED.
- **Why:** The slurp Vec is materially different from the BufWriter case that benefited from rounding to 1MB: BufWriter's backing buffer is allocated-but-untouched on the read-only reopen path, so the kernel never faults its pages and the mmap cost is purely the syscall + VMA insert (which is small enough that the 1MB version wins through fewer allocator slowdowns elsewhere). The slurp Vec, by contrast, is *immediately* filled by `read_to_end` with ~240KB of bytes — so we touch the first 240KB regardless of the capacity. Asking the kernel to track a 1MB VMA when we use 240KB of it costs ~5us more per reopen than tracking a 240KB VMA, and there is no compensating win because the prior 240KB allocation was already crossing the static 128KB mmap threshold (i.e., already on the mmap path most of the time). Consistent regression (~5%, ~6us) across all three runs makes this clearly worse, not noise.
- **Follow-ups / dead ends:** Closed: rounding the slurp Vec capacity up to force mmap. Closed (by extension): the general "make every per-open allocation ≥1MB so they all sit on the mmap path" pattern — works only for allocations whose pages are NOT touched (like BufWriter), not for ones we read into. Open: dropping the slurp Vec entirely in favor of memory-mapping the log file directly (would eliminate both the allocation AND the userspace `read_to_end` copy) — HIGH risk (`mmap` per the skill's risk tags). Open: streaming the replay through a smaller fixed-size buffer (e.g., 64KB) read in a loop — opposite direction, would let the kernel/page-cache do the reading without our Vec staging; would change the bincode call from `deserialize(borrowed slice)` to a copy out of the streaming buffer, so could lose more than it saves on the per-record path.

### 2026-05-14 — bundle-inconclusive-attempts

- **Hypothesis:** Stacking all still-applicable INCONCLUSIVE attempts in one diff exposes additive signal that each individually buried in ±1.5% noise. The four merged: (1) inline-enum-tag-u8 — manual 1-byte tag + bincode tuple, save 3 bytes/record on Insert and skip serde enum dispatch on replay; (2) inline-hot-path-functions — `#[inline]` on accessors, mutation entry points, `flush_scratch`, `maybe_compact`, `serialize_err`; (3) single-open-for-replay-and-append — one `OpenOptions{read, append, create}` handle for slurp + torn-tail set_len + runtime appends (saves 1–2 open syscalls); (4) skip-path-exists-probe — naturally subsumed by (3), no separate `path.exists()` call. presize-replay-payload-vec was excluded as obsolete: the per-record payload buffer it targeted no longer exists since `slurp-log-into-vec` (KEPT) replaced it with a single up-front Vec already sized to `total_on_disk`. This invocation explicitly bundles multiple hypotheses on user request, deviating from the skill's normal ONE-hypothesis-per-invocation rule.
- **Risk:** MEDIUM (changes on-disk log format via the 1-byte tag — older logs are unreadable; tests confirmed not to depend on byte-level layout).
- **Files touched:** `src/lib.rs` (removed `LogRef`/`LogOwned` enums and the `Deserialize` import; added `TAG_INSERT`/`TAG_REMOVE` constants; rewrote `open_with` to a single OpenOptions handle and manual-tag replay; rewrote `insert`/`remove`/`modify`/`compact` write paths to emit tag + bincode tuple; added `#[inline]` to public accessors, mutation entry points, both private helpers, and the free `serialize_err`).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.46 ms
  - lookup_100k: 630.13 us
  - modify_10k: 5.16 ms
  - reopen_10k: 121.04 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   -0.04% / -0.15% / -0.08%   (within noise)
  - insert_2k_strings: +0.30% / +0.57% / +0.43%   (within noise — directional positive but well under +1.5% guard)
  - lookup_100k:      -0.14% / -0.01% / +0.02%   (within noise)
  - modify_10k:       -1.44% / -1.69% / -1.24%   (directional improvement; 2 of 3 runs clear -1.5%, run 3 misses by 0.26pp — not a gate-passing scenario but moves in the right direction)
  - reopen_10k:       -2.02% / -1.80% / -1.56%   (KEPT — all three ≤ -1.5%, gate cleared by 0.06pp on the tightest run)
- **Verdict:** KEPT.
- **Why:** `reopen_10k` consistently improves -1.56% to -2.02% across all three runs against the fixed pre-change baseline, clearing the -1.5% improvement gate in every run. No scenario regresses past +1.5% in any run (max +0.57% on `insert_2k_strings`, well within the +1.5% guard). The win is modest — roughly 2us shaved off 121us — and barely clears the gate, which is expected because each individual ingredient was previously within ±1.5% noise. The most plausible contributors on the reopen path are: collapsing the three-open sequence to one handle (saves ~one stat + one extra open syscall, ~5–10us in cold-cache territory, though here the inode is hot in cache); the manual 1-byte tag (3 bytes less to slurp + one fewer serde dispatch path per record); and codegen layout shifts from the inline annotations and the dropped enums. `modify_10k` drifted directionally positive across all three runs (-1.24% to -1.69%) — close to gate-clearing — which hints at a real but small write-path benefit from the inlined hot-path attributes and the simpler tag path; doesn't quite clear the bar but is consistent with the inline-enum-tag-u8 entry's earlier observation of "modify_10k -1.1% to -1.4%" individually. The 4-byte → 1-byte tag is the only on-disk format change; tests verified semantic correctness end-to-end (persistence_across_reopen, recovers_from_torn_tail, recovers_from_truncated_payload all pass — the truncated-payload test still hits the `len == 0` and bad-tag early-out paths). The new `bench_results.json` from run 3 becomes the next baseline.
- **Follow-ups / dead ends:** Closed (by KEPT): all four contributing attempts, since they now live in the codebase. Closed (by exclusion): presize-replay-payload-vec — the buffer it targeted no longer exists; do not retry as an independent attempt. Open: targeted `#[inline(always)]` on a single specific callee if profiling later shows a function still on the critical path — the blanket `#[inline]` here is conservative and may leave some calls non-inlined. Open: replacing bincode with a hand-rolled fixed-prefix codec for primitive K/V — the manual tag is now in place, so the next step (skipping bincode entirely for `K: Pod + V: Pod`) is a smaller delta than before; still MEDIUM-risk because it changes the format further. Open: hashing the K once during replay to skip the IndexMap rehash — needs a cached-hash IndexMap variant, doesn't generalize.

### 2026-05-14 — mark-compact-as-cold

- **Hypothesis:** Adding `#[cold]` to `compact()` would tell LLVM the function is rarely called, allowing it to place compact's body far from the hot mutation/open paths in the text segment, improve icache locality on the hot path, and tilt branch prediction so `maybe_compact`'s ratio check is treated as predicted-not-taken.
- **Risk:** LOW (annotation only — no behavior change).
- **Files touched:** `src/lib.rs` (`compact`).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.46 ms
  - lookup_100k: 630.13 us
  - modify_10k: 5.16 ms
  - reopen_10k: 121.04 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   -0.05% / -0.01% / +0.07%   (within noise)
  - insert_2k_strings: +0.52% / +0.29% / +0.23%   (within noise)
  - lookup_100k:      +0.08% / +0.22% / +0.16%   (within noise)
  - modify_10k:       -0.26% / +0.08% / +0.39%   (within noise)
  - reopen_10k:       +3.96% / +3.96% / +3.60%   (REVERTED — all three over +2% guard)
- **Verdict:** REVERTED.
- **Why:** A consistent +3.6%–4.0% regression on `reopen_10k` across all three runs (~5us slower on a 121us baseline). Mechanism is almost certainly binary-layout: with `codegen-units = 1` + ThinLTO + `#[cold]`, the linker moves the compact body to the end of the text segment, which shifts the relative offsets of every function that came after it in the original layout. Some of those functions are on the hot open/replay path (e.g., bincode::deserialize monomorphizations, IndexMap::insert helpers), and the new layout incurs more icache misses for them. Reopen_10k has the smallest p50 of all scenarios (121us), so a ~5us layout cost shows up as a percentage-wise large regression. Insert/modify paths absorb the same shift but their per-iter cost is two orders of magnitude larger, so the layout shift is invisible there.
- **Follow-ups / dead ends:** Closed: blanket `#[cold]` on `compact()`. Closed (by extension): adding `#[cold]` to other rarely-called paths in this crate — the ThinLTO ordering is already near-optimal for the hot loop, and manual hints destabilise it. Open: explicit function ordering via `#[link_section]` / `-Wl,--symbol-ordering-file` to pin hot functions together — needs build-system support, separate hypothesis. Open: profile-guided optimization (PGO) with bench workloads — would let LLVM order functions empirically rather than guess from `#[cold]` hints.

### 2026-05-14 — inline-enum-tag-u8

- **Hypothesis:** Replacing bincode's serde-derived enum encoding (4-byte u32 variant tag + payload) with a manually written 1-byte tag (`0 = Insert`, `1 = Remove`) followed by bincode-serialized payload (`(K, V)` tuple for Insert, `K` for Remove) shrinks each record by 3 bytes (~12% for u64,u64 records) and skips the serde enum-dispatch trait machinery on both the write and replay paths.
- **Risk:** MEDIUM (changes on-disk log format — older logs are unreadable; tests confirmed not to rely on byte-level layout).
- **Files touched:** `src/lib.rs` (removed `LogRef`/`LogOwned` enums, added `TAG_INSERT`/`TAG_REMOVE` constants, rewrote `insert`/`remove`/`modify` write paths, replay loop in `open_with`, and `compact` write loop).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.46 ms
  - lookup_100k: 630.13 us
  - modify_10k: 5.16 ms
  - reopen_10k: 121.04 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   +0.01% / +0.17% / +0.91%   (within noise — drifts positive but under +2% guard)
  - insert_2k_strings: +0.35% / +0.16% / +0.00%   (within noise)
  - lookup_100k:      -0.03% / +0.25% / +0.29%   (within noise)
  - modify_10k:       -1.11% / -1.41% / -1.31%   (within noise — directionally positive but under -3% gate)
  - reopen_10k:       +0.14% / +0.93% / +0.55%   (within noise)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** The 3-byte/record savings translate to ~12% smaller writes but contribute essentially nothing to wallclock because (a) the log lives in the page cache during benches, so disk-size wins don't translate, and (b) bincode's fixint u32 tag is decoded as a single 4-byte load — already cheap. Replay's perceived bottleneck is the K/V deserialize + IndexMap::insert (~7ns/op combined), and skipping the enum discriminant dispatch saves maybe 1-2ns/op which is below noise. `modify_10k` shows a consistent ~1.2% improvement that hints at real-but-tiny signal, but it doesn't cross the -3% gate. Manual tag handling on the read side adds a branch that probably eats most of the saving. Net flat.
- **Follow-ups / dead ends:** Closed: trimming the enum tag from u32 to u8 via manual encoding. Closed (by extension): "make the log records smaller on disk" line for the current bench workload — the cache-resident replay path is bottlenecked by IndexMap::insert and serde dispatch, not bytes. Open: replacing bincode entirely with a hand-rolled codec specialized on `K: Pod + V: Pod` primitive types (changes format, would need a feature flag for non-Pod types) — could skip serde dispatch entirely, MEDIUM risk. Open: caching the hash of K to skip rehashing during replay (presumes K stores its precomputed hash, doesn't generalize). Open: adding `#[cold]` on compact() to give the inline-hot mutation path a better icache layout — separate hypothesis.

### 2026-05-14 — bufwriter-capacity-1mb

- **Hypothesis:** Raising `StoreConfig::default().buf_capacity` from 256KB to 1MB keeps the BufWriter backing buffer comfortably above glibc's dynamic mmap threshold (which starts at 128KB and can be raised by the heuristic up to ~64MB as mmap'd chunks are freed), so the allocator stays on the mmap path even after many alloc/free cycles in tight bench loops.
- **Risk:** LOW (no API or semantic change; default value tweak; field is configurable).
- **Files touched:** `src/lib.rs` (`StoreConfig::default`).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.34 ms
  - insert_2k_strings: 5.47 ms
  - lookup_100k: 632.21 us
  - modify_10k: 5.17 ms
  - reopen_10k: 172.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   -1.78% / -1.31% / -1.40%   (within noise — directionally positive, under -3% gate)
  - insert_2k_strings: +0.05% / -0.01% / -0.07%   (within noise)
  - lookup_100k:      -0.16% / -0.15% / -0.33%   (within noise)
  - modify_10k:       +0.15% / -0.65% / -0.21%   (within noise)
  - reopen_10k:       -29.95% / -29.35% / -29.69%   (KEPT — all three far below -3%)
- **Verdict:** KEPT.
- **Why:** Huge, dead-flat 30% improvement on `reopen_10k` (172us → 121us) reproduced to within 0.3% across three independent runs. Mechanism: glibc's M_MMAP_THRESHOLD is dynamic — when mmap'd allocations are freed, the threshold can rise toward `M_MMAP_MAX`, which means later allocations of similar size silently shift back to the heap, incurring per-call heap fragmentation work. 1MB allocations sit far enough above any plausible heuristic ceiling that the allocator consistently routes through `mmap` (page-aligned, lazily zeroed pages, no zeroing happens since the buffer is allocated-but-unused on the read-path). Mutation paths stay flat (the buffer gets written into either way; ~10k * 24 bytes = 240KB total fits in one flush at either 256KB or 1MB). The new `bench_results.json` from run 3 becomes the next baseline.
- **Follow-ups / dead ends:** Closed: bumping the default to 1MB. Open: investigating whether 2MB or 4MB helps further — diminishing returns expected and at some point committing too much memory hurts on multi-store workloads. Open: explicitly calling `mallopt(M_MMAP_THRESHOLD, 131072)` at lib init to force mmap for smaller buffers too — requires `libc` dep and a once-init, separate hypothesis. Open: replacing the BufWriter with a direct `mmap`-backed writer to skip the userspace copy on the buffered path — HIGH risk (introduces mmap, complex semantics).

### 2026-05-14 — larger-default-bufwriter-capacity

- **Hypothesis:** Raising `StoreConfig::default().buf_capacity` from 64KB to 256KB pushes the BufWriter backing buffer above glibc's default `M_MMAP_THRESHOLD` (128KB), so allocation goes through `mmap` (page-aligned, lazily zeroed) instead of the heap — every `open_with` allocates a buffer, and on the cold-reopen path that allocation is the only writable region we set up.
- **Risk:** LOW (no API or semantic change; `buf_capacity` is already configurable).
- **Files touched:** `src/lib.rs` (`StoreConfig::default`).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.47 ms
  - lookup_100k: 631.23 us
  - modify_10k: 5.12 ms
  - reopen_10k: 188.38 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   -0.22% / -0.66% / -0.42%   (within noise)
  - insert_2k_strings: +0.08% / +0.04% / -0.06%   (within noise)
  - lookup_100k:      -0.01% / +0.14% / +0.16%   (within noise)
  - modify_10k:       -0.42% / +1.30% / +0.92%   (within noise — under +2% guard)
  - reopen_10k:       -9.00% / -8.44% / -8.62%   (KEPT — all three well below -3%)
- **Verdict:** KEPT.
- **Why:** Consistent ~8.5% improvement on `reopen_10k` reproduced to within 0.6% across three independent runs against the fixed pre-change baseline. Mechanism: glibc malloc routes allocations under ~128KB through the heap (which can fragment and requires touching freelist metadata), while allocations at or above the mmap threshold go directly through `mmap`, returning fresh, lazily-zeroed pages. BufWriter only allocates — it doesn't write into the buffer on the read-and-replay path — so the larger allocation is cheaper in the wallclock-relevant work. Mutation paths are flat because they do write into the buffer (touching ~240KB either way), and the kernel page fault cost roughly matches the prior heap-touch cost. The new `bench_results.json` from run 3 becomes the next baseline.
- **Follow-ups / dead ends:** Closed: bumping the default `buf_capacity` to 256KB. Open: tuning further — 512KB or 1MB may give another small step on reopen but risks committing more memory for stores that never write much. Open: investigating whether the `Vec::with_capacity(total_on_disk)` slurp allocation also benefits from mmap-threshold sizing (for our 240KB log it already does). Open: replacing BufWriter entirely with a hand-rolled fixed-stride writer that avoids the dynamic capacity field — different shape, separate hypothesis.

### 2026-05-14 — inline-hot-path-functions

- **Hypothesis:** Adding `#[inline]` to the thin public accessors (`len`, `is_empty`, `contains_key`, `get`, `get_index`, `iter`, `keys`, `values`, `as_index_map`), the mutation entry points (`insert`, `remove`, `modify`), the private helpers (`flush_scratch`, `maybe_compact`), and the free function `serialize_err` lets ThinLTO inline call sites in the bench harness more aggressively, potentially eliminating call overhead on per-iteration hot paths.
- **Risk:** LOW (no behavior change, no API change).
- **Files touched:** `src/lib.rs`.
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.47 ms
  - lookup_100k: 631.23 us
  - modify_10k: 5.12 ms
  - reopen_10k: 188.38 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   -0.20% / -0.01% / -0.42%   (within noise)
  - insert_2k_strings: +0.21% / +0.22% / +0.20%   (within noise)
  - lookup_100k:      -0.18% / +0.06% / -0.22%   (within noise)
  - modify_10k:       +0.56% / +0.65% / +0.92%   (within noise — directionally negative but under +2% guard)
  - reopen_10k:       -0.61% / -0.82% / -0.53%   (within noise — directionally positive but not -3%)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** With `lto = "thin"` and `codegen-units = 1` already set in the bench profile, the compiler is already aggressively cross-crate-inlining hot bench callees based on size heuristics. Marking these `#[inline]` exposes MIR upfront but doesn't change ThinLTO's decisions for functions that were already small enough (e.g., one-line accessors) or already monomorphized in the consuming crate (the generic methods). The `modify_10k` runs drifted slightly positive (+0.6% to +0.9%) — likely codegen layout shuffling rather than real regression — but they stayed under the +2% guard. No scenario hit the -3% improvement gate.
- **Follow-ups / dead ends:** Closed: blanket `#[inline]` on the entire public/private surface. Open: targeted `#[inline(always)]` on a single specific hot callee (e.g., `flush_scratch` only) — would be a different shape and might surface a real signal if there is one, but the diffuse signal here suggests the call overhead simply isn't on the critical path. Open: `#[cold]` on `maybe_compact`'s slow branches (the `compact()` invocation) to keep the no-op fast path hotter in icache — different hypothesis.

### 2026-05-14 — single-open-for-replay-and-append

- **Hypothesis:** Opening the log once with `OpenOptions{read, append, create}` and reusing the same handle for the replay slurp, the torn-tail `set_len`, and the runtime BufWriter appends — instead of doing three separate opens (`File::open` for read, `OpenOptions::write` for truncate, `OpenOptions::create+append` for runtime) — saves one to two `openat` syscalls per `open_with`. `O_APPEND` only affects writes, so an initial `read_to_end` at offset 0 still works.
- **Risk:** LOW (semantically equivalent — `path.exists()` check folds into `total_on_disk > 0` after the always-create open).
- **Files touched:** `src/lib.rs` (`open_with`).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.47 ms
  - lookup_100k: 631.46 us
  - modify_10k: 5.13 ms
  - reopen_10k: 188.38 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   +0.09% / +0.14% / +0.08%   (within noise)
  - insert_2k_strings: -0.04% / -0.04% / -0.37%   (within noise)
  - lookup_100k:      +0.03% / +0.04% / +0.09%   (within noise)
  - modify_10k:       +0.12% / +1.10% / +1.37%   (within noise — drifting positive but under the +2% guard)
  - reopen_10k:       -0.68% / -0.26% / +0.47%   (within noise — no consistent improvement)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** The 1–2 open syscalls saved per call are individually ~5–10us on Linux, but `reopen_10k` includes a fresh `tempdir` per iteration and the kernel inode cache absorbs repeat opens cheaply. Net change buried in jitter. The change was also subtly worse for `modify_10k` runs 2 and 3 — opening the same handle with both read and append modes can change kernel pre-allocation or write-back heuristics, which may explain the small positive drift on the write-heavy scenario.
- **Follow-ups / dead ends:** Closed: combining read/truncate/append opens into one handle. The remaining `open_with` syscall cost is below the bench gate's resolution — further work on the open path is unlikely to clear -3%. Open: replacing bincode entirely for primitive K/V (hand-rolled fixed-prefix codec) — bincode now dominates reopen, but this is MEDIUM risk and changes the on-disk format.

### 2026-05-14 — bincode-varint-encoding

- **Hypothesis:** Switching the bincode codec from the back-compat `bincode::serialize`/`deserialize` helpers (fixint, native-endian) to `bincode::DefaultOptions::new()` (varint, little-endian) shrinks records on disk — small u64 keys/values collapse from 8 bytes to 1 — so both the writer and the slurped replay buffer process fewer bytes.
- **Risk:** MEDIUM (changes on-disk log format — older logs are unreadable; flagged in code comment).
- **Files touched:** `src/lib.rs` (added `codec()` helper, replaced all 5 bincode call sites in `insert`, `remove`, `modify`, `compact`, and the `open_with` replay).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.47 ms
  - lookup_100k: 631.23 us
  - modify_10k: 5.12 ms
  - reopen_10k: 188.38 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   -0.15% / -0.06% / -0.15%   (within noise)
  - insert_2k_strings: -0.14% / -0.04% / -0.13%   (within noise)
  - lookup_100k:      -0.23% / -0.20% / -0.21%   (within noise — `lookup_100k` doesn't touch the codec, expected flat)
  - modify_10k:       -0.32% / -0.11% / -0.36%   (within noise)
  - reopen_10k:       +65.55% / +65.33% / +65.65%   (REVERTED — catastrophic, ~+65% on every run)
- **Verdict:** REVERTED.
- **Why:** Varint decoding is bit-shift-and-branch per integer; against fixint's straight `read_unaligned` load, the per-byte processing cost is much higher. The on-disk savings don't help in benches because the log is in the OS page cache — slurping is already memory-bound, not disk-bound, and we now spend ~123us more parsing the bytes. Writers were flat because mutation cost is dominated by IndexMap insert + BufWriter copy, not by bincode size.
- **Follow-ups / dead ends:** Closed: varint encoding via `bincode::DefaultOptions`. Closed (by extension): general "shrink records on disk" line for in-memory workloads — wins on disk don't translate when reads are cached. Open: hand-rolled fixed-prefix codec specialised for `LogOwned<K, V>` where K/V are sized primitives — could skip the enum dispatch entirely. Still MEDIUM risk because it changes the format.

### 2026-05-14 — slurp-log-into-vec

- **Hypothesis:** Reading the whole log into a `Vec<u8>` via `File::read_to_end` and iterating in-memory over length-prefixed slices removes per-record `BufReader` refills and the memcpy into a separate `payload` buffer that the streaming path needed; `bincode::deserialize` can borrow the slice directly.
- **Risk:** LOW (no public API change, no new dependency — `BufReader` import dropped because it's no longer used).
- **Files touched:** `src/lib.rs` (`open_with`, removed unused `BufReader` import).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 629.80 us
  - modify_10k: 5.11 ms
  - reopen_10k: 208.65 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   -0.12% / -0.13% / -0.11%   (within noise)
  - insert_2k_strings: -0.16% / +0.43% / +0.29%   (within noise)
  - lookup_100k:      +0.04% / +0.22% / +0.17%   (within noise)
  - modify_10k:       -1.10% / -0.34% / -1.10%   (within noise)
  - reopen_10k:       -10.96% / -10.97% / -10.43%   (KEPT — all three < -3%)
- **Verdict:** KEPT.
- **Why:** reopen_10k drops from ~209us to ~188us, reproduced to within 0.5% across three independent runs against the fixed pre-change baseline. The win comes from eliminating per-record `memcpy buffer→payload Vec` and the `BufReader` refill/copy overhead — `bincode::deserialize` now operates on a borrowed slice straight from the slurped buffer. Memory profile changes (one Vec sized to the log) but for our workloads logs are bounded and well under available RAM; for an extremely large log a streaming path may be worth adding back as a fallback.
- **Follow-ups / dead ends:** Closed: `BufReader`-based replay. Open: memory-mapping the log instead of slurping (would skip the userspace copy too — but `mmap` is HIGH risk per the skill). Open: hand-rolled fixed-prefix codec for primitive K/V — bincode now dominates the per-record cost; replacing it with a u64-LE encoder would change the on-disk format and so is a separate, MEDIUM-risk hypothesis.

### 2026-05-14 — skip-path-exists-probe

- **Hypothesis:** Replacing the `path.exists()` + `File::open()` pair with a single `File::open()` (treating `NotFound` as "no existing log") saves one stat syscall per `open`, most visible on `reopen_10k` where the open call is the entire timed work.
- **Risk:** LOW.
- **Files touched:** `src/lib.rs` (`open_with`).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.46 ms
  - lookup_100k: 630.21 us
  - modify_10k: 5.19 ms
  - reopen_10k: 210.11 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   -0.36% / +0.09% / -0.26%   (within noise)
  - insert_2k_strings: +0.31% / +0.29% / +0.68%   (within noise)
  - lookup_100k:      +0.14% / -0.06% / -0.05%   (within noise)
  - modify_10k:       -0.93% / -1.06% / -1.26%   (within noise — directionally positive but not -3%)
  - reopen_10k:       -0.69% / -0.19% / -0.79%   (within noise — directionally positive but not -3%)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** A single stat syscall is ~0.5–2us on Linux; against `reopen_10k`'s 210us p50 that's at most ~1%, well below the -3% gate. The change is technically a small improvement (and removes a benign TOCTOU window between the probe and the open), but the bench gate's noise threshold is the right bar — accept the dead-end so we don't reopen this hypothesis later.
- **Follow-ups / dead ends:** Closed: collapsing `path.exists()` + `File::open()`. Open: combining the post-replay `OpenOptions::new().create(true).append(true).open(&path)` with the existing read handle to save a second open syscall — different shape, separate hypothesis.

### 2026-05-14 — presize-replay-payload-vec

- **Hypothesis:** Initialising the replay `payload` buffer with `Vec::with_capacity(256)` instead of `Vec::new()` saves a couple of early `realloc` calls as the first records grow the buffer.
- **Risk:** LOW.
- **Files touched:** `src/lib.rs` (`open_with`).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.37 ms
  - insert_2k_strings: 5.45 ms
  - lookup_100k: 630.14 us
  - modify_10k: 5.18 ms
  - reopen_10k: 210.31 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   -0.01% / -0.14% / -0.07%   (within noise)
  - insert_2k_strings: +0.25% / +0.00% / +0.20%   (within noise)
  - lookup_100k:      +0.20% / +0.03% / +0.01%   (within noise)
  - modify_10k:       -1.42% / -0.90% / +0.29%   (within noise)
  - reopen_10k:       -0.37% / -0.50% / -0.10%   (within noise — no improvement)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** After cycle 2 the IndexMap pre-sizing already dominates the reopen path; the first record's `resize` allocates a Vec backing store, and subsequent same-size records reuse that capacity for free. Saving two or three reallocations at the top of replay buys nothing measurable next to the per-record bincode deserialize cost.
- **Follow-ups / dead ends:** Closed: pre-sizing the replay payload Vec (no measurable win). Open: slurping the entire log into a Vec<u8> with `read_to_end` so the replay parses from memory rather than via `BufReader::read_exact` — different shape of optimization, separate hypothesis.

### 2026-05-14 — presize-indexmap-from-file-size

- **Hypothesis:** Calling `IndexMap::reserve(file_size / 24)` before the replay loop in `open_with` lets the map skip the ~14 grow-rehash steps it would otherwise do while filling from zero to thousands of entries, cutting cold-reopen latency.
- **Risk:** LOW.
- **Files touched:** `src/lib.rs` (`open_with`).
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.46 ms
  - lookup_100k: 631.70 us
  - modify_10k: 5.13 ms
  - reopen_10k: 357.58 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64:   +0.62% / +0.50% / +0.13%   (within noise)
  - insert_2k_strings: +0.13% / +0.20% / -0.04%   (within noise)
  - lookup_100k:      -0.43% / -0.01% / -0.25%   (within noise)
  - modify_10k:       -0.08% / +0.96% / +0.86%   (within noise)
  - reopen_10k:       -41.20% / -41.28% / -41.18%   (KEPT — all three far below -3%)
- **Verdict:** KEPT.
- **Why:** Massive, dead-flat improvement on `reopen_10k` (~150us shaved off ~360us p50) reproduced to within 0.1% across three independent runs, with all other scenarios within the ±1% noise band — pre-sizing avoids the geometric rehash sequence on the replay path. The 24 bytes/record divisor matches a `Insert<u64,u64>` record; larger records over-reserve harmlessly because IndexMap only allocates one hash-table backing array and never shrinks during replay.
- **Follow-ups / dead ends:** Closed: file-size-based replay capacity hint. Open: tuning the divisor for string-heavy workloads (currently over-reserves for `insert_2k_strings`-shaped data — wastes some memory, doesn't help further). Open: pre-sizing the replay `payload` Vec from the largest length seen so far (separate hypothesis, smaller potential payoff now that reopen_10k is already much faster). Open: faster hasher (foldhash/ahash) — would touch mutation paths too, MEDIUM risk because it adds a dep.

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
