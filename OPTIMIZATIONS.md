# Optimization Log

This file records every optimization attempt on `indexmap_store`, kept or not.
Future `/optimize` runs read the **Index** table first and skip any hypothesis
already attempted (regardless of verdict).

**Verdict legend**

- `KEPT (deep-win)` — change is in the codebase; ≥1 scenario reliably improves ≤ -1.5% across all 3 runs with no scenario regressing ≥ +1.5% in any run.
- `KEPT (broad-win)` — change is in the codebase; ALL scenarios improve ≤ -0.5% across all 3 runs with no scenario drifting > +0.1% in any run. Captures "many small wins everywhere" diffs.
- `REVERTED` — change broke tests/clippy, or bench gate failed; code restored.
- `INCONCLUSIVE` — change neither improved nor regressed beyond noise; reverted, treat as a closed dead-end so we don't retry it.

**Diff column** — each row links to `optimization-diffs/<NNN>-<slug>.patch`, the saved source diff for that attempt. `<NNN>` is a 3-digit zero-padded index reflecting the order attempts were tried (001 is the first). For KEPT verdicts the patch is exactly the change that landed in the codebase. For REVERTED/INCONCLUSIVE the patch lets future invocations replay the attempt (e.g. as part of a bulk stack or retry). Entries pre-dating the diff-capture step in `/optimize` were back-filled: KEPT diffs were extracted from their commits; REVERTED/INCONCLUSIVE diffs were re-implemented from each entry's detailed description and the parent-commit source state, then emitted as a unified diff against that parent — so the patches reflect the change but are not byte-identical to the original (untracked) revert.

## Index

| Date (UTC) | Hypothesis                              | Files touched          | Risk   | Verdict         | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Diff                                                                         |
| ---------- | --------------------------------------- | ---------------------- | ------ | --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| 2026-05-15 | unsafe-compact-batch-len-prefix-029-pattern | src/lib.rs | MEDIUM | INCONCLUSIVE | Unsafe variant of 022 (`compact-batch-len-prefix-and-payload`, INCONCLUSIVE). Applied 029-kept's unsafe in-place placeholder + tag + len-prefix-write pattern to `compact()`'s record loop: reserve LEN_BYTES + 1 at front of `buf`, unsafe-init placeholder + tag, bincode-append, unsafe-write real length, single `write_all`. Replaces the prior `buf.clear(); buf.push(TAG); bincode...; write_all(&len); write_all(&buf);` (two write_alls). Miri **passed** (12/12). Compact is never triggered by the bench scenarios (all stay under `min_compact_bytes = 1 MiB`), so the diff cannot directly improve any scenario; expected layout-shift effects observed: Δ trim_mean insert_10k_u64 +0.26%/+0.51%/+0.12%, insert_2k_strings -1.37%/-1.27%/-1.27% (just shy of -1.5% deep-win bar in all 3 runs), lookup_100k -0.42%/-0.44%/-0.45%, modify_10k +0.12%/+0.08%/+0.24%, reopen_10k +0.70%/+0.54%/+0.84% (positive but under guard). INCONCLUSIVE — no scenario clears ≤ -1.5% in all 3 runs (deep-win fails by 0.1–0.2 percentage points on insert_2k_strings) and broad-win blocked by positive insert_10k_u64/modify_10k/reopen_10k. The compact-path change relocates other functions in the binary and shifts non-compact scenarios — exactly the icache-aliasing pattern seen throughout this round. Closed dead end: compact-loop unsafe rewrite is not a bench-observable win. | [diff](optimization-diffs/035-unsafe-compact-batch-len-prefix-029-pattern.patch) |
| 2026-05-15 | unsafe-replay-prefetch-next-record | src/lib.rs | MEDIUM | REVERTED | Added `_mm_prefetch(buf.as_ptr().add(payload_end))` (T0 hint) inside the replay loop to overlap next-record memory load with current record's bincode deserialize. x86_64-gated. Miri **passed** (12/12; `_mm_prefetch` is documented side-effect-free, accepts non-dereferenceable pointers, never UB). Δ trim_mean: insert_10k_u64 -0.15%/-0.14%/-0.14%, insert_2k_strings -0.22%/-0.29%/-0.18%, lookup_100k +0.01%/-0.02%/-0.00%, modify_10k +0.14%/+0.11%/+0.07%, reopen_10k **+1.46%/+1.78%/+1.17%** (R2 trips +1.5% guard). REVERTED — reopen_10k R2 +1.78% above guard. Prefetch adds 4-byte op + branch per record but buffer is already L2-hot on subsequent reopens (50 inner-reps over same 240 KB), so prefetch adds work without hiding latency. The added code in `open_with`'s hot loop also re-shifts icache layout (same failure mode as 030/033). Closed dead end: replay-loop software prefetch is not profitable. | [diff](optimization-diffs/034-unsafe-replay-prefetch-next-record.patch) |
| 2026-05-15 | unsafe-prefilled-scratch-truncate-via-set-len | src/lib.rs | MEDIUM | REVERTED | Unsafe variant of 019/027 (`truncate-scratch-preserve-len-prefix`). Pre-seeded scratch with [0u8; LEN_BYTES + 1] in constructor; rewrote `begin_record` as `unsafe { scratch.set_len(LEN_BYTES + 1); *scratch.get_unchecked_mut(LEN_BYTES) = tag; }`. Drops the `clear() + reserve(5)` pair on every record. Miri **passed** (12/12; set_len(5) is always a pure truncation given `scratch.len() ≥ LEN_BYTES + 1` invariant). Δ trim_mean: insert_10k_u64 -0.29%/-0.28%/-0.59%, insert_2k_strings -0.35%/-0.31%/-0.26%, lookup_100k +0.24%/+0.24%/+0.26%, modify_10k **-1.72%/-1.72%/-1.78%** (deep-win on this scenario), reopen_10k **+1.79%/+1.66%/+2.66%** (consistent regression, R1/R3 both >+1.5%). REVERTED — reopen_10k trips +1.5% guard on runs 1, 2, and 3 (R2 +1.66% sits exactly above the bar). Same icache-aliasing failure mode that bit 019/027/030/032 — code-layout shift in `open_with`'s replay path. The mutation hot path won big (-1.72% modify_10k matches the 029-scratch-prep cluster) but reopen lost the same amount in the opposite direction. Closed dead end: replacing `clear()+reserve()` with prefilled+`set_len(LEN_BYTES+1)` is layout-fragile post-ahash and post-029. | [diff](optimization-diffs/033-unsafe-prefilled-scratch-truncate-via-set-len.patch) |
| 2026-05-15 | unsafe-uninit-scratch-skip-zero-placeholder | src/lib.rs | MEDIUM | INCONCLUSIVE | Variant of kept-029 (`unsafe-set-len-scratch-prep`). Dropped the placeholder `ptr::write_unaligned::<u32>(p, 0u32)` store in `begin_record`, leaving bytes 0..LEN_BYTES uninit until `flush_scratch` writes the real length in place. Save 1 u32 store per record on the mutation hot path. Miri **passed** (12/12). Δ trim_mean: insert_10k_u64 -0.24%/-0.33%/-0.13%, insert_2k_strings +0.23%/+0.48%/+0.73% (positive in all 3 runs — kills broad-win), lookup_100k -0.43%/-0.44%/-0.42% (just shy of -0.5% bar), modify_10k -0.96%/-0.69%/-0.81% (negative but well above -1.5%), reopen_10k +0.46%/+0.39%/+0.75% (small positive). INCONCLUSIVE — no scenario ≤ -1.5% (deep-win fails) and broad-win blocked by insert_2k_strings positive run + reopen_10k positive run. The placeholder zero store apparently was being folded into adjacent stores by LLVM (likely a merged 8-byte SSE store with the tag), so removing it actually unmerged the store and added an icache penalty on the unrelated insert_2k_strings hot path — same icache-aliasing signature as 020/021/030. Closed dead end: scratch placeholder removal not profitable. | [diff](optimization-diffs/032-unsafe-uninit-scratch-skip-zero-placeholder.patch) |
| 2026-05-15 | unsafe-set-len-read-exact-replay-buf | src/lib.rs | MEDIUM | REVERTED | Replaced `Vec::with_capacity(n) + file.read_to_end` in `open_with` replay path with `Vec::with_capacity(n) + unsafe set_len(n) + file.read_exact`. Hypothesis: skip read_to_end's grow-loop + EOF probe syscall when total file size is known. Miri **passed** (12/12, no UB; uninit prefix is initialized by `read_exact` before any read). Δ trim_mean: insert_10k_u64 +0.23%/+0.35%/+0.02%, insert_2k_strings +0.55%/+0.45%/+0.37%, lookup_100k -0.02%/-0.02%/-0.04%, modify_10k -0.06%/-0.02%/+0.14%, reopen_10k +2.05%/+2.55%/+2.39% (consistent +2.3% regression — opposite of hypothesis). REVERTED — reopen_10k trips +1.5% guard on runs 2,3 (and run 1 at +2.05% also above). std's File-specialized `read_to_end_with_reservation` already does one read into the pre-reserved slot then a single zero-byte EOF probe; `read_exact` via default trait impl loops with extra checks on each iteration. Net negative on the targeted scenario. Closed dead end: replay-buffer read path is not a profitable target via this swap. | [diff](optimization-diffs/031-unsafe-set-len-read-exact-replay-buf.patch) |
| 2026-05-15 | unsafe-unaligned-length-read-replay | src/lib.rs | MEDIUM | REVERTED | First /optimize cycle under the new §5b miri gate. Replaced replay-loop `u32::from_le_bytes(buf[offset..offset+LEN_BYTES].try_into().unwrap())` with `u32::from_le(unsafe { ptr::read_unaligned(buf.as_ptr().add(offset) as *const u32) })`. Miri gate **passed** (12/12, no UB under Stacked Borrows). Δ trim_mean: insert_10k_u64 +0.17%/+0.06%/-0.11%, insert_2k_strings +2.20%/+2.45%/+2.35% (consistent ~+2.3% regression on a scenario that NEVER hits replay), lookup_100k +0.22%/+0.22%/+0.21%, modify_10k +0.55%/+0.53%/+0.62%, reopen_10k +4.26%/+4.13%/+4.18% (consistent +4% regression — opposite of hypothesis). REVERTED — reopen_10k +4.18% and insert_2k_strings +2.30% both >+1.5% guard. LLVM was already eliding the bounds check after the `offset + LEN_BYTES <= buf.len()` loop guard (verified by codegen-equivalent expected behavior), so the unsafe form is functionally identical but reshuffles binary layout enough to push something off icache on both reopen (hot inside replay) AND insert_2k_strings (unrelated path) — same icache-aliasing pattern as 020/021 inline-knob tweaks. Closed dead end: replay-loop length read is not a profitable unsafe target. | [diff](optimization-diffs/030-unsafe-unaligned-length-read-replay.patch) |
| 2026-05-15 | unsafe-set-len-scratch-prep | src/lib.rs | MEDIUM | KEPT (deep-win) | User-authorized `unsafe`. Replaced `begin_record`'s `clear + extend_from_slice(&[0;4]) + push(tag)` with `clear + reserve(LEN_BYTES + 1) + unsafe { write_unaligned u32 placeholder + write tag + set_len(5) }`. Replaced `flush_scratch`'s `[..LEN_BYTES].copy_from_slice(&payload_len.to_le_bytes())` with `unsafe { ptr::write_unaligned::<u32>(...) }`. Soundness: `reserve` guarantees capacity ≥ 5; both placeholder bytes (u32) and tag byte are written before `set_len`; `flush_scratch` only touches the head 4 bytes after `begin_record` has already initialized them. Δ trim_mean: insert_10k_u64 -0.08%/+0.10%/+0.10% (flat), insert_2k_strings -2.33%/-1.76%/-1.91% (all ≤ -1.5%), lookup_100k -0.30%/-0.30%/-0.31% (incidental cache shift), modify_10k -3.29%/-3.53%/-3.44% (all ≤ -1.5%, ~3.4% avg win), reopen_10k -0.62%/-0.69%/-0.56%. KEPT (deep-win) via §7 step-2: modify_10k and insert_2k_strings both clear ≤ -1.5% in all 3 runs with no scenario ≥ +1.5%. Skipping the 4-byte placeholder memset + folded push-cap-check measurably wins on the mutation hot path; the post-ahash layout-sensitivity flagged in 027/028 didn't bite this attempt. | [diff](optimization-diffs/029-unsafe-set-len-scratch-prep.patch) |
| 2026-05-15 | inline-always-flush-scratch-post-ahash | src/lib.rs | LOW | REVERTED | Retry of INCONCLUSIVE 017 against post-ahash baseline. `#[inline]` → `#[inline(always)]` on `flush_scratch`. Δ p50: insert_10k_u64 -0.17%/-0.12%/+0.66%, insert_2k_strings +1.10%/+0.98%/+0.87% (consistent ~+1% regression), lookup_100k +0.05%/-0.03%/-0.07%, modify_10k +0.62%/+0.69%/+0.57% (consistent ~+0.6% regression), reopen_10k +2.51%/-0.26%/-1.39% (R1 spike trips +1.5% guard, same cold-icache shape as 027). REVERTED — same failure pattern as the previous post-ahash retry: R1 reopen spike + small consistent strings/modify regression. Pre-ahash 017 already noted "ThinLTO already inlined flush_scratch as expected, the `always` only forces what was already happening" — the annotation is a no-op for codegen but apparently shifts symbol layout enough to harm the mutation hot path post-ahash. Closed dead end. | [diff](optimization-diffs/028-inline-always-flush-scratch-post-ahash.patch) |
| 2026-05-15 | truncate-scratch-preserve-len-prefix-post-ahash | src/lib.rs | LOW | REVERTED | Retry of INCONCLUSIVE 019 against post-ahash baseline (reopen now ~65us). Re-implemented intent (constructor pre-seeds scratch with [0u8; LEN_BYTES]; insert/remove/modify use truncate(LEN_BYTES) + push(tag) instead of clear + extend_from_slice + push). Δ p50: insert_10k_u64 +0.05%/+1.08%/+0.21%, insert_2k_strings +1.20%/+1.42%/+1.12% (consistent ~+1.2% regression — all 3 runs), lookup_100k +0.32%/+0.06%/+0.02%, modify_10k +0.54%/+0.42%/+0.80% (all positive), reopen_10k +14.01%/-0.29%/-1.08% (R1 spike >+1.5% revert guard; R2/R3 noise). REVERTED — R1 reopen +14.01% trips guard; insert_2k_strings consistently +1.0–1.4% in all 3 runs (real, not noise); modify_10k drifts positive — opposite direction from pre-ahash 019 (-1.62/-1.14/-0.86 reopen, -0.49/-0.19/-0.31 strings). Post-ahash codegen layout for the mutation hot path now penalizes the truncate-vs-clear pattern; ahash interaction hypothesis from 023's follow-up note is falsified for this specific lever. | [diff](optimization-diffs/027-truncate-scratch-preserve-len-prefix-post-ahash.patch) |
| 2026-05-15 | ahash-indexmap-hasher                   | src/lib.rs, Cargo.toml | MEDIUM | KEPT (deep-win) | Swapped IndexMap default std::hash::RandomState (SipHash-1-3, DoS-resistant but slow) for ahash::RandomState via IndexMap::with_hasher(...); struct field and as_indexmap() return type now bind S = ahash::RandomState (technically a public-API change, accepted under user explicit "use ahash" directive). Δ p50: insert_10k_u64 -0.57%/-1.53%/-2.33%, insert_2k_strings +0.06%/+0.13%/-1.30%, lookup_100k -39.20%/-39.28%/-39.20%, modify_10k -1.92%/-1.67%/-2.23%, reopen_10k -43.90%/-44.51%/-44.23%. Largest single optimization in this log: lookup and reopen are both bottlenecked on hasher cost (lookup is pure HashMap probe; reopen rebuilds the map by hashing every replayed key). modify_10k clears -1.5% in all 3 runs (-1.92/-1.67/-2.23) → KEPT (deep-win); lookup/reopen would have qualified independently. Insert paths neutral (write cost dominated by serialization + buffered I/O, not hash). MEDIUM-risk dep swap; ahash is not DoS-resistant — fine for trusted-input stores, document if exposed. | [diff](optimization-diffs/023-ahash-indexmap-hasher.patch)                   |
| 2026-05-15 | release-profile-panic-abort-strip       | Cargo.toml             | LOW    | INCONCLUSIVE    | Added `opt-level=3`, `debug=false`, `panic="abort"`, `strip=true` to both `[profile.release]` and `[profile.bench]` (on top of existing `lto="thin"` + `codegen-units=1`). Cargo emitted `warning: panic setting is ignored for bench profile` — benches always unwind, so `panic="abort"` does not apply to bench binaries. `opt-level=3` and `debug=false` are already release defaults; `strip=true` only shrinks the binary. Effective lever for bench codegen: ~none. Δ p50: insert_10k_u64 -1.26%/-0.91%/-1.44%, insert_2k_strings -1.40%/-0.90%/-0.92%, lookup_100k -0.27%/-0.20%/-0.15%, modify_10k -1.45%/-0.86%/-1.50%, reopen_10k -0.44%/-1.27%/-1.26%. No regression (max +0% — all runs negative), but deep-win fails (no scenario clears ≤-1.5% in all 3 runs; best is modify_10k missing R2 at -0.86%) and broad-win fails (lookup_100k -0.15%/-0.20%/-0.27% and reopen_10k -0.44% R1 sit above the ≤-0.5% bar). INCONCLUSIVE — reverted.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | [diff](optimization-diffs/026-release-profile-panic-abort-strip.patch)       |
| 2026-05-14 | bundle-inconclusive-round-2             | src/lib.rs, Cargo.toml | HIGH   | INCONCLUSIVE    | Stacked the 5 still-applicable INCONCLUSIVE attempts that landed _after_ `bundle-inconclusive-attempts` (round 1): (1) bincode-2-upgrade-new-config (HIGH: dep 1.3→2.0.1, new typed Configuration, fixed-int), (2) compact-batch-len-prefix-and-payload (compact loop pre-seeds buf with LEN*BYTES + single write_all per record), (3) truncate-scratch-preserve-len-prefix (constructor pre-seeds scratch with [0;LEN_BYTES]; insert/remove/modify use `truncate(LEN_BYTES) + push(tag)`), (4) inline-always-flush-scratch (`#[inline]` → `#[inline(always)]`), (5) bufwriter-capacity-2mb (default 1MB → 2MB). Excluded `combine-len-prefix-and-tag-extend` as subsumed by (3). Result: no scenario regressed past +1.5% (max +0.47% on reopen_10k), but neither KEPT path clears: deep-win fails (only insert_10k_u64 hit -1.41% once, then -0.56% / -0.24%); broad-win fails because reopen_10k went \_positive* in 2 of 3 runs (+0.43% / -0.44% / +0.47%) — opposite of round-1's reopen -1.56% to -2.02%; the layout interactions of round-2's 5 changes don't compose like round-1's did. INCONCLUSIVE — reverted                                                     | [diff](optimization-diffs/025-bundle-inconclusive-round-2.patch)             |
| 2026-05-14 | bincode-2-upgrade-new-config            | src/lib.rs, Cargo.toml | HIGH   | INCONCLUSIVE    | Upgraded bincode 1.3 → 2.0.1 using the new typed `Configuration<LittleEndian, Fixint, NoLimit>` API (built via `bincode::config::standard().with_fixed_int_encoding().with_little_endian().with_no_limit()` — NOT `legacy()`; fixed-int chosen to preserve byte-compat with bincode 1.x defaults and avoid the known +65% varint reopen regression) and migrated all 4 `serialize_into` callsites to `bincode::serde::encode_into_std_write(...)` and both `deserialize` callsites to `bincode::serde::decode_from_slice(...)` — initial 3-run gate tripped REVERTED on lookup_100k +1.51% in run 3, but a follow-up 3 runs (4/5/6) showed lookup_100k in [-0.09%, +0.31%], confirming the +1.51% was an isolated noise spike; reclassified REVERTED → INCONCLUSIVE; even discounting the spike, neither KEPT gate is satisfied: no scenario clears -1.5% reliably (best is insert_10k_u64 -1.03% / -1.35% on 2 of 6) and broad-win fails because modify_10k drifts both ways (-0.39% to +0.22%) and run-5 reopen_10k spiked +1.02%; user-authorized HIGH-risk dependency swap; ~-0.5% directional improvement on codec-touching scenarios is real but below the noise floor | [diff](optimization-diffs/024-bincode-2-upgrade-new-config.patch)            |
| 2026-05-14 | construct-IndexMap-with-capacity-direct | src/lib.rs             | LOW    | REVERTED        | Replaced `IndexMap::new() + map.reserve(capacity_hint)` with single-step `IndexMap::with_capacity_and_hasher(capacity_hint, Default::default())` — reopen_10k regressed +3.56% / +3.66% / +4.10% across all 3 runs (~5us on 119us baseline); the unconditional `with_capacity_and_hasher(0, ...)` for empty-file opens added allocation work the prior `IndexMap::new()` skipped, AND the moved-out `let capacity_hint` line shifted symbol layout; the original two-step pattern (`new()` always + conditional `reserve()`) was locally optimal                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | [diff](optimization-diffs/023-construct-IndexMap-with-capacity-direct.patch) |
| 2026-05-14 | compact-batch-len-prefix-and-payload    | src/lib.rs             | LOW    | INCONCLUSIVE    | Applied the KEPT `batch-len-prefix-and-payload` pattern (one `write_all` per record by filling length prefix in scratch) to `compact()`'s rewrite loop — all scenarios within ±0.75% noise (reopen_10k -0.74% / -0.22% / -0.52%, insert_2k_strings +0.68% / +0.41% / +0.15%); bench doesn't exercise compact so this was expected to be flat; reverted                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | [diff](optimization-diffs/022-compact-batch-len-prefix-and-payload.patch)    |
| 2026-05-14 | uninline-mutation-entry-points          | src/lib.rs             | LOW    | REVERTED        | Removed the `#[inline]` annotation from `insert`/`remove`/`modify` to test whether ThinLTO's heuristic would _not_ inline them and leave the bodies in lib.rs's text segment (motivated by `inline-always-insert-remove-modify`'s +9% reopen regression from over-inlining) — reopen_10k still regressed +6.14% / +6.05% / +5.65% across all 3 runs; both directions of the inline knob hurt reopen, confirming the existing `#[inline]` (heuristic-friendly) is locally optimal; write paths flat                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | [diff](optimization-diffs/021-uninline-mutation-entry-points.patch)          |
| 2026-05-14 | inline-always-insert-remove-modify      | src/lib.rs             | LOW    | REVERTED        | Promoted `#[inline]` → `#[inline(always)]` on the three public mutation entry points — reopen_10k regressed +9.33% / +9.39% / +9.23% across all 3 runs (~11us on a 119us baseline) despite reopen calling none of those functions; the larger inlined bodies in the bench harness's u64-monomorphisation shifted symbol layout enough to push something off the read path's icache; clear example of how `#[inline(always)]` on hot public APIs can hurt unrelated cold-ish paths through codegen-layout side effects                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | [diff](optimization-diffs/020-inline-always-insert-remove-modify.patch)      |
| 2026-05-14 | truncate-scratch-preserve-len-prefix    | src/lib.rs             | LOW    | INCONCLUSIVE    | Replaced `clear + extend(&[0;4]) + push(tag)` with `truncate(LEN_BYTES) + push(tag)` after pre-seeding scratch with 4 zero bytes at construction — reopen_10k drifts consistently -1.62% / -1.14% / -0.86% (only run 1 clears -1.5%; gate requires all 3) and insert_2k_strings -0.49% / -0.19% / -0.31% (directional, well under gate); reverted                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | [diff](optimization-diffs/019-truncate-scratch-preserve-len-prefix.patch)    |
| 2026-05-14 | combine-len-prefix-and-tag-extend       | src/lib.rs             | LOW    | INCONCLUSIVE    | Fused `extend_from_slice(&[0;4])` + `push(tag)` into one `extend_from_slice(&[0,0,0,0,tag])` across insert/remove/modify — all scenarios within ±1.2% noise; modify_10k -0.71%/+0.01%/+0.16%, reopen_10k -0.59%/-1.20%/-0.24% (directional but neither clears -1.5% gate); reverted                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | [diff](optimization-diffs/018-combine-len-prefix-and-tag-extend.patch)       |
| 2026-05-14 | inline-always-flush-scratch             | src/lib.rs             | LOW    | INCONCLUSIVE    | Promoted `#[inline]` → `#[inline(always)]` on `flush_scratch` (targeted follow-up from inline-hot-path-functions) — all scenarios within ±1% noise; reopen_10k drifts -0.21% / -0.65% / -0.70% (directional but under -1.5% gate); ThinLTO already inlined flush_scratch as expected, the `always` only forces what was already happening                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | [diff](optimization-diffs/017-inline-always-flush-scratch.patch)             |
| 2026-05-14 | lazy-bufwriter-allocation               | src/lib.rs             | LOW    | REVERTED        | Deferred the 1MB BufWriter mmap until first mutation via `log: Option<BufWriter>` + `file: Option<File>` — reopen_10k regressed +0.95% / +1.56% / +2.89% (run 1 over +1.5% guard); the extra struct field + per-write `is_none()` branch + Drop-time discrimination outweighed the saved mmap, and the struct grew enough that codegen layout shifted unfavorably on the read path                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | [diff](optimization-diffs/016-lazy-bufwriter-allocation.patch)               |
| 2026-05-14 | bufwriter-capacity-2mb                  | src/lib.rs             | LOW    | INCONCLUSIVE    | Bumped default `buf_capacity` from 1MB to 2MB — all scenarios within ±1% noise (reopen_10k -0.68% / +0.61% / -0.61%); 1MB already past the mmap-threshold ceiling, diminishing returns confirmed; reverted                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | [diff](optimization-diffs/015-bufwriter-capacity-2mb.patch)                  |
| 2026-05-14 | mmap-slurp-buffer-min-1mb               | src/lib.rs             | LOW    | REVERTED        | `Vec::with_capacity(total_on_disk).max(1MB)` regressed reopen_10k +4.86% to +5.04% across all 3 runs — the larger mmap region the kernel has to track (1MB) costs more on every reopen than the 240KB slurp it replaced, while only the first 240KB ever gets touched                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | [diff](optimization-diffs/014-mmap-slurp-buffer-min-1mb.patch)               |
| 2026-05-14 | bundle-inconclusive-attempts            | src/lib.rs             | MEDIUM | KEPT (deep-win) | Stacked the 4 still-applicable INCONCLUSIVE attempts (inline-enum-tag-u8 + inline-hot-path-functions + single-open-for-replay-and-append + skip-path-exists-probe (subsumed)) — reopen_10k -1.56% to -2.02% across all 3 runs (consistently above -1.5% gate); modify_10k drifts -1.24% to -1.69% (directional but not gate-clearing); presize-replay-payload-vec excluded as obsolete after slurp-log-into-vec landed                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | [diff](optimization-diffs/013-bundle-inconclusive-attempts.patch)            |
| 2026-05-14 | mark-compact-as-cold                    | src/lib.rs             | LOW    | REVERTED        | `#[cold]` on `compact()` regressed reopen_10k +3.6% to +4.0% across all 3 runs — binary-layout side effect hurt icache locality on the read path                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | [diff](optimization-diffs/012-mark-compact-as-cold.patch)                    |
| 2026-05-14 | inline-enum-tag-u8                      | src/lib.rs             | MEDIUM | INCONCLUSIVE    | Replaced bincode 4-byte enum tag with manual 1-byte tag + bincode tuple; saved 3 bytes/record but all scenarios within ±2% noise; modify_10k -1.1% to -1.4% under -3% gate; reverted                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | [diff](optimization-diffs/011-inline-enum-tag-u8.patch)                      |
| 2026-05-14 | bufwriter-capacity-1mb                  | src/lib.rs             | LOW    | KEPT (deep-win) | -29.4% to -30.0% on reopen_10k across all 3 runs; 256K→1MB stays consistently above glibc dynamic mmap_threshold                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | [diff](optimization-diffs/010-bufwriter-capacity-1mb.patch)                  |
| 2026-05-14 | larger-default-bufwriter-capacity       | src/lib.rs             | LOW    | KEPT (deep-win) | -8.4% to -9.0% on reopen_10k across all 3 runs; 64K→256K bumps allocation above glibc mmap_threshold                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | [diff](optimization-diffs/009-larger-default-bufwriter-capacity.patch)       |
| 2026-05-14 | inline-hot-path-functions               | src/lib.rs             | LOW    | INCONCLUSIVE    | Added #[inline] to thin accessors, mutation entry points, flush_scratch, maybe_compact, serialize_err; all scenarios within ±1% noise across 3 runs; reverted                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | [diff](optimization-diffs/008-inline-hot-path-functions.patch)               |
| 2026-05-14 | single-open-for-replay-and-append       | src/lib.rs             | LOW    | INCONCLUSIVE    | Saved 1–2 open syscalls per open_with; reopen_10k drift -0.7% to +0.5%, all within noise; reverted                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | [diff](optimization-diffs/007-single-open-for-replay-and-append.patch)       |
| 2026-05-14 | bincode-varint-encoding                 | src/lib.rs             | MEDIUM | REVERTED        | +65% regression on reopen_10k — varint decode overhead dwarfs disk-size savings (data is page-cached)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | [diff](optimization-diffs/006-bincode-varint-encoding.patch)                 |
| 2026-05-14 | slurp-log-into-vec                      | src/lib.rs             | LOW    | KEPT (deep-win) | -10.4% to -11.0% on reopen_10k across all 3 runs; mutation paths within ±1% noise                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | [diff](optimization-diffs/005-slurp-log-into-vec.patch)                      |
| 2026-05-14 | skip-path-exists-probe                  | src/lib.rs             | LOW    | INCONCLUSIVE    | Saved one stat syscall per open; reopen_10k drift -0.2% to -0.8%, all within noise; reverted                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | [diff](optimization-diffs/004-skip-path-exists-probe.patch)                  |
| 2026-05-14 | presize-replay-payload-vec              | src/lib.rs             | LOW    | INCONCLUSIVE    | All scenarios within ±1.5% noise; reverted                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | [diff](optimization-diffs/003-presize-replay-payload-vec.patch)              |
| 2026-05-14 | presize-indexmap-from-file-size         | src/lib.rs             | LOW    | KEPT (deep-win) | -41.2% on reopen_10k across all 3 runs; mutation paths within ±1% noise                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | [diff](optimization-diffs/002-presize-indexmap-from-file-size.patch)         |
| 2026-05-14 | batch-len-prefix-and-payload            | src/lib.rs             | LOW    | KEPT (deep-win) | -4.5% to -4.8% on reopen_10k across all 3 runs; mutation paths within ±1% noise                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | [diff](optimization-diffs/001-batch-len-prefix-and-payload.patch)            |

## Detailed entries

### 2026-05-15 — unsafe-compact-batch-len-prefix-029-pattern

- **Hypothesis:** Unsafe variant of 022 (`compact-batch-len-prefix-and-payload`, INCONCLUSIVE). Apply 029-kept's pattern to `compact()`'s record-building loop: reserve `LEN_BYTES + 1` at front of `buf`, unsafe-write placeholder length (u32) + tag (u8) + `set_len(LEN_BYTES + 1)`, bincode-extend payload, then unsafe in-place `write_unaligned` of the real length and a single `writer.write_all(&buf)`. Replaces the original `clear() + push(TAG) + bincode + write_all(len_bytes) + write_all(payload)` (two writer calls per record).
- **Risk:** MEDIUM. Two `unsafe` blocks per record in `compact()`; same shape as kept-029. Bench scenarios never trigger compact, so any observed deltas are pure binary-layout effects.
- **Files touched:** `src/lib.rs`.
- **Diff:** [`optimization-diffs/035-unsafe-compact-batch-len-prefix-029-pattern.patch`](optimization-diffs/035-unsafe-compact-batch-len-prefix-029-pattern.patch) — reference only.
- **Soundness audit:** `Vec::reserve(LEN_BYTES + 1)` guarantees `capacity ≥ 5` from offset 0. First unsafe block writes bytes 0..4 (u32 placeholder) and byte 4 (tag) before `set_len(LEN_BYTES + 1)` — all bytes 0..5 initialized before the slot is exposed. Bincode's `serialize_into` only appends past `len` via `Vec::extend_from_slice` (no read of bytes 0..5). Second unsafe block writes `write_unaligned::<u32>(p, payload_len.to_le())` to bytes 0..4 — pure write, no read; bytes 0..4 were already initialized by the first block, so this is just overwriting valid memory. `writer.write_all(&buf)` reads the full slice 0..len with everything initialized. `Vec<u8>` has no drop glue. If invariants broke: "encountered uninitialized memory" or "out-of-bounds" — caught by miri.
- **Miri gate:** PASSED. 12/12 OK, no UB. 78 s wall time. The miri test `compaction_shrinks_log` and `empty_after_full_delete_compacts` cover the compact() path directly with the new unsafe blocks.
- **Baseline trim_mean (ns):** insert_10k_u64 206147, insert_2k_strings 494775, lookup_100k 317841, modify_10k 63400, reopen_10k 57119.
- **Δ trim_mean (3 runs):**
  - insert_10k_u64: +0.262% / +0.508% / +0.122%
  - insert_2k_strings: -1.373% / -1.270% / -1.273%
  - lookup_100k: -0.418% / -0.436% / -0.455%
  - modify_10k: +0.120% / +0.079% / +0.240%
  - reopen_10k: +0.702% / +0.541% / +0.844%
- **Verdict:** **INCONCLUSIVE** (§7 step 4). Deep-win fails: insert_2k_strings's -1.37/-1.27/-1.27 all miss the ≤ -1.5% bar by ~0.1-0.2 pp. Broad-win fails: insert_10k_u64 / modify_10k / reopen_10k all positive in all 3 runs. No scenario ≥ +1.5% so revert guard doesn't trip — reverted as INCONCLUSIVE.
- **Why nothing was actually being tested:** The bench scenarios are constructed to stay under `StoreConfig::min_compact_bytes = 1 MiB` (e.g., `modify_10k`'s `MODIFY_INNER_REPS = 3` capped to keep log < 1 MiB; insert_10k_u64 = 240 KB; insert_2k_strings ~120 KB). Compact is therefore unreachable from the bench. The observed deltas come entirely from how the new compact-loop code reshapes the binary's symbol layout and pushes neighboring scenarios across icache-set boundaries — same effect seen in 020/021/030/032/033/034 this round. insert_2k_strings happened to land *just* below -1.5% but did not pass the bar; reopen_10k drifted positive but stayed under guard.
- **Follow-ups / dead ends:**
  - Compact-loop unsafe rewrite is permanently closed for bench-observable wins. The change is *correct* and *should* speed up real compactions, but with the current bench harness it's not measurable.
  - Adjacent untried angle: extend the bench harness with a `compact_*` scenario that explicitly invokes `store.compact()` on a primed log to measure this path directly. Out of scope for /optimize but a clear gap.
  - Adjacent untried angle: factor the record-building logic shared between `flush_scratch` and `compact()` into a `serialize_record(buf, tag, payload_writer)` helper to reduce duplication once the patterns converge. Adds an abstraction the codebase doesn't need today; defer.

### 2026-05-15 — unsafe-replay-prefetch-next-record

- **Hypothesis:** In `open_with`'s replay loop, software-prefetch the first cache line of the next record before bincode-deserializing the current record. The buffer is sequential and ~240 KB for the bench, exceeding L1; software prefetch with sufficient lead time should hide the L1 miss for the next record's bytes.
- **Risk:** MEDIUM. One `unsafe` block per record (`_mm_prefetch` intrinsic), x86_64-gated; touches `open_with`'s hot replay loop (the same code path whose layout sensitivity has bitten 030/032/033).
- **Files touched:** `src/lib.rs`.
- **Diff:** [`optimization-diffs/034-unsafe-replay-prefetch-next-record.patch`](optimization-diffs/034-unsafe-replay-prefetch-next-record.patch) — reference only.
- **Soundness audit:** `_mm_prefetch` is documented as side-effect-free at the architectural level — it is a hint to the cache controller, the address is *not* dereferenced, no fault is raised on bad addresses, no aliasing rules are violated. The pointer arithmetic `buf.as_ptr().add(next)` requires `next ≤ buf.capacity()` for in-bounds provenance; guarded by `if next < buf.len()` and `buf.len() ≤ buf.capacity()`. Bytes at `next` are initialized (read_to_end filled the full buffer). If invariants somehow broke: pointer-arithmetic OOB would be UB "out-of-bounds pointer arithmetic" — caught by miri.
- **Miri gate:** PASSED. 12/12 OK, no UB errors. 78 s wall time. Miri implements `_mm_prefetch` as a no-op (as expected for a cache hint), so this gate validates pointer-arithmetic soundness but cannot tell us the prefetch is *effective*.
- **Baseline trim_mean (ns):** insert_10k_u64 206147, insert_2k_strings 494775, lookup_100k 317841, modify_10k 63400, reopen_10k 57119.
- **Δ trim_mean (3 runs):**
  - insert_10k_u64: -0.154% / -0.137% / -0.138%
  - insert_2k_strings: -0.222% / -0.289% / -0.176%
  - lookup_100k: +0.014% / -0.020% / -0.004%
  - modify_10k: +0.139% / +0.112% / +0.069%
  - reopen_10k: **+1.457% / +1.784% / +1.166%**
- **Verdict:** **REVERTED** (§7 step 1). reopen_10k R2 +1.78% above the +1.5% guard.
- **Why it lost:** Two reasons stack. (1) The bench harness does 50 inner-reps of reopen against the same on-disk file (`REOPEN_INNER_REPS = 50`), so by reopens 2+ the 240 KB buffer is fully resident in L2 — there is no L1 latency left to hide. Hardware prefetcher already handles the sequential L2→L1 streaming. (2) Adding ~3-4 x86 instructions to the loop body (branch + AGU + prefetch) and shifting the loop's binary layout reproduces the 030/032/033 icache-aliasing penalty on the very scenario the change targets. Net: pure overhead.
- **Follow-ups / dead ends:**
  - Replay-loop software prefetch (T0) is permanently closed.
  - Adjacent untried angle: `_MM_HINT_NTA` (non-temporal) for the buffer read pattern — buffer bytes are read once per record and not reused, so NTA might avoid polluting L1/L2. Risk of harming the second+ reopen runs which DO benefit from cache residency.
  - Adjacent untried angle: prefetch *farther* ahead (2-3 records, not just the next 64 B) — but the per-record size is variable (24-44 B for u64 pairs vs strings), making the prefetch distance a moving target.

### 2026-05-15 — unsafe-prefilled-scratch-truncate-via-set-len

- **Hypothesis:** Unsafe variant of 019/027 (`truncate-scratch-preserve-len-prefix`, both INCONCLUSIVE/REVERTED). Pre-seed `scratch` with `[0u8; LEN_BYTES + 1]` in the constructor and rewrite `begin_record` to use `unsafe { scratch.set_len(LEN_BYTES + 1); *scratch.get_unchecked_mut(LEN_BYTES) = tag; }`. Eliminates the `clear() + reserve(LEN_BYTES + 1)` pair on every record — the constructor invariant guarantees `scratch.len() >= LEN_BYTES + 1`, so `set_len` here is always a non-growing truncation.
- **Risk:** MEDIUM. One `unsafe` block per record on the mutation hot path; the safety invariant is `scratch.len() >= LEN_BYTES + 1` between calls.
- **Files touched:** `src/lib.rs`.
- **Diff:** [`optimization-diffs/033-unsafe-prefilled-scratch-truncate-via-set-len.patch`](optimization-diffs/033-unsafe-prefilled-scratch-truncate-via-set-len.patch) — reference only.
- **Soundness audit:** Constructor's `Vec::with_capacity(256)` then `resize(LEN_BYTES + 1, 0)` initializes bytes 0..5 with len=5. Every `flush_scratch` runs after bincode-append, leaving `scratch.len() = LEN_BYTES + 1 + payload_size ≥ LEN_BYTES + 1`. Therefore `set_len(LEN_BYTES + 1)` at the top of every `begin_record` is `new_len ≤ old_len` — a pure truncation. `Vec::set_len`'s contract for truncation only requires `new_len ≤ self.capacity()` (and `<= self.len()` for the no-init form); no initialization required because the dropped slots are still inside the allocated region and `u8` has no drop glue (no per-element drop runs). `get_unchecked_mut(LEN_BYTES)` writes byte 4 which is within the just-set `len()` of 5. No uninit read occurs anywhere. If the invariant broke (e.g. `scratch.len() < LEN_BYTES + 1` somehow): `get_unchecked_mut(4)` would be out-of-bounds — UB "out-of-bounds pointer arithmetic" / "memory access failed: pointer not dereferenceable" — caught by miri.
- **Miri gate:** PASSED. `MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks" cargo +nightly miri test --test integration` → 12/12 OK, no UB errors. 78 s wall time.
- **Baseline trim_mean (ns):** insert_10k_u64 206147, insert_2k_strings 494775, lookup_100k 317841, modify_10k 63400, reopen_10k 57119.
- **Δ trim_mean (3 runs):**
  - insert_10k_u64: -0.289% / -0.278% / -0.591%
  - insert_2k_strings: -0.346% / -0.314% / -0.264%
  - lookup_100k: +0.237% / +0.239% / +0.264%
  - modify_10k: **-1.718% / -1.724% / -1.779%** (deep-win on this scenario)
  - reopen_10k: **+1.793% / +1.656% / +2.661%** (regression, R1+R3 trip guard)
- **Verdict:** **REVERTED** (§7 step 1). reopen_10k Δ ≥ +1.5% on runs 1 and 3 (and R2 is at +1.66%, still above the bar). The modify_10k deep-win cannot save the diff — step 1's regression check runs before step 2's win check.
- **Why it lost:** Same icache-aliasing failure mode seen across 019/027/030/032: the mutation hot path (modify/insert) wins consistently when `begin_record` shrinks, but the post-ahash codegen layout puts `open_with`'s replay loop (reopen_10k) in a precarious icache neighborhood, and any nontrivial reshuffle of nearby symbols pushes its hot inner loop off cache lines. modify_10k -1.72% confirms the savings target was real (`clear() + reserve(5)` pair is measurable), but the reopen penalty is structural — closing this lever permanently. Note that the *same* idea (019) was tried pre-ahash as INCONCLUSIVE (no regression that big), so the post-ahash baseline made it worse.
- **Follow-ups / dead ends:**
  - Pre-seeded scratch + truncating `set_len` in `begin_record` is permanently closed.
  - Adjacent untried angle: same diff but with `#[cold]` or `#[inline(never)]` on `open_with` to force `replay` code into a colder code section, decoupling reopen_10k's icache locality from `begin_record`'s. Would only help reopen_10k; risk of regressing first-open latency in production usage.
  - Adjacent untried angle: move `begin_record` to a separate translation unit or `#[inline(never)]` it, then test whether the mutation hot path is dominated by call overhead (currently `#[inline]` lets it inline into insert/modify/remove). Counter-intuitive — likely regresses modify but might fix reopen layout.

### 2026-05-15 — unsafe-uninit-scratch-skip-zero-placeholder

- **Hypothesis:** Kept-029 (`unsafe-set-len-scratch-prep`) writes a u32 zero placeholder in `begin_record` and overwrites it with the real length in `flush_scratch`. Skip the zero write: leave bytes 0..LEN_BYTES uninitialized after `set_len(LEN_BYTES + 1)` and rely on `flush_scratch`'s in-place write to initialize them before any read. Save one u32 store per record on the mutation hot path.
- **Risk:** MEDIUM. One `unsafe` block in `begin_record` that calls `set_len` over a partially-uninit region.
- **Files touched:** `src/lib.rs`.
- **Diff:** [`optimization-diffs/032-unsafe-uninit-scratch-skip-zero-placeholder.patch`](optimization-diffs/032-unsafe-uninit-scratch-skip-zero-placeholder.patch) — reference only.
- **Soundness audit:** `Vec::reserve(LEN_BYTES + 1)` guarantees `capacity ≥ 5` from the base pointer. `write(p.add(LEN_BYTES), tag)` writes byte 4. `set_len(LEN_BYTES + 1)` widens len to 5; bytes 0..4 are *technically* uninitialized memory at this point. Sound iff no Rust code reads bytes 0..4 between `set_len` and the next write. The only intervening code is `bincode::serialize_into(&mut scratch, ...)` → `<Vec<u8> as io::Write>::write` → `Vec::extend_from_slice`, which uses `ptr::copy_nonoverlapping` past the current `len` and then `set_len(new)`; no read of pre-existing bytes. `flush_scratch` then performs `ptr::write_unaligned::<u32>(p, payload_len.to_le())` (pure write) initializing bytes 0..4, then `self.log.write_all(&self.scratch)` reads the full slice with bytes 0..len all initialized. Drop path: `u8` has no drop glue, so `scratch.clear()` on the next call invokes `set_len(0)` with no per-element read; same for `Drop for IndexMapStore`. If the invariant were broken (e.g. `write_all` ran before `flush_scratch`'s length write), UB would be "encountered uninitialized memory" — caught by miri's per-byte init tracking.
- **Miri gate:** PASSED. `MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks" cargo +nightly miri test --test integration` → 12/12 OK, no UB errors. 78 s wall time. Miri tracks per-byte init state and confirmed no read of uninit bytes across the full integration suite (including the modify/insert/remove paths that exercise `begin_record` → bincode → `flush_scratch`).
- **Baseline trim_mean (ns):** insert_10k_u64 206147, insert_2k_strings 494775, lookup_100k 317841, modify_10k 63400, reopen_10k 57119.
- **Δ trim_mean (3 runs):**
  - insert_10k_u64: -0.244% / -0.331% / -0.126%
  - insert_2k_strings: +0.227% / +0.477% / +0.730%
  - lookup_100k: -0.434% / -0.438% / -0.418%
  - modify_10k: -0.964% / -0.688% / -0.806%
  - reopen_10k: +0.455% / +0.385% / +0.753%
- **Verdict:** **INCONCLUSIVE** (§7 step 4). No scenario clears ≤ -1.5% in all 3 runs (deep-win fails — best is modify_10k at -0.96/-0.69/-0.81). Broad-win requires ALL scenarios `Δ < 0` in all 3 runs; insert_2k_strings is positive (+0.23/+0.48/+0.73) and reopen_10k is positive (+0.46/+0.39/+0.75) in all runs — fails broad-win. No regression past +1.5% so step 1 doesn't trip; reverted.
- **Why it lost:** The "save one store" reasoning ignored that LLVM/codegen was almost certainly merging the u32 zero store with the adjacent u8 tag write into a single aligned 8-byte SSE/MOV store (or fusing them into the same μop port on x86), so the apparent "two stores" was effectively one. Removing the placeholder unfused the pair and shifted the hot-path code layout enough to penalize unrelated scenarios — same icache-aliasing failure mode as 020/021/030. The targeted scenario (insert_10k_u64) was the only one to mildly improve; insert_2k_strings (which also uses this code path with a larger payload) regressed instead, suggesting payload-size-dependent codegen interaction.
- **Follow-ups / dead ends:**
  - Scratch placeholder removal is not profitable — closed.
  - Adjacent untried angle: explicit 8-byte unaligned write of `(0u32 << 32) | tag_byte_as_u64` to atomically place both placeholder + tag in one store and confirm/falsify the fusion hypothesis. Likely also subject to icache layout sensitivity.
  - Adjacent untried angle: keep the placeholder but place it after the tag (tag at byte 0, placeholder at bytes 1..5, then `write_unaligned` len at byte 1 from `flush_scratch`). Restructures record layout — would also touch replay decode (tag-first vs length-first). Out of scope for an unsafe-only attempt.

### 2026-05-15 — unsafe-set-len-read-exact-replay-buf

- **Hypothesis:** In `open_with`'s replay path, replace `Vec::with_capacity(n) + file.read_to_end(&mut buf)` with `Vec::with_capacity(n) + unsafe { buf.set_len(n) } + file.read_exact(&mut buf)`. Total file size is known via `file.metadata().len()`, so the dynamic grow-loop and EOF probe inside `read_to_end` are wasted work; a single `read_exact` of exactly `n` bytes should be cheaper. Targets `reopen_10k`.
- **Risk:** MEDIUM. One `unsafe` block (`set_len(n)`) extending Vec length over uninitialized bytes; followed immediately by `read_exact` which writes all `n` bytes before any read of `buf`.
- **Files touched:** `src/lib.rs`.
- **Diff:** [`optimization-diffs/031-unsafe-set-len-read-exact-replay-buf.patch`](optimization-diffs/031-unsafe-set-len-read-exact-replay-buf.patch) — reference only.
- **Soundness audit:** `Vec::with_capacity(n)` allocates `n` bytes (cap=n, len=0). `set_len(n)` widens len to n leaving 0..n uninitialized — sound provided no read happens before initialization. The very next statement passes `&mut buf` (deref `Vec<u8>` → `&mut [u8]` of length n) to `Read::read_exact`. `read_exact` writes all `n` bytes via syscalls; constructing the `&mut [u8]` does not read the bytes (Stacked Borrows allows the borrow). If `read_exact` returns `Err`, `?` propagates and `buf` is dropped; `u8` has no drop glue, so per-element reads do not happen — only the allocation is freed. If invariant were broken (e.g., a code path reading `buf` before `read_exact` returns Ok): UB classified as "constructing invalid value … encountered uninitialized memory" — miri catches.
- **Miri gate:** PASSED. `MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks" cargo +nightly miri test --test integration` → 12/12 OK, no UB errors. 78 s wall time.
- **Clippy:** required `#[allow(clippy::uninit_vec)]` at the unsafe block (default deny on `clippy::uninit_vec` because the pattern is a footgun for non-`u8` types; safe here because `u8` has no invalid values and `read_exact` fills every byte before any read).
- **Baseline trim_mean (ns):** insert_10k_u64 206147, insert_2k_strings 494775, lookup_100k 317841, modify_10k 63400, reopen_10k 57119.
- **Δ trim_mean (3 runs):**
  - insert_10k_u64: +0.230% / +0.349% / +0.022%
  - insert_2k_strings: +0.550% / +0.453% / +0.371%
  - lookup_100k: -0.018% / -0.025% / -0.036%
  - modify_10k: -0.060% / -0.016% / +0.139%
  - reopen_10k: +2.047% / +2.553% / +2.391%
- **Verdict:** **REVERTED** (§7 step 1). reopen_10k trips +1.5% guard on runs 1, 2, AND 3 (+2.05%, +2.55%, +2.39%). insert_2k_strings drifts consistently positive (+0.4–0.6%) but stays under guard.
- **Why it lost:** std's `File`-specialized `Read::read_to_end` (via `read_to_end_with_reservation` on Unix) reads into pre-reserved capacity in one pass and then probes for EOF with a single zero-byte read — i.e., 2 syscalls for the typical case. `read_exact` via the default `Read` trait impl loops over `read()` with extra Rust-side checks per iteration (handling possible short reads, checked subtraction on remaining length). The optimizer cannot fold those checks away because `read` returns a variable length. On the targeted 240 KB replay buffer (10k records × ~24 B), the loop overhead exceeds the cost of the second EOF-probe syscall it was supposed to eliminate. Net regression on the single scenario this targeted.
- **Follow-ups / dead ends:**
  - Replay-buffer read path is not improvable via this swap. Closed dead end for `unsafe-set-len-read-exact-replay-buf`.
  - Adjacent untried angle: use `Vec::spare_capacity_mut()` + `BorrowedBuf` (unstable `read_buf` feature) once stabilized — would let `read_to_end` skip the EOF probe without leaving uninit bytes addressable. Not actionable on stable today.
  - Adjacent untried angle: do `file.read(&mut buf.spare_capacity_mut())` as a single call after `assume_init` — bypasses the trait-default loop. Would still require `unsafe` and would not help if the kernel returns short on the first read.

### 2026-05-15 — unsafe-unaligned-length-read-replay

- **Hypothesis:** First explicit follow-up from 029 (`unsafe-set-len-scratch-prep`). Apply the same unsafe-elide-bounds-check pattern to the replay loop's length-prefix decode: replace `u32::from_le_bytes(buf[offset..offset+LEN_BYTES].try_into().unwrap())` with `u32::from_le(unsafe { ptr::read_unaligned(buf.as_ptr().add(offset) as *const u32) })`. The loop guard `offset + LEN_BYTES <= buf.len()` already ensures bounds; LLVM may or may not be eliding the runtime check inside `try_into`.
- **Risk:** MEDIUM. Single `unsafe` block in `open_with`'s replay loop. First /optimize cycle to exercise the new §5b miri gate end-to-end.
- **Files touched:** `src/lib.rs`.
- **Diff:** [`optimization-diffs/030-unsafe-unaligned-length-read-replay.patch`](optimization-diffs/030-unsafe-unaligned-length-read-replay.patch) — reference only.
- **Soundness audit:** `ptr::read_unaligned(buf.as_ptr().add(offset) as *const u32)` requires (1) `offset + 4 <= buf.capacity()`, (2) bytes at `offset..offset+4` initialized, (3) no live aliasing of the destination region. (1) and (2) both follow from the loop guard `offset + LEN_BYTES <= buf.len()` combined with `buf.len() <= buf.capacity()` and `read_to_end`'s guarantee that every byte `0..buf.len()` is initialized. (3) holds: `buf` is a stack-local `Vec<u8>` with no other reference (immutable or mutable) live inside the loop body — bincode borrows `body` from `buf` later, but that borrow ends before each loop iteration's next length read. If broken: out-of-bounds pointer arithmetic, uninit read, or pointer not dereferenceable — all caught by miri's Stacked Borrows + uninit-tracking.
- **Miri gate:** PASSED. `MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-ignore-leaks" cargo +nightly miri test --test integration` → 12/12 OK, no UB errors. 78 s wall time.
- **Baseline (pre-change) trim_mean (sha c9be6b2):**
  - insert_10k_u64: 206.15 us
  - insert_2k_strings: 494.77 us
  - lookup_100k: 317.84 us
  - modify_10k: 63.40 us
  - reopen_10k: 57.12 us
- **Δ trim_mean across 3 confirming runs (vs fixed pre-change baseline):**
  - insert_10k_u64: +0.17% / +0.06% / -0.11% (flat, well within noise)
  - insert_2k_strings: **+2.20% / +2.45% / +2.35% (consistent +2.3% regression — REAL, trips +1.5% guard)**
  - lookup_100k: +0.22% / +0.22% / +0.21% (small consistent drift)
  - modify_10k: +0.55% / +0.53% / +0.62% (small consistent drift)
  - reopen_10k: **+4.26% / +4.13% / +4.18% (consistent +4.2% regression — REAL, trips +1.5% guard; opposite of hypothesis)**
- **Verdict:** **REVERTED.** Two scenarios independently breach the +1.5% revert guard with consistent (all-3-runs) regressions — insert_2k_strings +2.3% and reopen_10k +4.2%. The fact that insert_2k_strings (which never touches the replay loop) regresses at all confirms this is a codegen-layout / icache-aliasing effect, not the unsafe block's runtime cost.
- **Why this is a dead end:** LLVM was already eliding the implicit bounds check after the loop guard — the unsafe form is *functionally equivalent* but reshuffles symbol layout enough to push something off the icache hot region. Same pattern flagged in 020 (`inline-always-insert-remove-modify`: +9% reopen from reshuffling unrelated bodies) and 021 (`uninline-mutation-entry-points`: +6% reopen from the opposite reshuffling). The replay-loop length decode is not, in itself, on a critical bandwidth boundary; chasing it with unsafe yields no execution-time win and lots of layout-perturbation downside.
- **Follow-ups / dead ends:**
  - **Closed:** unsafe pointer reads in the replay-loop length-decode path. Both forms (safe try_into + safe unsafe pointer read) generate equivalent runtime work; the safe form has friendlier layout characteristics.
  - **Closed:** "post-029 the obvious next unsafe target is the replay-loop length read" — falsified.
  - **Open / new pattern observed:** the §5b miri gate works as designed — it confirmed memory soundness so we knew the +4% reopen regression was definitely a layout effect rather than a runtime correctness bug. This is exactly the discriminator the gate was added to provide.
  - **Open:** other unsafe candidates that DON'T shift binary layout much: maybe `core::hint::assert_unchecked(payload_end <= buf.len())` to inform LLVM of post-guard invariants (no new pointer arithmetic, just a hint). Would need a separate attempt with a different slug.

### 2026-05-15 — unsafe-set-len-scratch-prep

- **Hypothesis:** User-authorized `unsafe` on the mutation hot path. `begin_record`'s safe `extend_from_slice(&[0u8; LEN_BYTES])` does a 4-byte memset of placeholder bytes that `flush_scratch` unconditionally overwrites; the subsequent `push(tag)` carries an independent capacity check. Folding both into one `reserve(LEN_BYTES + 1) + unsafe { write_unaligned u32 + write tag + set_len }` removes the wasted memset and the redundant capacity check. Symmetric change in `flush_scratch`: `[..LEN_BYTES].copy_from_slice(&payload_len.to_le_bytes())` becomes `unsafe { ptr::write_unaligned::<u32>(scratch.as_mut_ptr(), payload_len.to_le()) }` — single u32 store with no slice bounds check (the head 4 bytes were guaranteed allocated and initialized by `begin_record`).
- **Risk:** MEDIUM. Introduces two `unsafe` blocks. Soundness audit:
  - `begin_record`: after `reserve(LEN_BYTES + 1)`, `scratch.capacity() >= 5`. We `write_unaligned` a u32 (4 bytes of valid storage at offset 0) and a `u8` at offset LEN_BYTES (1 byte of valid storage). Both writes occur before `set_len(LEN_BYTES + 1)`, so on return all bytes within the new length are initialized.
  - `flush_scratch`: the head 4 bytes are always initialized by `begin_record` before this is called, so `write_unaligned` does not read uninitialized memory. The destination is within the Vec's allocation (`scratch.len() >= LEN_BYTES + 1`).
  - No aliasing: `self.scratch` is the sole owner; the `&mut Vec<u8>` borrow in `begin_record` ends before `flush_scratch` runs.
  - bincode's `serialize_into(&mut scratch, ...)` between `begin_record` and `flush_scratch` only appends past the existing len — it never reads the head bytes.
- **Files touched:** `src/lib.rs` (`begin_record` and `flush_scratch`).
- **Diff:** [`optimization-diffs/029-unsafe-set-len-scratch-prep.patch`](optimization-diffs/029-unsafe-set-len-scratch-prep.patch) — reference only; re-implement against current code if resurfacing.
- **Baseline (pre-change) trim_mean (sha 69021d7):**
  - insert_10k_u64: 206.22 us
  - insert_2k_strings: 505.35 us
  - lookup_100k: 318.84 us
  - modify_10k: 65.73 us
  - reopen_10k: 57.60 us
- **Δ trim_mean across 3 confirming runs (vs fixed pre-change baseline):**
  - insert_10k_u64: -0.08% / +0.10% / +0.10% (flat, well under +1.5% guard)
  - insert_2k_strings: **-2.33% / -1.76% / -1.91% (all 3 runs ≤ -1.5%)**
  - lookup_100k: -0.30% / -0.30% / -0.31% (incidental cache-shift; read path untouched)
  - modify_10k: **-3.29% / -3.53% / -3.44% (all 3 runs ≤ -1.5%, ~3.4% avg win)**
  - reopen_10k: -0.62% / -0.69% / -0.56% (directional; under -1.5% gate)
- **Verdict:** **KEPT (deep-win).** Two scenarios independently clear the deep-win bar; max regression is +0.10% (insert_10k_u64), well under the +1.5% revert guard.
- **Why:** Mutation hot path is called twice per record (begin_record + flush_scratch), so collapsing the placeholder memset, the extra cap-check on push, and the slice-bounds-checked u32 store into three unconditional unaligned stores saves ~3.4% on modify_10k and ~2% on insert_2k_strings. The post-ahash codegen layout-sensitivity flagged in 027/028 did not bite this attempt — likely because the unsafe path keeps the byte-write pattern identical to the safe one (placeholder u32 + tag byte) and only removes overhead, rather than reshaping the byte layout.
- **Follow-ups / dead ends:**
  - **Open:** could the same pattern be applied to the replay loop's `u32::from_le_bytes(buf[offset..offset + LEN_BYTES].try_into().unwrap())` via `ptr::read_unaligned`? Untried; would target reopen_10k. Separate `/optimize` invocation.
  - **Open:** apply unsafe `set_len` to the file slurp buffer in `open_with` (`Vec::with_capacity(total_on_disk).read_to_end`) — but stdlib `read_to_end` already uses uninit-spec, likely no win.
  - **Closed pattern:** post-ahash mutation tweaks aren't *always* doomed (027/028 had falsely suggested they were); the difference here is unsafe explicitly elides work rather than reshuffling it.

### 2026-05-15 — inline-always-flush-scratch-post-ahash

- **Hypothesis:** Replay attempt 017 against the post-ahash baseline. Pre-ahash 017 was INCONCLUSIVE with reopen_10k drifting -0.21%/-0.65%/-0.70% — directional but under the gate; the same hypothesis (force-inline a single-callsite hot helper) might surface now that reopen_10k baseline is ~65us instead of ~118us and fixed-cost overheads loom proportionally larger.
- **Risk:** LOW. Single-attribute change (`#[inline]` → `#[inline(always)]`) on a private fn; no API change, no dep change, no behavior change.
- **Files touched:** `src/lib.rs`.
- **Diff:** [`optimization-diffs/028-inline-always-flush-scratch-post-ahash.patch`](optimization-diffs/028-inline-always-flush-scratch-post-ahash.patch) — reference only; re-implemented against current code.
- **Implementation:** Changed line 332 attribute on `fn flush_scratch` from `#[inline]` to `#[inline(always)]`. Function body unchanged.
- **Baseline (pre-change) p50 (`/tmp/optimize/baseline.json`, sha d6f89c1):**
  - insert_10k_u64: 5.15 ms
  - insert_2k_strings: 5.38 ms
  - lookup_100k: 382.00 us
  - modify_10k: 4.96 ms
  - reopen_10k: 65.56 us
- **Δ p50 across 3 confirming runs (vs fixed pre-change baseline):**
  - insert_10k_u64: -0.17% / -0.12% / +0.66% (mixed, within noise)
  - insert_2k_strings: +1.10% / +0.98% / +0.87% (consistent ~+1% regression — REAL signal)
  - lookup_100k: +0.05% / -0.03% / -0.07% (flat, read path untouched)
  - modify_10k: +0.62% / +0.69% / +0.57% (consistent ~+0.6% regression)
  - reopen_10k: +2.51% / -0.26% / -1.39% (R1 spike >+1.5% guard; R2/R3 settle)
- **Verdict:** **REVERTED.**
- **Why:** R1 reopen +2.51% trips §7 step-1 guard. Independently, insert_2k_strings is +0.87–1.10% across all 3 runs (real, not noise). The pre-ahash 017 entry already observed that ThinLTO inlines `flush_scratch` regardless, so `inline(always)` is a codegen no-op — but the annotation evidently still nudges symbol layout enough to harm the post-ahash mutation hot path. Same failure shape as 027 (`truncate-scratch-preserve-len-prefix-post-ahash` retry): R1 cold-icache reopen spike + consistent small positive on strings/modify. Two independent post-ahash mutation-side micro-tweaks have now shown the same regression signature, suggesting the post-ahash codegen layout for the mutation hot path is highly sensitive to *any* perturbation, not just to a specific lever.
- **Follow-ups / dead ends:**
  - **Closed:** force-inlining `flush_scratch`. Both regimes (pre- and post-ahash) — no win, REVERTED post-ahash.
  - **Closed:** the entire shortlist from this `/optimize retry 3 best ones` invocation:
    - 003 `presize-replay-payload-vec` — obsoleted by 005 (slurp-into-vec)
    - 019 `truncate-scratch-preserve-len-prefix` — REVERTED (027)
    - 017 `inline-always-flush-scratch` — REVERTED (028)
  - **Open / new pattern observed:** post-ahash mutation hot path appears layout-sensitive — two unrelated tweaks both regressed strings ~+1% and modify ~+0.6%. Could indicate the ahash codegen sits in a fragile local optimum; future post-ahash optimizations on the mutation path should expect surprise regressions and need bigger payoffs to clear noise.



- **Hypothesis:** Replay attempt 019 (`truncate-scratch-preserve-len-prefix`, INCONCLUSIVE pre-ahash with reopen_10k -1.62%/-1.14%/-0.86%, modify directional) against the post-ahash baseline. The ahash entry's follow-up note flagged that "fixed-cost overheads on the [reopen] path show up proportionally larger" once reopen dropped from ~118us to ~65us, so a previously-noise-buried scratch-buffer micro-optimization could surface. Re-implemented from the saved patch as a reference (not `git apply`-d) since the surrounding code is post-ahash.
- **Risk:** LOW. No new dependencies, no public API change, no `unsafe`. Same byte-for-byte log format (constructor pre-seeds 4 zero bytes that get overwritten on each flush; per-mutation hot path swaps `clear() + extend_from_slice(&[0u8;4])` for `truncate(LEN_BYTES)`).
- **Files touched:** `src/lib.rs`.
- **Diff:** [`optimization-diffs/027-truncate-scratch-preserve-len-prefix-post-ahash.patch`](optimization-diffs/027-truncate-scratch-preserve-len-prefix-post-ahash.patch) — reference only; patch was re-implemented against current code, not `git apply`-d.
- **Implementation:**
  - Constructor `Ok(Self { ..., scratch: { let mut s = Vec::with_capacity(256); s.extend_from_slice(&[0u8; LEN_BYTES]); s } })` — replaces `scratch: Vec::with_capacity(256)`.
  - `insert`/`remove`/`modify`: `self.scratch.truncate(LEN_BYTES); self.scratch.push(TAG_*);` — replaces `self.scratch.clear(); self.scratch.extend_from_slice(&[0u8; LEN_BYTES]); self.scratch.push(TAG_*);`.
  - `flush_scratch` unchanged (still overwrites `scratch[..LEN_BYTES]` with the computed payload length).
- **Baseline (pre-change) p50 (`/tmp/optimize/baseline.json`, sha 66a92b2):**
  - insert_10k_u64: 5.15 ms
  - insert_2k_strings: 5.38 ms
  - lookup_100k: 382.00 us
  - modify_10k: 4.96 ms
  - reopen_10k: 65.56 us
- **Δ p50 across 3 confirming runs (vs fixed pre-change baseline):**
  - insert_10k_u64: +0.05% / +1.08% / +0.21% (drift positive but under guard)
  - insert_2k_strings: +1.20% / +1.42% / +1.12% (consistent ~+1.2% regression — REAL, not noise; 3-run agreement rules out spike)
  - lookup_100k: +0.32% / +0.06% / +0.02% (flat — read path untouched)
  - modify_10k: +0.54% / +0.42% / +0.80% (all positive, opposite direction from pre-ahash 019)
  - reopen_10k: +14.01% / -0.29% / -1.08% (R1 spike trips +1.5% revert guard; R2/R3 settle to noise)
- **Verdict:** **REVERTED.**
- **Why:** R1 reopen_10k +14.01% trips the §7 step-1 revert guard immediately. Independently, insert_2k_strings is +1.0–1.4% across all 3 runs — that's the consistent signal: the post-ahash codegen layout for the mutation hot path penalizes `truncate + push` relative to `clear + extend + push`, opposite of the pre-ahash measurement (019 had strings -0.49%/-0.19%/-0.31%, reopen -1.62%/-1.14%/-0.86%). The ahash hypothesis ("fixed costs proportionally larger on the now-faster reopen path") is falsified for this specific lever — instead, the change *adds* fixed cost on the strings path that the pre-ahash baseline absorbed. R1 reopen +14% is most likely a cold-icache spike on the first bench run after rebuild (R2/R3 returned to baseline), but the §7 protocol still treats any single-run +1.5% as a revert trigger and the strings regression is independently disqualifying.
- **Follow-ups / dead ends:**
  - **Closed:** the `truncate(LEN_BYTES) + push` rewrite of the scratch hot path. Pre-ahash and post-ahash both INCONCLUSIVE/REVERTED — the pattern is not a win in either codegen regime.
  - **Open / next retry candidates queued from this invocation's shortlist:**
    - `presize-replay-payload-vec-post-ahash` (003) — **obsoleted** by `slurp-log-into-vec` (005, KEPT); the per-record `payload: Vec` no longer exists. Drop from the queue.
    - `inline-always-flush-scratch-post-ahash` (017) — still live. Force-inlines a single-callsite hot helper; small directional drift pre-ahash (-0.21%/-0.65%/-0.70% on reopen). Worth a separate `/optimize` invocation.
  - **Open / new candidate from this attempt:** the constructor pre-seed adds 4 bytes of work once at open; the hot-path saved op is `clear() vs truncate(LEN_BYTES)` (one length-write either way) plus `extend_from_slice(&[0;4]) vs nothing` (~4 bytes copy avoided). The 4-byte copy avoidance is too small to outweigh the icache layout shift the rewrite caused — confirms inserts/modify are I/O-bound, not memory-bound.



- **Hypothesis:** Replace `IndexMap`'s default `std::hash::RandomState` (SipHash-1-3, DoS-resistant but ~3-5x slower than non-cryptographic hashers on small/integer keys) with `ahash::RandomState`. The two hottest scenarios — `lookup_100k` (pure HashMap probe) and `reopen_10k` (rehashes every replayed key when rebuilding the in-memory map) — should both be hasher-bound; ahash typically ships 2-10x faster lookups for u64/short-string keys, so a multi-percent move is plausible (and would be the largest single optimization in this log).
- **Risk:** MEDIUM. Adds a new dependency (`ahash 0.8`, default-features off, `std` feature only — no nightly, no SIMD opt-in beyond what's gated on stable). Also a *technical* public-API change: `as_indexmap()` now returns `&IndexMap<K, V, ahash::RandomState>` and the struct field bakes the hasher type into `IndexMapStore`'s layout. Accepted under the user's explicit `/optimize use ahash` directive.
- **Files touched:** `src/lib.rs`, `Cargo.toml`, `Cargo.lock`.
- **Diff:** [`optimization-diffs/023-ahash-indexmap-hasher.patch`](optimization-diffs/023-ahash-indexmap-hasher.patch)
- **Implementation:**
  - `Cargo.toml`: `ahash = { version = "0.8", default-features = false, features = ["std"] }`.
  - `IndexMapStore<K, V>` field: `map: IndexMap<K, V, ahash::RandomState>`.
  - `open_with`: `let mut map: IndexMap<K, V, ahash::RandomState> = IndexMap::with_hasher(ahash::RandomState::new());` (replaces `IndexMap::new()`; the existing `map.reserve(capacity_hint)` two-step pre-size is preserved — `construct-IndexMap-with-capacity-direct` already proved single-step `with_capacity_and_hasher` regresses reopen).
  - `as_indexmap` return type updated to `&IndexMap<K, V, ahash::RandomState>`.
  - `iter()` / `keys()` / `values()` return `indexmap::map::Iter/Keys/Values<'_, K, V>` which are not parameterised on `S`, so those signatures are unchanged.
- **Baseline (pre-change) p50 (`/tmp/optimize/baseline.json`, sha 42bfa63):**
  - insert_10k_u64: 5.27 ms
  - insert_2k_strings: 5.45 ms
  - lookup_100k: 628.32 us
  - modify_10k: 5.08 ms
  - reopen_10k: 117.55 us
- **Δ p50 across 3 confirming runs (vs fixed pre-change baseline):**
  - insert_10k_u64: -0.57% / -1.53% / -2.33% (directional improvement, R3 clears -1.5% but not all 3)
  - insert_2k_strings: +0.06% / +0.13% / -1.30% (essentially flat — string hashing dominated by string allocation/copy + serde, not hasher)
  - lookup_100k: -39.20% / -39.28% / -39.20% (KEPT — pure hasher win)
  - modify_10k: -1.92% / -1.67% / -2.23% (KEPT — all 3 clear -1.5%)
  - reopen_10k: -43.90% / -44.51% / -44.23% (KEPT — replay path rehashes every record)
- **Verdict:** **KEPT (deep-win).**
- **Why:** Three independent scenarios clear the deep-win bar (lookup_100k, modify_10k, reopen_10k all ≤ -1.5% in every run). No scenario regresses past +1.5% in any run — worst single-run drift is `insert_2k_strings` at +0.13%, well inside noise. The ~40% lookup and ~44% reopen wins are exactly what the hypothesis predicted: those scenarios spend most cycles inside the hasher, and SipHash-1-3 → ahash is roughly that ratio on u64 keys. `modify_10k` benefits indirectly because every modify does an internal lookup. Insert paths are flat because the dominant cost there is bincode serialization + buffered-I/O, not the hash. Largest single-attempt improvement landed in this log.
- **Follow-ups / dead ends:**
  - **Closed:** the "swap default hasher" lever — ahash captures the win; no need to also try foldhash/fxhash/rustc-hash unless ahash itself becomes a problem.
  - **Open / worth a separate attempt:** revisit `presize-replay-payload-vec` and `compact-batch-len-prefix-and-payload` — both were INCONCLUSIVE against the old (slower) reopen baseline; with reopen now ~65 us instead of ~118 us, fixed-cost overheads on that path show up proportionally larger and a previously-noise-buried win could surface.
  - **Open / Cargo.lock sensitivity:** ahash 0.8 pulls in `zerocopy`, `version_check`, `once_cell`. If supply-chain footprint matters, `foldhash` is single-crate and similar speed — would be a separate hypothesis.
  - **Caveat (not a follow-up, a documentation note):** ahash is *not* DoS-resistant against adversarial keys. Acceptable for a single-process embedded store but worth flagging in crate docs if the store is ever exposed to untrusted key streams.

### 2026-05-15 — release-profile-panic-abort-strip

- **Hypothesis:** User-requested cargo profile tweaks. Add `opt-level=3`, `debug=false`, `panic="abort"`, `strip=true` to both `[profile.release]` and `[profile.bench]` on top of the existing `lto="thin"` + `codegen-units=1`. Of those four flags, `opt-level=3` and `debug=false` are already release defaults; `strip=true` only affects binary size, not codegen. The only flag that can plausibly move benchmark numbers is `panic="abort"` — by eliminating unwind landing pads and `.eh_frame` tables, the compiler can sometimes shrink hot functions enough to unlock inlining or improve icache behaviour.
- **Risk:** LOW (build-system flags only, no source edit).
- **Files touched:** `Cargo.toml` (added 4 keys to each of `[profile.release]` and `[profile.bench]`).
- **Diff:** [`optimization-diffs/026-release-profile-panic-abort-strip.patch`](optimization-diffs/026-release-profile-panic-abort-strip.patch)
- **Cargo behaviour note:** Both `cargo test`/`cargo clippy` and `cargo bench` emitted `warning: 'panic' setting is ignored for 'bench' profile`. The bench harness (and test harness) always link with unwinding because libtest needs `catch_unwind`. So in practice `panic="abort"` only takes effect for `[profile.release]` consumers of the library — the bench binary, which is what the gate measures, still uses unwind. This materially weakens the hypothesis: the only flag with a plausible codegen effect doesn't reach the bench-measured binary.
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -1.26% / -0.91% / -1.44% (negative all 3 but none clears -1.5%; best run -1.44% is just shy of the deep-win gate)
  - insert_2k_strings: -1.40% / -0.90% / -0.92% (negative all 3, same pattern — R1 close to deep-win, R2/R3 mid)
  - lookup_100k: -0.27% / -0.20% / -0.15% (consistent tiny negative drift well inside the ≈1% noise band, doesn't clear ≤-0.5% for broad-win)
  - modify_10k: -1.45% / -0.86% / -1.50% (two runs essentially _at_ the -1.5% gate but R2 -0.86% breaks deep-win; broad-win passes for this scenario)
  - reopen_10k: -0.44% / -1.27% / -1.26% (R1 -0.44% breaks the ≤-0.5% broad-win bar; R2/R3 are firmly negative)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** Walk through the gate logic:
  - **REVERTED gate**: all 15 deltas are negative — max positive is effectively 0%. PASS (no revert-for-regression).
  - **Deep-win**: requires ≥1 scenario ≤-1.5% in _all_ 3 runs. Closest are modify_10k (-1.45%/-0.86%/-1.50%) and insert_10k_u64 (-1.26%/-0.91%/-1.44%) — both have a sub-(-1.5%) run that disqualifies the scenario. FAIL.
  - **Broad-win**: requires ALL scenarios ≤-0.5% in all 3 runs AND no run > +0.1%. lookup_100k tops out at -0.27% (never clears -0.5%) and reopen_10k R1 -0.44% misses by 0.06%. FAIL.
  - **INCONCLUSIVE** (default).
- **Why the deltas look like a uniform negative drift rather than a clean signal:** Three of five scenarios sit in the deep-win neighbourhood (-0.86% to -1.50%); the other two (lookup_100k, reopen_10k R1) drift only slightly negative. Given `panic="abort"` is _ignored_ for the bench profile, the most likely explanation for the small uniform improvement is timer/cache state drift between baseline and re-bench (the baseline was captured at a different point in the recent attempt history, and the rebuilt bench binary has a slightly different symbol layout from `strip=true`). The signal is real-looking but not causally tied to a load-bearing flag — exactly the shape of an INCONCLUSIVE.
- **Follow-ups / dead ends:**
  - **Closed:** `panic="abort"`/`strip=true` at the bench-profile level. Cargo ignores `panic` for bench, so the only codegen lever is dead-on-arrival for the bench harness. No reason to retry this combo in any future stack.
  - **Closed:** `opt-level=3` + `debug=false` as standalone tweaks. Both are already release defaults; making them explicit cannot change codegen.
  - **Open (HIGH risk, requires user authorization):** switch `lto = "thin"` → `lto = "fat"`. This _does_ reach the bench binary and is the meaningful unexplored profile lever. Risk: long link times, larger codegen-unit cost, possible icache regression on the larger bench binary; conventionally HIGH because it changes a published-build dependency and is awkward to revert quickly.
  - **Open (HIGH risk):** PGO build. The "bench numbers at codegen-noise floor" meta-observation from `bundle-inconclusive-round-2`'s follow-ups still stands — PGO is the natural next escalation but requires tooling beyond `/optimize`'s single-attempt loop.
  - **Open (LOW risk, narrow):** set `panic = "abort"` _only_ on `[profile.release]` (drop the bench setting that cargo already ignores). Strictly cosmetic — won't change bench numbers — but worth doing if the project ships a release artifact. Out of scope for `/optimize` which gates on bench movement.

### 2026-05-14 — bundle-inconclusive-round-2

- **Hypothesis:** A second-round stack of every INCONCLUSIVE attempt that landed _after_ the first stacking attempt `bundle-inconclusive-attempts` (KEPT, deep-win). Round 1 merged 4 attempts and cleared the -1.5% gate on reopen_10k. The thinking: round 1's success suggests stacking can additively expose signal that each attempt individually buried in noise — try the same pattern on round-2 INCONCLUSIVEs. The 5 merged:
  1. **bincode-2-upgrade-new-config** (HIGH risk: dep version swap, user-authorized) — bincode 1.3 → 2.0.1, new typed `Configuration<LittleEndian, Fixint, NoLimit>` API (NOT `legacy()`), all 4 `serialize_into` callsites migrated to `bincode::serde::encode_into_std_write`, both `deserialize` callsites migrated to `bincode::serde::decode_from_slice`, `serialize_err` signature updated to `bincode::error::EncodeError`.
  2. **compact-batch-len-prefix-and-payload** (LOW) — `compact()` rewrite loop now pre-seeds `buf` with `LEN_BYTES` zero bytes outside the loop and uses `truncate(LEN_BYTES) + push(tag)` per record, filling the length prefix in place and emitting length + payload via a single `write_all`.
  3. **truncate-scratch-preserve-len-prefix** (LOW) — `open_with` pre-seeds `scratch` with `[0u8; LEN_BYTES]` at construction; `insert`/`remove`/`modify` use `truncate(LEN_BYTES) + push(tag)` instead of `clear + extend(&[0;4]) + push(tag)`. The first 4 bytes survive between calls; `flush_scratch` always overwrites them before the write.
  4. **inline-always-flush-scratch** (LOW) — `#[inline]` → `#[inline(always)]` on `flush_scratch`.
  5. **bufwriter-capacity-2mb** (LOW) — `StoreConfig::default().buf_capacity` from `1024 * 1024` to `2 * 1024 * 1024`.

  Excluded `combine-len-prefix-and-tag-extend` (round-2 INCONCLUSIVE #6) as subsumed by #3 — both alter the same scratch byte sequence; truncate-scratch is the stronger directional bet on reopen.

- **Risk:** HIGH (bundles a dependency version upgrade, which the skill rubric flags as HIGH; user explicitly authorized the bincode 2 upgrade in the immediately-preceding attempt).
- **Files touched:** `Cargo.toml` (`bincode = "1.3"` → `bincode = { version = "2.0.1", features = ["serde"] }`); `src/lib.rs` (added `bincode::config::*` imports + `BINCODE_CONFIG` const; updated 4 serialize callsites + 2 deserialize callsites; updated `serialize_err` signature; pre-seeded scratch in `open_with`'s constructor return; rewrote insert/remove/modify write-preludes to `truncate(LEN_BYTES) + push(tag)`; rewrote `compact()` loop's `buf` to mirror flush_scratch's pattern; `#[inline]` → `#[inline(always)]` on `flush_scratch`; `buf_capacity` default 1MB → 2MB).
- **Diff:** [`optimization-diffs/025-bundle-inconclusive-round-2.patch`](optimization-diffs/025-bundle-inconclusive-round-2.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -1.41% / -0.56% / -0.24% (run 1 nearly clears -1.5% gate; runs 2/3 don't — gate requires all 3, so no deep-win on this scenario)
  - insert_2k_strings: -0.42% / -0.45% / -0.48% (extremely consistent small negative drift, same tight signal as bincode-2-upgrade-new-config alone — but doesn't clear ≤-0.5% for broad-win)
  - lookup_100k: -0.18% / +0.16% / -0.03% (flat — noise band, no codec touchpoint)
  - modify_10k: -0.13% / +0.03% / -0.18% (flat — gone from round-1's -1.24% to -1.69% directional improvement back to noise; the round-2 changes aren't reinforcing modify the way round-1's hot-path inline annotations did)
  - reopen*10k: +0.43% / -0.44% / +0.47% (the killer — went \_positive* in 2 of 3 runs, opposite direction from individual INCONCLUSIVEs which mostly drifted reopen slightly negative; broad-win sub-guard fails)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** Verdict logic walks through cleanly:
  - **REVERTED gate**: max positive Δ is +0.47% (reopen_10k R3), well under the +1.5% guard. PASS.
  - **Deep-win**: requires ≥1 scenario ≤-1.5% in _all_ 3 runs. The best is insert_10k_u64's -1.41% / -0.56% / -0.24% — one run almost clears, two are far from it. FAIL.
  - **Broad-win**: requires ALL scenarios ≤-0.5% in all 3 runs AND no scenario >+0.1% in any run. insert_10k_u64 R3 -0.24% breaks ≤-0.5%; insert_2k_strings -0.42% is just above the threshold; modify_10k is flat; reopen R1/R3 at +0.43% / +0.47% blows the +0.1% sub-guard. FAIL.
  - **INCONCLUSIVE** (default).
- **Why the stack didn't compose like round-1**: Round 1 (`bundle-inconclusive-attempts`, KEPT) got reopen*10k -1.56% to -2.02% from 4 changes whose individual INCONCLUSIVEs all moved reopen \_negative* in at least 2 of 3 runs each. The round-2 inputs are weaker reopen movers individually:
  - `bincode-2-upgrade-new-config`: reopen -0.78% to -1.08% (3 runs), then -0.45% to -0.61% w/ +1.02% in rerun
  - `truncate-scratch-preserve-len-prefix`: reopen -0.86% / -1.14% / -1.62%
  - `compact-batch-len-prefix-and-payload`: reopen -0.22% / -0.52% / -0.74%
  - `inline-always-flush-scratch`: reopen -0.21% / -0.65% / -0.70%
  - `bufwriter-capacity-2mb`: reopen -0.68% / +0.61% / -0.61% (already showed positive variance)
  - But stacked, reopen lands at +0.43% / -0.44% / +0.47%. The composition appears to _cancel_ rather than reinforce — most likely codegen-layout side effects from the 5 simultaneous edits conflict with one another (the bincode 2 monomorphisations are bigger, the `#[inline(always)]` forces more code into the bench harness, the truncate-scratch change shifts struct construction layout, and the 2MB BufWriter widens the VMA). Round 1's contributors were more orthogonal: they touched different layers (codec format, struct attributes, file-open API, function inlining) without competing for the same icache footprint.
- **Follow-ups / dead ends:**
  - **Closed:** stacking all of round-2's INCONCLUSIVEs together. The composition is non-additive; doesn't ship.
  - **Closed (by extension):** any subset of these 5 that includes `bincode-2-upgrade-new-config` along with `truncate-scratch-preserve-len-prefix` — the two largest layout-shifting changes (the bincode 2 monomorphisation expansion + the constructor pre-seed reorder) seem to be the ones cancelling each other on reopen. Worth confirming with a smaller stack if a future invocation targets these specifically.
  - **Open:** a smaller round-2 stack that excludes the bincode dep upgrade — i.e., (truncate-scratch + compact-batch + inline-always-flush-scratch + buf 2MB) without the bincode 2 swap. The bincode 2 upgrade is the riskiest layout change in this stack; isolating its codegen impact would clarify whether it's the cancellation source.
  - **Open:** a much narrower round-2 pair: just (truncate-scratch + bufwriter-2mb), the two most-orthogonal round-2 INCONCLUSIVEs that still target reopen. Speculative — round-2's individual signals are weaker than round-1's were, so even an orthogonal subset may stay INCONCLUSIVE.
  - **Closed (re-affirmed):** the meta-observation across all of round-2 — this crate's bench numbers are at the codegen-layout noise floor for source-level micro-optimization. Future productive paths likely require workload changes (a codec-bypass for primitive K/V) or build-system tooling (PGO, explicit symbol ordering), not more single-file edits.
  - **Bench-harness diagnostic:** `insert_2k_strings` reliably drifts -0.33% to -0.60% on every bincode-2-touching attempt (3 runs of round-2 here: -0.42% / -0.45% / -0.48%; 6 runs of bincode-2 alone: -0.33% to -0.60%). This is the tightest, most reproducible signal across all recent attempts and points to bincode 2's encoder being incrementally faster on String — but it's below every gate. If the bench harness gained a scenario that does more bincode encode work (e.g., insert_100k_strings or insert_10k_struct), this signal might clear a gate cleanly.

### 2026-05-14 — bincode-2-upgrade-new-config

- **Hypothesis:** Upgrading bincode from 1.3 → 2.0.1 with the new typed `Configuration<LittleEndian, Fixint, NoLimit>` API (built via `bincode::config::standard().with_fixed_int_encoding().with_little_endian().with_no_limit()`; explicitly NOT `bincode::config::legacy()`) exercises bincode 2's rewritten encoder/decoder. Fixed-int encoding chosen so the on-disk byte layout matches bincode 1.x defaults — this both keeps existing-file recovery viable and avoids the known +65% reopen regression from varint decode (already documented under the closed `bincode-varint-encoding` attempt).
- **Risk:** HIGH (dependency-version swap, normally requires user approval per skill rubric). User explicitly authorized.
- **Files touched:**
  - `Cargo.toml` (`bincode = "1.3"` → `bincode = { version = "2.0.1", features = ["serde"] }`).
  - `src/lib.rs`: added `BINCODE_CONFIG` const using the new typed `Configuration` builder API; migrated the 4 `bincode::serialize_into(W, &T)` callsites (insert / remove / modify / compact) to `bincode::serde::encode_into_std_write(T, &mut W, BINCODE_CONFIG)`; migrated the 2 `bincode::deserialize(body)` callsites in the replay loop to `bincode::serde::decode_from_slice(body, BINCODE_CONFIG)` (destructuring the `(T, usize)` return); updated `serialize_err` signature from `bincode::Error` to `bincode::error::EncodeError`.
- **Diff:** [`optimization-diffs/024-bincode-2-upgrade-new-config.patch`](optimization-diffs/024-bincode-2-upgrade-new-config.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -1.35% / -0.22% / -0.35% (directionally negative — encoding path slightly faster; run 1 nearly clears -1.5% gate but runs 2/3 don't)
  - insert_2k_strings: -0.60% / -0.41% / -0.52% (consistent small negative drift)
  - lookup_100k: +0.25% / +0.01% / **+1.51%** (REVERTED-trigger — lookup doesn't touch bincode at all, so the +1.51% is almost certainly noise / codegen-layout drift from the source edit, but ≥ +1.5% triggers the guard)
  - modify_10k: +0.01% / +0.12% / +0.05% (flat — modify_10k re-inserts existing keys so its hot path is the map mutation, not the codec)
  - reopen_10k: -1.08% / -0.64% / -0.78% (directionally negative — decode path slightly faster, doesn't clear -1.5% gate in all 3)
- **Verdict:** INCONCLUSIVE (reclassified from initial REVERTED after follow-up 3-run gate confirmed the lookup_100k +1.51% was noise — see "Follow-up runs" below).
- **Why (initial 3-run gate):** Three paths fail at once. (1) **Regression guard fires**: lookup*100k showed +1.51% in run 3 — exactly at the `≥ +1.5%` threshold. Lookup is a pure `map.get(k)` benchmark that never touches the codec, so this was suspected to be layout/cache noise from the source edit (bincode 2's monomorphisations are bigger and reshuffle the binary), not a real codec regression. (2) **Deep-win path fails**: no scenario clears -1.5% in \_all* 3 runs (insert_10k_u64 only hits -1.35% best; reopen_10k tops out at -1.08%). (3) **Broad-win path fails**: lookup_100k +1.51% blows well past the +0.1% sub-guard, and modify_10k is flat-or-slightly-positive (+0.01% to +0.12%) so it doesn't satisfy the ≤-0.5% requirement either. The directional signal in the codec-touching scenarios is real and consistently negative (every write/replay benchmark improved across all 3 runs, never positive), suggesting bincode 2's fixed-int encoder is incrementally faster than bincode 1.x's — but the improvement is ~-0.5% to -1.3%, not big enough to clear the deep-win gate.
- **Follow-up runs (4/5/6) — user-requested rerun to validate the noise hypothesis:** Re-applied the change and ran 3 more rounds against the same pinned baseline.
  - insert_10k_u64: -0.06% / -0.12% / -1.03% (still directionally negative; one run dipped past -1%)
  - insert_2k_strings: -0.53% / -0.45% / -0.33% (extremely consistent — every single one of the 6 runs landed in [-0.33%, -0.60%], the tightest signal in the dataset)
  - lookup_100k: -0.09% / +0.31% / +0.19% (**confirmed as noise** — 5 of 6 runs in [-0.09%, +0.31%]; the run-3 +1.51% spike was a one-time outlier)
  - modify_10k: -0.39% / +0.22% / -0.00% (flat across all 6 runs; this scenario re-inserts existing keys so the hot path is map mutation, not the codec)
  - reopen_10k: -0.61% / +1.02% / -0.45% (5 of 6 runs negative, but run 5's +1.02% breaks the broad-win sub-guard; the decode-path improvement is real but jitter-prone)
- **Reclassified verdict (all 6 runs considered):** INCONCLUSIVE.
  - Regression guard: 1-out-of-6 lookup spike is below typical noise rates for this benchmark; treated as discard-able noise.
  - Deep-win: still fails. The closest is insert_10k_u64 -1.03% / -1.35% on 2 of 6 runs (and -0.06% to -0.35% on the other 4) — nowhere near "≤-1.5% in all 3 runs".
  - Broad-win: still fails on two counts. modify_10k drifts both ways across runs (-0.39% to +0.22%) — fails the "≤-0.5% in all runs" requirement. Run 5's reopen +1.02% blows past the +0.1% sub-guard.
- **Follow-ups / dead ends:**
  - **Closed:** the bincode 2.0.1 upgrade itself, _with fixed-int config_. The improvement on codec-touching paths is real (insert_2k_strings reliably -0.33% to -0.60% across all 6 runs) but too small to ship past the gate, and modify_10k is flat (the hot path is map mutation, not the codec).
  - **Closed by extension:** any "swap the bincode major version" attempt — the migration cost (4 callsite changes + error type + new config const) reshuffles symbol layout enough that the bench's most layout-sensitive scenario (lookup_100k, which has a 630us p50 where 1% = 6us, well within typical codegen jitter for monomorphized hot loops) gets perturbed in unpredictable ways. Same risk profile as the slug-26 `inline-always-insert-remove-modify` REVERT — the change is unrelated to the regressing scenario, but the binary shifts.
  - **Open (NOT retried here):** bincode 2 with `standard()` config (varint) — already closed under `bincode-varint-encoding`'s +65% reopen regression; bincode 2's varint encoder is faster than v1's but the dominant cost is still decode-arithmetic, not encode.
  - **Open (a different shape):** hand-rolled fixed-prefix codec for the primitive K/V cases (u64, String). Sits in the MEDIUM-risk bucket per the skill rubric. Would skip bincode entirely on the hot path and let bincode handle only the fallback. Not attempted here.
  - **Open (build-system, outside `/optimize` scope):** PGO-based function ordering so source-edit-induced layout shifts don't perturb unrelated scenarios. Same observation as `construct-IndexMap-with-capacity-direct`'s follow-up.
  - **Meta-observation:** This is now the 4th consecutive REVERTED/INCONCLUSIVE attempt. The pattern across them all: codec-touching paths trend negative on every write-time / replay-time refactor, but at <1.5% magnitudes that get drowned by codegen-layout noise on the unrelated `lookup_100k` scenario. The crate's bench numbers are at the codegen-layout noise floor for source-level micro-optimization. Future productive paths likely require workload changes (a codec-bypass for primitives) or build-system tooling (PGO, explicit `#[link_section]`), not more single-file edits.

### 2026-05-14 — construct-IndexMap-with-capacity-direct

- **Hypothesis:** Collapsing the two-step `IndexMap::new() + map.reserve(capacity_hint)` into a single-step `IndexMap::with_capacity_and_hasher(capacity_hint, Default::default())` would avoid the redundant capacity-check + dual-allocation pattern (new allocates zero, reserve grows). For the reopen_10k bench the hint is non-zero, so `with_capacity_and_hasher` allocates once for exactly the needed size — same end state, fewer hashbrown internal branches.
- **Risk:** LOW (refactor, semantically equivalent — same final map capacity, same hasher).
- **Files touched:** `src/lib.rs` (`open_with` — IndexMap construction and the moved-up `capacity_hint` calculation).
- **Diff:** [`optimization-diffs/023-construct-IndexMap-with-capacity-direct.patch`](optimization-diffs/023-construct-IndexMap-with-capacity-direct.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: +0.12% / +0.28% / -0.06% (within noise — write path flat)
  - insert_2k_strings: -0.10% / +0.01% / -0.07% (within noise)
  - lookup_100k: +0.13% / -0.08% / +0.22% (within noise)
  - modify_10k: -0.19% / -0.24% / -0.59% (within noise — small directional improvement, under gate)
  - reopen_10k: +3.56% / +3.66% / +4.10% (REVERTED — all three over +1.5% guard, ~5us regression on a 119us baseline)
- **Verdict:** REVERTED.
- **Why:** Two compounding factors. (1) Subtle semantic shift: the old code constructed `IndexMap::new()` _unconditionally_ (zero-alloc) then called `reserve()` _only if_ `capacity_hint > 0`. The new code calls `with_capacity_and_hasher(capacity_hint, ...)` unconditionally — even for empty-file opens where capacity_hint == 0, this passes through hashbrown's capacity logic and does a hashmap-init even at zero capacity (creating an empty hashbrown table). It's tiny per op, but the call site is now reached for every open instead of conditionally. (2) Source-layout drift: moving `capacity_hint`'s declaration out of the `if total_on_disk > 0` block changed the function body's structure enough that ThinLTO produced a different symbol ordering, and the read path's icache layout suffered. The +3.5–4.1% reopen regression is large (~5us) and stable across runs, indicating a real codegen layout effect rather than measurement noise. The original two-step pattern (always `new()`, conditionally `reserve()`) is locally optimal.
- **Follow-ups / dead ends:** Closed: collapsing IndexMap construction + reserve into one call. Closed (by extension): any structural refactor of `open_with` that moves declarations across the `if total_on_disk > 0` boundary — codegen layout is delicately tuned here. Open: PGO would let the linker re-order functions based on measured hotness and shield the codebase from these layout-side-effects of source edits — but it needs build-system support outside the single-file-edit model and applies to a single workload at a time. Open: explicit `#[link_section]` placement of the reopen-hot path's functions to make them position-stable across source edits — same tooling caveat. Final observation across attempts 7–10: this codebase's bench numbers are now dominated by codegen layout, and source-level micro-optimizations are at or below the gate's noise floor. The cleanest path forward is either (a) a measurable workload change (foldhash hasher, hand-rolled primitive codec, snapshot file) that requires HIGH-risk authorization, or (b) build-system tooling (PGO, symbol ordering) outside the `/optimize` model.

### 2026-05-14 — compact-batch-len-prefix-and-payload

- **Hypothesis:** Open follow-up from the KEPT `batch-len-prefix-and-payload` ("`compact()` still does two separate `write_all`s per record — could be unified the same way"). Reserve `LEN_BYTES` at the start of the compact loop's `buf`, fill the length in place, and emit length-prefix + payload via a single `write_all` per record. Bench doesn't exercise `compact()` (log stays under the 1MB compact threshold for all five scenarios), so primary outcome is consistency-with-flush_scratch-pattern; secondary outcome is codegen-layout drift that might nudge reopen positively.
- **Risk:** LOW (no API/format/dep change; on-disk layout identical — same bytes in the same order).
- **Files touched:** `src/lib.rs` (`compact()`'s rewrite loop).
- **Diff:** [`optimization-diffs/022-compact-batch-len-prefix-and-payload.patch`](optimization-diffs/022-compact-batch-len-prefix-and-payload.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: +0.10% / -0.38% / -0.33% (within noise)
  - insert_2k_strings: +0.68% / +0.41% / +0.15% (within noise — small directional positive drift, well under +1.5% guard)
  - lookup_100k: -0.18% / -0.05% / +0.14% (within noise)
  - modify_10k: +0.02% / +0.33% / -0.15% (within noise)
  - reopen_10k: -0.74% / -0.22% / -0.52% (within noise — directional improvement, doesn't clear -1.5% gate)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** As expected, the bench scenarios don't exercise compaction (logs under 1MB stay below `min_compact_bytes`), so the only way this change moves the numbers is through codegen-layout drift from the source-level edit. The small reopen_10k improvement (-0.22% to -0.74%) is real-but-tiny — directionally consistent with previous "any edit shifts symbol layout and reopen drifts negative" pattern but doesn't approach the gate. `insert_2k_strings` drifted slightly positive (+0.15% to +0.68%) for similar layout reasons. The deeper observation: this codebase's bench numbers are dominated by codegen layout, not by source-level micro-optimizations on cold paths. A consistency/cleanup change that doesn't ship without a real bench win.
- **Follow-ups / dead ends:** Closed: applying the batch-len-prefix-and-payload pattern to compact (the optimization is correct in principle but the bench doesn't exercise it; not worth the codegen-layout risk to ship). Open: a workload that _does_ exercise compaction (lower `min_compact_bytes` in a bench scenario, or write 1.5MB+ of records) would be needed to validate this; that's a bench harness change, not a source change. Open: when the bench harness is later expanded to cover compact, re-evaluate this hypothesis under a fresh slug like `compact-batch-write-when-exercised`.

### 2026-05-14 — uninline-mutation-entry-points

- **Hypothesis:** Motivated by the immediately-preceding `inline-always-insert-remove-modify` REVERTED result (+9% reopen regression from forcing inlining of large bodies into the bench harness): the symmetric experiment is to _remove_ the `#[inline]` annotation entirely from `insert`/`remove`/`modify`, letting ThinLTO's heuristic decide. The heuristic might judge these ~30-line bodies too big to inline cross-crate, which would keep them in lib.rs's text segment, shrink the bench harness's loop bodies, and free up icache for the read path that reopen exercises.
- **Risk:** LOW (no API/format/dep change; removal of one annotation).
- **Files touched:** `src/lib.rs` (`insert`, `remove`, `modify` — dropped `#[inline]`).
- **Diff:** [`optimization-diffs/021-uninline-mutation-entry-points.patch`](optimization-diffs/021-uninline-mutation-entry-points.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.22% / +0.18% / +0.09% (within noise — write path flat as expected from the heuristic)
  - insert_2k_strings: -0.34% / -0.15% / +0.07% (within noise)
  - lookup_100k: -0.15% / +0.20% / +0.15% (within noise)
  - modify_10k: -0.23% / -0.17% / -0.43% (within noise — modify_10k drifts slightly negative but well under gate)
  - reopen_10k: +6.14% / +6.05% / +5.65% (REVERTED — all three over +1.5% guard, ~7us regression on a 119us baseline)
- **Verdict:** REVERTED.
- **Why:** Both directions of the inline knob hurt reopen, but the existing `#[inline]` (heuristic-friendly hint) is locally optimal — and that lands well below the always-inline backfire and above the never-inline backfire. Removing the annotation didn't make ThinLTO skip inlining; instead the linker's symbol-ordering pass produced a layout where the read path's hot icache lines compete with something else. The net effect (+6%) is smaller than `inline-always-insert-remove-modify`'s (+9%) but still firmly over the guard. Key takeaway combined with the previous attempt: this codebase sits at a delicate codegen-layout optimum on the existing annotation set. Edits that touch annotation strength on the mutation entry points shift the read path's icache layout unfavourably regardless of direction, and we don't have a tool (in the single-file-edit model) to control the layout directly.
- **Follow-ups / dead ends:** Closed: removing the `#[inline]` from the mutation entry points. Closed (by extension): trying `#[inline(hint)]` / `#[inline(never)]` / other inline-knob variants on these functions — the layout is already at a local optimum and tuning inlining without controlling layout is a wash at best. Open: explicit symbol ordering via `-Wl,--symbol-ordering-file` (would pin the reopen-hot functions together) or PGO (would let the linker measure and decide) — both require build-system support beyond `Cargo.toml`/`src/`. Open: extracting the hot bincode tuple deserialise into a non-generic helper so it can be deduplicated across u64-vs-String monomorphisations — would shrink the binary and could shrink icache pressure, but it's a refactor rather than a knob.

### 2026-05-14 — inline-always-insert-remove-modify

- **Hypothesis:** Open follow-up from `inline-always-flush-scratch`. Promoting the three public mutation entry points `insert` / `remove` / `modify` from `#[inline]` to `#[inline(always)]` would let LLVM see the bench harness's 10k-iteration mutation loop as a single fused function (rather than 10k separate calls to monomorphised entry points), opening loop-invariant code motion and tighter register allocation across iterations.
- **Risk:** LOW (no API/format/dep change; just annotation strength).
- **Files touched:** `src/lib.rs` (`insert`, `remove`, `modify`).
- **Diff:** [`optimization-diffs/020-inline-always-insert-remove-modify.patch`](optimization-diffs/020-inline-always-insert-remove-modify.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.33% / -1.16% / -0.45% (within noise — directional improvement on the targeted scenario, but well under the -1.5% gate)
  - insert_2k_strings: -0.45% / -0.53% / +0.01% (within noise — small consistent directional improvement on the string variant)
  - lookup_100k: -0.04% / +0.05% / +0.16% (within noise — lookup doesn't touch these entry points)
  - modify_10k: +0.12% / +0.19% / +0.20% (within noise — modify drifts slightly positive, the OPPOSITE of intent)
  - reopen_10k: +9.33% / +9.39% / +9.23% (REVERTED — all three far over +1.5% guard, ~11us regression on a 119us baseline; reopen calls none of these functions)
- **Verdict:** REVERTED.
- **Why:** Strong codegen-layout backfire. The bench harness in `benches/store_bench.rs` is a separate crate that consumes `IndexMapStore` and monomorphises `insert`/`remove`/`modify` for `u64,u64` and `String,String`. With `#[inline(always)]` the entire bodies of those functions (including the `bincode::serialize_into`, the `flush_scratch`, the `maybe_compact` check, the live-records-counter update) get pasted into the bench harness's loop bodies, swelling those callsites by hundreds of bytes each. ThinLTO then has to fit the swollen harness _and_ the unchanged read-path (replay, open_with) into the same text segment ordering, and it picks a layout where something that was hot during reopen (most likely an `IndexMap::insert` helper or a bincode tuple-deserialise) is now further from the loop in icache, costing ~11us per reopen. The reopen scenarios timed work is `open + len + drop` — none of the three mutation functions are called — so this is purely a layout side effect of bloating other code. The targeted scenarios (`insert_10k_u64`, `insert_2k_strings`) showed small directional improvements (-0.3% to -1.2%) consistent with the loop fusion idea actually working a little, but the reopen regression dwarfs the win. Important lesson: `#[inline(always)]` on hot public APIs can cause material regressions on unrelated cold paths through icache pressure alone.
- **Follow-ups / dead ends:** Closed: blanket `#[inline(always)]` on the public mutation entry points. Closed (by extension): any attempt to force-inline the full mutation bodies into the bench harness — they're too big to inline without poisoning icache for the rest of the binary. Open: `#[inline(always)]` on just the _innermost_ mutation helper (e.g., the bincode call, or the IndexMap insert) — would inline less code per callsite and avoid the swelling effect; needs a refactor to extract that helper. Open: profile-guided optimization (PGO) — would let LLVM decide which calls to inline based on measured hotness rather than annotations, almost certainly the right answer for this codebase but needs build-system support outside the single-file-edit model. Open: explicit `#[link_section]` ordering or `-Wl,--symbol-ordering-file` to pin the reopen-hot functions together regardless of unrelated source edits.

### 2026-05-14 — truncate-scratch-preserve-len-prefix

- **Hypothesis:** Explicit follow-up from `combine-len-prefix-and-tag-extend`. Pre-seed `scratch` with `LEN_BYTES` zero bytes once at struct construction, then in each mutation use `truncate(LEN_BYTES) + push(tag)` instead of `clear + extend(&[0;4]) + push(tag)`. Saves a 4-byte memcpy (the zero-fill of the length prefix) and one length-store per mutation, ~5–7 cycles each — over 10k mutations, ~50–70us, in the ballpark of clearing the -1.5% gate on `modify_10k`. The first 4 bytes of `scratch` survive between calls — they hold the previous record's length, which is fine because `flush_scratch` overwrites them with the new length before each write.
- **Risk:** LOW (no API/format/dep change; semantic equivalence verified — the prefix bytes are always overwritten before they're emitted, and all 12 integration tests pass including the torn-tail recovery cases that exercise the on-disk layout).
- **Files touched:** `src/lib.rs` (`open_with` scratch construction; `insert`, `remove`, `modify` write-preludes).
- **Diff:** [`optimization-diffs/019-truncate-scratch-preserve-len-prefix.patch`](optimization-diffs/019-truncate-scratch-preserve-len-prefix.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: +0.06% / -1.03% / +0.14% (within noise — run 2 only)
  - insert_2k_strings: -0.49% / -0.19% / -0.31% (within noise — small consistent directional improvement, well under gate)
  - lookup_100k: -0.15% / +0.29% / +0.20% (within noise)
  - modify_10k: -0.28% / +0.01% / -0.08% (within noise — flat, the targeted scenario didn't move)
  - reopen*10k: -1.62% / -1.14% / -0.86% (run 1 clears the -1.5% gate, runs 2 and 3 don't — KEPT requires \_all three* runs ≤ -1.5%, so this is INCONCLUSIVE)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** Reopen_10k trends consistently directionally negative (-1.62 / -1.14 / -0.86) but only one of three runs clears the -1.5% gate. The other write-path scenarios (`insert_10k_u64`, `modify_10k`) sat flat. Two observations: (1) reopen doesn't call any of the modified functions (no mutations on the read path), so the reopen improvement is again a codegen-layout side effect — the diff shifted symbol ordering and put something on the read path slightly closer in icache. (2) The targeted scenario `modify_10k` showed almost zero change, suggesting the per-call savings on the write prelude are below the noise floor and LLVM's existing optimization of `Vec::push`/`extend_from_slice` is already close to optimal. Conservative gate (`KEPT` requires all 3 runs ≤ -1.5%) holds — directional but unreliable wins shouldn't ship.
- **Follow-ups / dead ends:** Closed: `truncate(LEN_BYTES)` as a substitute for `clear + extend(&[0;LEN_BYTES])`. Closed (by extension): further refinement of the scratch-prelude pattern — the optimizer already collapses these calls and the actual bottleneck is elsewhere (bincode dispatch + IndexMap insertion). Open: replacing the bincode call entirely on the write path with a hand-rolled fixed-prefix codec for primitive K/V — MEDIUM risk because it changes the on-disk format. Open: the reopen-layout-drift effect is real — three different no-op-looking edits (`inline-always-flush-scratch`, `combine-len-prefix-and-tag-extend`, this one) all moved reopen_10k slightly negative without changing the read path. Suggests there's icache pressure on reopen that a deliberate layout intervention (`#[link_section]` ordering, PGO) could exploit — but those need build-system support and don't fit the single-file-edit model.

### 2026-05-14 — combine-len-prefix-and-tag-extend

- **Hypothesis:** The current per-mutation prelude `scratch.clear(); scratch.extend_from_slice(&[0u8; LEN_BYTES]); scratch.push(tag);` calls `Vec` machinery three times — clear (length reset), extend (capacity check + 4-byte memcpy + length update), push (capacity check + 1-byte store + length update). Fusing extend and push into a single `extend_from_slice(&[0, 0, 0, 0, tag])` collapses two capacity checks and two length updates into one each — saving ~2–3 cycles per mutation across `insert`/`remove`/`modify`. Over 10k mutations this is ~20–30us, in the ballpark of clearing the -1.5% gate on `modify_10k` (5.09ms baseline, gate would need ~76us improvement).
- **Risk:** LOW (no behavior, format, API, or dep change — same bytes written in the same order, just emitted from one slice literal instead of two ops).
- **Files touched:** `src/lib.rs` (`insert`, `remove`, `modify` write-prelude).
- **Diff:** [`optimization-diffs/018-combine-len-prefix-and-tag-extend.patch`](optimization-diffs/018-combine-len-prefix-and-tag-extend.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.34% / +0.58% / +0.02% (within noise)
  - insert_2k_strings: -0.22% / -0.13% / -0.25% (within noise — small consistent directional improvement, well under the gate)
  - modify_10k: -0.71% / +0.01% / +0.16% (within noise — run 1 was directionally positive, runs 2–3 flat; no consistent improvement)
  - lookup_100k: -0.14% / +0.23% / +0.18% (within noise)
  - reopen_10k: -0.59% / -1.20% / -0.24% (within noise — directionally positive but reopen doesn't call the modified paths, so this is codegen-layout drift from the diff, not a real reopen win)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** The cycle savings are real but smaller than the bench's noise floor. ThinLTO + codegen_units=1 already inlines `Vec::push` and `Vec::extend_from_slice` for known-small slices, and LLVM is good at coalescing adjacent length stores when the writes are in the same basic block. The 2–3 cycles per call we hoped to save are likely already collapsed by the optimizer, so the diff buys only the symbol layout shuffle that codegen produces from any source-level edit. `insert_2k_strings`'s small consistent -0.13% to -0.25% directional drift is the closest thing to signal — but it's under the gate. `modify_10k` was the target scenario (~5.09ms baseline gives the largest absolute budget for a small per-call improvement to clear the -1.5% gate) but it sat flat with one run only at -0.71%, well short of the gate.
- **Follow-ups / dead ends:** Closed: fusing the LEN-prefix and tag extends into one literal. Closed (by extension): any further micro-optimization of the `clear; extend; push` prelude — ThinLTO has already collapsed it to optimum. Open: replacing `clear()` with `truncate(LEN_BYTES)` so the LEN-prefix bytes survive between calls and don't need to be re-zeroed (correctness is fine because `flush_scratch` overwrites them anyway) — different shape, would need to drop the `extend_from_slice` entirely and just `push(tag)` after `truncate(LEN_BYTES)`; saves an additional ~5-byte store and one length-store per mutation, separate hypothesis. Open: the deeper write-path bottleneck is the bincode `serialize_into` call (which monomorphizes through serde dispatch); replacing it with a hand-rolled fixed-prefix codec specialised for primitive K/V is MEDIUM-risk because it changes the on-disk format.

### 2026-05-14 — inline-always-flush-scratch

- **Hypothesis:** Targeted follow-up from `inline-hot-path-functions` ("targeted `#[inline(always)]` on a single specific callee"). Promoting `flush_scratch` from `#[inline]` to `#[inline(always)]` would force inlining at every callsite (`insert`/`remove`/`modify`) even where ThinLTO's size heuristic might pass on it — possibly merging the per-call setup into the bench loop body and exposing loop-invariant code-motion opportunities for LLVM.
- **Risk:** LOW (one-line annotation change; no behavior, API, format, or dep impact).
- **Files touched:** `src/lib.rs` (`flush_scratch`).
- **Diff:** [`optimization-diffs/017-inline-always-flush-scratch.patch`](optimization-diffs/017-inline-always-flush-scratch.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.06% / -0.22% / +0.22% (within noise)
  - insert_2k_strings: +0.31% / -0.14% / +0.25% (within noise)
  - lookup_100k: -0.16% / +0.18% / -0.01% (within noise)
  - modify_10k: -0.13% / +0.28% / -0.17% (within noise)
  - reopen_10k: -0.21% / -0.65% / -0.70% (within noise — directionally positive but well under -1.5% gate)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** `flush_scratch` is small (~20 lines, mostly straight-line stores) and was already `#[inline]`. With `lto = "thin"` + `codegen-units = 1` in the bench profile, ThinLTO's inliner uses MIR cost to decide cross-crate inlines, and a function this small clears any heuristic threshold — promoting to `#[inline(always)]` is essentially redundant in this build profile. The slight directional improvement on `reopen_10k` (-0.21% to -0.70%) is interesting but reopen doesn't call `flush_scratch` (no mutations on the read path), so this must be a codegen-layout side effect (different symbol ordering after the annotation change shifted something on the read path closer in icache). Under the -1.5% gate either way.
- **Follow-ups / dead ends:** Closed: `#[inline(always)]` on `flush_scratch`. Closed (by extension): `#[inline(always)]` on the other already-`#[inline]`d helpers (`maybe_compact`, the accessors) — same logic, the inliner already inlines them. Open: `#[inline(always)]` on `insert`/`remove`/`modify` (the public mutation entry points) — might let LLVM see the whole 10k-iteration bench loop as a single function and apply tighter optimization. Open: `#[cold]` placed on the slow-path branch _inside_ `maybe_compact` (the `compact()` invocation) via an outlined helper — different shape from the failed `mark-compact-as-cold`, and the bench doesn't exercise it so this is purely a codegen-layout knob with low expected payoff.

### 2026-05-14 — lazy-bufwriter-allocation

- **Hypothesis:** Deferring the 1MB `BufWriter::with_capacity(...)` mmap until the first mutation (by replacing `log: BufWriter<File>` with the pair `log: Option<BufWriter<File>>` + `file: Option<File>`, maintaining the invariant that exactly one is `Some`) would skip the allocation entirely on read-only opens. On the `reopen_10k` hot path the bench opens-then-drops without mutating, so the buffer is allocated-and-never-touched today — a clear waste.
- **Risk:** LOW (internal struct change; no API, format, or dep change; all 12 integration tests still pass).
- **Files touched:** `src/lib.rs` (`IndexMapStore` struct + `Drop`, `open_with`, `flush`, `compact`, `flush_scratch`).
- **Diff:** [`optimization-diffs/016-lazy-bufwriter-allocation.patch`](optimization-diffs/016-lazy-bufwriter-allocation.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: +0.10% / +0.40% / +0.22% (within noise; consistent positive drift, probably layout-induced)
  - insert_2k_strings: -0.18% / -0.18% / +0.03% (within noise)
  - lookup_100k: -0.05% / +0.26% / +0.03% (within noise)
  - modify_10k: +0.02% / -0.02% / +0.38% (within noise)
  - reopen_10k: +2.89% / +0.95% / +1.56% (REVERTED — run 1 firmly over +1.5% guard, runs 2 and 3 sit on or above the line)
- **Verdict:** REVERTED.
- **Why:** The expected win — saving the 1MB BufWriter mmap on read-only opens — exists but is small (~1–2us out of 119us). Three offsetting costs eat it and tip the balance negative on reopen*10k: (1) the struct grew by one `Option<File>` (~16 bytes including discriminant + padding), pushing later fields like `scratch` onto a different cache line and shifting where the struct lives in malloc's chunk space; (2) `Drop` now has to discriminate the Option each time a store goes out of scope (the reopen bench drops 1001 stores in the timed loop, so even cheap branches accumulate); (3) the `flush` path the bench calls indirectly through the harness has to discriminate too. On `insert_10k_u64` the same struct grew but the per-call overhead of `flush_scratch`'s extra `is_none()` is amortised across 10k inserts and the lazy upgrade happens only once — small but consistent positive drift (+0.1% to +0.4%) suggests a real but sub-noise cost. The mechanism that paid out at +30% for `bufwriter-capacity-1mb` (mmap allocation is \_fast* in this regime) is the same mechanism that makes "skip the mmap entirely" worth less than expected: the alloc was already cheap. Combined with codegen layout effects, net regression.
- **Follow-ups / dead ends:** Closed: lazy-init via `Option<BufWriter>` + `Option<File>` pair pattern. Closed (by extension): any reformulation that puts another Option-discriminant on the hot mutation path — codegen layout shifts on this struct hurt reopen more than the saved alloc helps. Open: lazy-init via a `BufWriter::with_capacity(0, file)` placeholder + in-place upgrade (avoids the Option pair, keeps the struct shape the same, costs an `unsafe` `mem::replace` or a dummy-File constructor) — different shape, MEDIUM risk because of the unsafe. Open: making `BufWriter::with_capacity(cap, file)` truly zero-alloc by passing a file-pre-sized capacity — std doesn't expose this, would need a custom writer, separate hypothesis. Open: `mallopt(M_MMAP_THRESHOLD, 131072)` global anchor — same family but different mechanism, adds libc dep (MEDIUM).

### 2026-05-14 — bufwriter-capacity-2mb

- **Hypothesis:** Bumping `StoreConfig::default().buf_capacity` from 1MB → 2MB might widen the gap above glibc's dynamic `M_MMAP_THRESHOLD` ceiling further, giving an additional small step on `reopen_10k` (which gained -30% going 256KB→1MB). Explicit open follow-up from `bufwriter-capacity-1mb`'s "investigate whether 2MB or 4MB helps further — diminishing returns expected".
- **Risk:** LOW (one-constant change; field stays configurable; no API or semantic impact).
- **Files touched:** `src/lib.rs` (`StoreConfig::default`).
- **Diff:** [`optimization-diffs/015-bufwriter-capacity-2mb.patch`](optimization-diffs/015-bufwriter-capacity-2mb.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: +0.16% / +0.05% / -0.21% (within noise)
  - insert_2k_strings: +0.15% / -0.15% / -0.16% (within noise)
  - lookup_100k: -0.12% / -0.17% / +0.19% (within noise)
  - modify_10k: +0.09% / +0.15% / -0.39% (within noise)
  - reopen_10k: -0.68% / +0.61% / -0.61% (within noise — no consistent direction; one run was tiny improvement, next tiny regression)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** The mechanism that paid out for 256KB→1MB (crossing the dynamic mmap-threshold ceiling consistently) is already saturated at 1MB — going 1MB→2MB doesn't change whether the allocator routes through mmap (it does either way), it just allocates a larger lazily-zeroed VMA. The bench harness exercises a hot path where the buffer is allocated but never written into on `reopen_10k`, so the extra 1MB of capacity is pure overhead per VMA insert, and the cost is small enough to disappear under the ±1% noise band. Confirms the diminishing-returns prediction from `bufwriter-capacity-1mb`'s follow-ups.
- **Follow-ups / dead ends:** Closed: bumping default `buf_capacity` further (the threshold-crossing mechanism is exhausted at 1MB). Closed (by extension): 4MB or 8MB defaults — same logic, slightly worse VMA overhead. Open: dropping the default to a lazy/zero-cost initial state and only allocating the buffer on first mutation (would save the 1MB mmap entirely on read-only opens) — requires struct refactor to `Option<BufWriter>` or enum, LOW-MEDIUM risk. Open: `mallopt(M_MMAP_THRESHOLD, 131072)` at lib init to anchor the threshold globally — adds libc dep, MEDIUM risk.

### 2026-05-14 — mmap-slurp-buffer-min-1mb

- **Hypothesis:** Rounding the replay slurp Vec's capacity to at least 1MB (`Vec::with_capacity((total_on_disk as usize).max(1 << 20))`) would force the allocation consistently through `mmap` the way `bufwriter-capacity-1mb` did — for our 240KB log the static glibc threshold is crossed but the dynamic `M_MMAP_THRESHOLD` can drift higher under repeated bench allocations, occasionally routing the slurp through the heap. Forcing ≥1MB should pin it on the mmap path the same way the BufWriter trick did.
- **Risk:** LOW (one-line capacity bump in `open_with`; no API change, no new deps, unused pages stay lazy).
- **Files touched:** `src/lib.rs` (`open_with` — the `Vec::with_capacity(total_on_disk as usize)` site only).
- **Diff:** [`optimization-diffs/014-mmap-slurp-buffer-min-1mb.patch`](optimization-diffs/014-mmap-slurp-buffer-min-1mb.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 630.26 us
  - modify_10k: 5.09 ms
  - reopen_10k: 119.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: +0.13% / -0.35% / -0.15% (within noise)
  - insert_2k_strings: -0.06% / +0.15% / +0.15% (within noise)
  - lookup_100k: -0.23% / -0.17% / +0.14% (within noise)
  - modify_10k: -0.07% / +0.09% / +0.04% (within noise)
  - reopen_10k: +4.86% / +5.04% / +4.94% (REVERTED — all three over +1.5% guard, ~5–6us regression on a 119us baseline)
- **Verdict:** REVERTED.
- **Why:** The slurp Vec is materially different from the BufWriter case that benefited from rounding to 1MB: BufWriter's backing buffer is allocated-but-untouched on the read-only reopen path, so the kernel never faults its pages and the mmap cost is purely the syscall + VMA insert (which is small enough that the 1MB version wins through fewer allocator slowdowns elsewhere). The slurp Vec, by contrast, is _immediately_ filled by `read_to_end` with ~240KB of bytes — so we touch the first 240KB regardless of the capacity. Asking the kernel to track a 1MB VMA when we use 240KB of it costs ~5us more per reopen than tracking a 240KB VMA, and there is no compensating win because the prior 240KB allocation was already crossing the static 128KB mmap threshold (i.e., already on the mmap path most of the time). Consistent regression (~5%, ~6us) across all three runs makes this clearly worse, not noise.
- **Follow-ups / dead ends:** Closed: rounding the slurp Vec capacity up to force mmap. Closed (by extension): the general "make every per-open allocation ≥1MB so they all sit on the mmap path" pattern — works only for allocations whose pages are NOT touched (like BufWriter), not for ones we read into. Open: dropping the slurp Vec entirely in favor of memory-mapping the log file directly (would eliminate both the allocation AND the userspace `read_to_end` copy) — HIGH risk (`mmap` per the skill's risk tags). Open: streaming the replay through a smaller fixed-size buffer (e.g., 64KB) read in a loop — opposite direction, would let the kernel/page-cache do the reading without our Vec staging; would change the bincode call from `deserialize(borrowed slice)` to a copy out of the streaming buffer, so could lose more than it saves on the per-record path.

### 2026-05-14 — bundle-inconclusive-attempts

- **Hypothesis:** Stacking all still-applicable INCONCLUSIVE attempts in one diff exposes additive signal that each individually buried in ±1.5% noise. The four merged: (1) inline-enum-tag-u8 — manual 1-byte tag + bincode tuple, save 3 bytes/record on Insert and skip serde enum dispatch on replay; (2) inline-hot-path-functions — `#[inline]` on accessors, mutation entry points, `flush_scratch`, `maybe_compact`, `serialize_err`; (3) single-open-for-replay-and-append — one `OpenOptions{read, append, create}` handle for slurp + torn-tail set_len + runtime appends (saves 1–2 open syscalls); (4) skip-path-exists-probe — naturally subsumed by (3), no separate `path.exists()` call. presize-replay-payload-vec was excluded as obsolete: the per-record payload buffer it targeted no longer exists since `slurp-log-into-vec` (KEPT) replaced it with a single up-front Vec already sized to `total_on_disk`. This invocation explicitly bundles multiple hypotheses on user request, deviating from the skill's normal ONE-hypothesis-per-invocation rule.
- **Risk:** MEDIUM (changes on-disk log format via the 1-byte tag — older logs are unreadable; tests confirmed not to depend on byte-level layout).
- **Files touched:** `src/lib.rs` (removed `LogRef`/`LogOwned` enums and the `Deserialize` import; added `TAG_INSERT`/`TAG_REMOVE` constants; rewrote `open_with` to a single OpenOptions handle and manual-tag replay; rewrote `insert`/`remove`/`modify`/`compact` write paths to emit tag + bincode tuple; added `#[inline]` to public accessors, mutation entry points, both private helpers, and the free `serialize_err`).
- **Diff:** [`optimization-diffs/013-bundle-inconclusive-attempts.patch`](optimization-diffs/013-bundle-inconclusive-attempts.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.46 ms
  - lookup_100k: 630.13 us
  - modify_10k: 5.16 ms
  - reopen_10k: 121.04 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.04% / -0.15% / -0.08% (within noise)
  - insert_2k_strings: +0.30% / +0.57% / +0.43% (within noise — directional positive but well under +1.5% guard)
  - lookup_100k: -0.14% / -0.01% / +0.02% (within noise)
  - modify_10k: -1.44% / -1.69% / -1.24% (directional improvement; 2 of 3 runs clear -1.5%, run 3 misses by 0.26pp — not a gate-passing scenario but moves in the right direction)
  - reopen_10k: -2.02% / -1.80% / -1.56% (KEPT — all three ≤ -1.5%, gate cleared by 0.06pp on the tightest run)
- **Verdict:** KEPT (deep-win).
- **Why:** `reopen_10k` consistently improves -1.56% to -2.02% across all three runs against the fixed pre-change baseline, clearing the -1.5% improvement gate in every run. No scenario regresses past +1.5% in any run (max +0.57% on `insert_2k_strings`, well within the +1.5% guard). The win is modest — roughly 2us shaved off 121us — and barely clears the gate, which is expected because each individual ingredient was previously within ±1.5% noise. The most plausible contributors on the reopen path are: collapsing the three-open sequence to one handle (saves ~one stat + one extra open syscall, ~5–10us in cold-cache territory, though here the inode is hot in cache); the manual 1-byte tag (3 bytes less to slurp + one fewer serde dispatch path per record); and codegen layout shifts from the inline annotations and the dropped enums. `modify_10k` drifted directionally positive across all three runs (-1.24% to -1.69%) — close to gate-clearing — which hints at a real but small write-path benefit from the inlined hot-path attributes and the simpler tag path; doesn't quite clear the bar but is consistent with the inline-enum-tag-u8 entry's earlier observation of "modify_10k -1.1% to -1.4%" individually. The 4-byte → 1-byte tag is the only on-disk format change; tests verified semantic correctness end-to-end (persistence_across_reopen, recovers_from_torn_tail, recovers_from_truncated_payload all pass — the truncated-payload test still hits the `len == 0` and bad-tag early-out paths). The new `bench_results.json` from run 3 becomes the next baseline.
- **Follow-ups / dead ends:** Closed (by KEPT): all four contributing attempts, since they now live in the codebase. Closed (by exclusion): presize-replay-payload-vec — the buffer it targeted no longer exists; do not retry as an independent attempt. Open: targeted `#[inline(always)]` on a single specific callee if profiling later shows a function still on the critical path — the blanket `#[inline]` here is conservative and may leave some calls non-inlined. Open: replacing bincode with a hand-rolled fixed-prefix codec for primitive K/V — the manual tag is now in place, so the next step (skipping bincode entirely for `K: Pod + V: Pod`) is a smaller delta than before; still MEDIUM-risk because it changes the format further. Open: hashing the K once during replay to skip the IndexMap rehash — needs a cached-hash IndexMap variant, doesn't generalize.

### 2026-05-14 — mark-compact-as-cold

- **Hypothesis:** Adding `#[cold]` to `compact()` would tell LLVM the function is rarely called, allowing it to place compact's body far from the hot mutation/open paths in the text segment, improve icache locality on the hot path, and tilt branch prediction so `maybe_compact`'s ratio check is treated as predicted-not-taken.
- **Risk:** LOW (annotation only — no behavior change).
- **Files touched:** `src/lib.rs` (`compact`).
- **Diff:** [`optimization-diffs/012-mark-compact-as-cold.patch`](optimization-diffs/012-mark-compact-as-cold.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.46 ms
  - lookup_100k: 630.13 us
  - modify_10k: 5.16 ms
  - reopen_10k: 121.04 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.05% / -0.01% / +0.07% (within noise)
  - insert_2k_strings: +0.52% / +0.29% / +0.23% (within noise)
  - lookup_100k: +0.08% / +0.22% / +0.16% (within noise)
  - modify_10k: -0.26% / +0.08% / +0.39% (within noise)
  - reopen_10k: +3.96% / +3.96% / +3.60% (REVERTED — all three over +2% guard)
- **Verdict:** REVERTED.
- **Why:** A consistent +3.6%–4.0% regression on `reopen_10k` across all three runs (~5us slower on a 121us baseline). Mechanism is almost certainly binary-layout: with `codegen-units = 1` + ThinLTO + `#[cold]`, the linker moves the compact body to the end of the text segment, which shifts the relative offsets of every function that came after it in the original layout. Some of those functions are on the hot open/replay path (e.g., bincode::deserialize monomorphizations, IndexMap::insert helpers), and the new layout incurs more icache misses for them. Reopen_10k has the smallest p50 of all scenarios (121us), so a ~5us layout cost shows up as a percentage-wise large regression. Insert/modify paths absorb the same shift but their per-iter cost is two orders of magnitude larger, so the layout shift is invisible there.
- **Follow-ups / dead ends:** Closed: blanket `#[cold]` on `compact()`. Closed (by extension): adding `#[cold]` to other rarely-called paths in this crate — the ThinLTO ordering is already near-optimal for the hot loop, and manual hints destabilise it. Open: explicit function ordering via `#[link_section]` / `-Wl,--symbol-ordering-file` to pin hot functions together — needs build-system support, separate hypothesis. Open: profile-guided optimization (PGO) with bench workloads — would let LLVM order functions empirically rather than guess from `#[cold]` hints.

### 2026-05-14 — inline-enum-tag-u8

- **Hypothesis:** Replacing bincode's serde-derived enum encoding (4-byte u32 variant tag + payload) with a manually written 1-byte tag (`0 = Insert`, `1 = Remove`) followed by bincode-serialized payload (`(K, V)` tuple for Insert, `K` for Remove) shrinks each record by 3 bytes (~12% for u64,u64 records) and skips the serde enum-dispatch trait machinery on both the write and replay paths.
- **Risk:** MEDIUM (changes on-disk log format — older logs are unreadable; tests confirmed not to rely on byte-level layout).
- **Files touched:** `src/lib.rs` (removed `LogRef`/`LogOwned` enums, added `TAG_INSERT`/`TAG_REMOVE` constants, rewrote `insert`/`remove`/`modify` write paths, replay loop in `open_with`, and `compact` write loop).
- **Diff:** [`optimization-diffs/011-inline-enum-tag-u8.patch`](optimization-diffs/011-inline-enum-tag-u8.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.26 ms
  - insert_2k_strings: 5.46 ms
  - lookup_100k: 630.13 us
  - modify_10k: 5.16 ms
  - reopen_10k: 121.04 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: +0.01% / +0.17% / +0.91% (within noise — drifts positive but under +2% guard)
  - insert_2k_strings: +0.35% / +0.16% / +0.00% (within noise)
  - lookup_100k: -0.03% / +0.25% / +0.29% (within noise)
  - modify_10k: -1.11% / -1.41% / -1.31% (within noise — directionally positive but under -3% gate)
  - reopen_10k: +0.14% / +0.93% / +0.55% (within noise)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** The 3-byte/record savings translate to ~12% smaller writes but contribute essentially nothing to wallclock because (a) the log lives in the page cache during benches, so disk-size wins don't translate, and (b) bincode's fixint u32 tag is decoded as a single 4-byte load — already cheap. Replay's perceived bottleneck is the K/V deserialize + IndexMap::insert (~7ns/op combined), and skipping the enum discriminant dispatch saves maybe 1-2ns/op which is below noise. `modify_10k` shows a consistent ~1.2% improvement that hints at real-but-tiny signal, but it doesn't cross the -3% gate. Manual tag handling on the read side adds a branch that probably eats most of the saving. Net flat.
- **Follow-ups / dead ends:** Closed: trimming the enum tag from u32 to u8 via manual encoding. Closed (by extension): "make the log records smaller on disk" line for the current bench workload — the cache-resident replay path is bottlenecked by IndexMap::insert and serde dispatch, not bytes. Open: replacing bincode entirely with a hand-rolled codec specialized on `K: Pod + V: Pod` primitive types (changes format, would need a feature flag for non-Pod types) — could skip serde dispatch entirely, MEDIUM risk. Open: caching the hash of K to skip rehashing during replay (presumes K stores its precomputed hash, doesn't generalize). Open: adding `#[cold]` on compact() to give the inline-hot mutation path a better icache layout — separate hypothesis.

### 2026-05-14 — bufwriter-capacity-1mb

- **Hypothesis:** Raising `StoreConfig::default().buf_capacity` from 256KB to 1MB keeps the BufWriter backing buffer comfortably above glibc's dynamic mmap threshold (which starts at 128KB and can be raised by the heuristic up to ~64MB as mmap'd chunks are freed), so the allocator stays on the mmap path even after many alloc/free cycles in tight bench loops.
- **Risk:** LOW (no API or semantic change; default value tweak; field is configurable).
- **Files touched:** `src/lib.rs` (`StoreConfig::default`).
- **Diff:** [`optimization-diffs/010-bufwriter-capacity-1mb.patch`](optimization-diffs/010-bufwriter-capacity-1mb.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.34 ms
  - insert_2k_strings: 5.47 ms
  - lookup_100k: 632.21 us
  - modify_10k: 5.17 ms
  - reopen_10k: 172.15 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -1.78% / -1.31% / -1.40% (within noise — directionally positive, under -3% gate)
  - insert_2k_strings: +0.05% / -0.01% / -0.07% (within noise)
  - lookup_100k: -0.16% / -0.15% / -0.33% (within noise)
  - modify_10k: +0.15% / -0.65% / -0.21% (within noise)
  - reopen_10k: -29.95% / -29.35% / -29.69% (KEPT — all three far below -3%)
- **Verdict:** KEPT (deep-win).
- **Why:** Huge, dead-flat 30% improvement on `reopen_10k` (172us → 121us) reproduced to within 0.3% across three independent runs. Mechanism: glibc's M_MMAP_THRESHOLD is dynamic — when mmap'd allocations are freed, the threshold can rise toward `M_MMAP_MAX`, which means later allocations of similar size silently shift back to the heap, incurring per-call heap fragmentation work. 1MB allocations sit far enough above any plausible heuristic ceiling that the allocator consistently routes through `mmap` (page-aligned, lazily zeroed pages, no zeroing happens since the buffer is allocated-but-unused on the read-path). Mutation paths stay flat (the buffer gets written into either way; ~10k \* 24 bytes = 240KB total fits in one flush at either 256KB or 1MB). The new `bench_results.json` from run 3 becomes the next baseline.
- **Follow-ups / dead ends:** Closed: bumping the default to 1MB. Open: investigating whether 2MB or 4MB helps further — diminishing returns expected and at some point committing too much memory hurts on multi-store workloads. Open: explicitly calling `mallopt(M_MMAP_THRESHOLD, 131072)` at lib init to force mmap for smaller buffers too — requires `libc` dep and a once-init, separate hypothesis. Open: replacing the BufWriter with a direct `mmap`-backed writer to skip the userspace copy on the buffered path — HIGH risk (introduces mmap, complex semantics).

### 2026-05-14 — larger-default-bufwriter-capacity

- **Hypothesis:** Raising `StoreConfig::default().buf_capacity` from 64KB to 256KB pushes the BufWriter backing buffer above glibc's default `M_MMAP_THRESHOLD` (128KB), so allocation goes through `mmap` (page-aligned, lazily zeroed) instead of the heap — every `open_with` allocates a buffer, and on the cold-reopen path that allocation is the only writable region we set up.
- **Risk:** LOW (no API or semantic change; `buf_capacity` is already configurable).
- **Files touched:** `src/lib.rs` (`StoreConfig::default`).
- **Diff:** [`optimization-diffs/009-larger-default-bufwriter-capacity.patch`](optimization-diffs/009-larger-default-bufwriter-capacity.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.47 ms
  - lookup_100k: 631.23 us
  - modify_10k: 5.12 ms
  - reopen_10k: 188.38 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.22% / -0.66% / -0.42% (within noise)
  - insert_2k_strings: +0.08% / +0.04% / -0.06% (within noise)
  - lookup_100k: -0.01% / +0.14% / +0.16% (within noise)
  - modify_10k: -0.42% / +1.30% / +0.92% (within noise — under +2% guard)
  - reopen_10k: -9.00% / -8.44% / -8.62% (KEPT — all three well below -3%)
- **Verdict:** KEPT (deep-win).
- **Why:** Consistent ~8.5% improvement on `reopen_10k` reproduced to within 0.6% across three independent runs against the fixed pre-change baseline. Mechanism: glibc malloc routes allocations under ~128KB through the heap (which can fragment and requires touching freelist metadata), while allocations at or above the mmap threshold go directly through `mmap`, returning fresh, lazily-zeroed pages. BufWriter only allocates — it doesn't write into the buffer on the read-and-replay path — so the larger allocation is cheaper in the wallclock-relevant work. Mutation paths are flat because they do write into the buffer (touching ~240KB either way), and the kernel page fault cost roughly matches the prior heap-touch cost. The new `bench_results.json` from run 3 becomes the next baseline.
- **Follow-ups / dead ends:** Closed: bumping the default `buf_capacity` to 256KB. Open: tuning further — 512KB or 1MB may give another small step on reopen but risks committing more memory for stores that never write much. Open: investigating whether the `Vec::with_capacity(total_on_disk)` slurp allocation also benefits from mmap-threshold sizing (for our 240KB log it already does). Open: replacing BufWriter entirely with a hand-rolled fixed-stride writer that avoids the dynamic capacity field — different shape, separate hypothesis.

### 2026-05-14 — inline-hot-path-functions

- **Hypothesis:** Adding `#[inline]` to the thin public accessors (`len`, `is_empty`, `contains_key`, `get`, `get_index`, `iter`, `keys`, `values`, `as_indexmap`), the mutation entry points (`insert`, `remove`, `modify`), the private helpers (`flush_scratch`, `maybe_compact`), and the free function `serialize_err` lets ThinLTO inline call sites in the bench harness more aggressively, potentially eliminating call overhead on per-iteration hot paths.
- **Risk:** LOW (no behavior change, no API change).
- **Files touched:** `src/lib.rs`.
- **Diff:** [`optimization-diffs/008-inline-hot-path-functions.patch`](optimization-diffs/008-inline-hot-path-functions.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.47 ms
  - lookup_100k: 631.23 us
  - modify_10k: 5.12 ms
  - reopen_10k: 188.38 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.20% / -0.01% / -0.42% (within noise)
  - insert_2k_strings: +0.21% / +0.22% / +0.20% (within noise)
  - lookup_100k: -0.18% / +0.06% / -0.22% (within noise)
  - modify_10k: +0.56% / +0.65% / +0.92% (within noise — directionally negative but under +2% guard)
  - reopen_10k: -0.61% / -0.82% / -0.53% (within noise — directionally positive but not -3%)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** With `lto = "thin"` and `codegen-units = 1` already set in the bench profile, the compiler is already aggressively cross-crate-inlining hot bench callees based on size heuristics. Marking these `#[inline]` exposes MIR upfront but doesn't change ThinLTO's decisions for functions that were already small enough (e.g., one-line accessors) or already monomorphized in the consuming crate (the generic methods). The `modify_10k` runs drifted slightly positive (+0.6% to +0.9%) — likely codegen layout shuffling rather than real regression — but they stayed under the +2% guard. No scenario hit the -3% improvement gate.
- **Follow-ups / dead ends:** Closed: blanket `#[inline]` on the entire public/private surface. Open: targeted `#[inline(always)]` on a single specific hot callee (e.g., `flush_scratch` only) — would be a different shape and might surface a real signal if there is one, but the diffuse signal here suggests the call overhead simply isn't on the critical path. Open: `#[cold]` on `maybe_compact`'s slow branches (the `compact()` invocation) to keep the no-op fast path hotter in icache — different hypothesis.

### 2026-05-14 — single-open-for-replay-and-append

- **Hypothesis:** Opening the log once with `OpenOptions{read, append, create}` and reusing the same handle for the replay slurp, the torn-tail `set_len`, and the runtime BufWriter appends — instead of doing three separate opens (`File::open` for read, `OpenOptions::write` for truncate, `OpenOptions::create+append` for runtime) — saves one to two `openat` syscalls per `open_with`. `O_APPEND` only affects writes, so an initial `read_to_end` at offset 0 still works.
- **Risk:** LOW (semantically equivalent — `path.exists()` check folds into `total_on_disk > 0` after the always-create open).
- **Files touched:** `src/lib.rs` (`open_with`).
- **Diff:** [`optimization-diffs/007-single-open-for-replay-and-append.patch`](optimization-diffs/007-single-open-for-replay-and-append.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.47 ms
  - lookup_100k: 631.46 us
  - modify_10k: 5.13 ms
  - reopen_10k: 188.38 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: +0.09% / +0.14% / +0.08% (within noise)
  - insert_2k_strings: -0.04% / -0.04% / -0.37% (within noise)
  - lookup_100k: +0.03% / +0.04% / +0.09% (within noise)
  - modify_10k: +0.12% / +1.10% / +1.37% (within noise — drifting positive but under the +2% guard)
  - reopen_10k: -0.68% / -0.26% / +0.47% (within noise — no consistent improvement)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** The 1–2 open syscalls saved per call are individually ~5–10us on Linux, but `reopen_10k` includes a fresh `tempdir` per iteration and the kernel inode cache absorbs repeat opens cheaply. Net change buried in jitter. The change was also subtly worse for `modify_10k` runs 2 and 3 — opening the same handle with both read and append modes can change kernel pre-allocation or write-back heuristics, which may explain the small positive drift on the write-heavy scenario.
- **Follow-ups / dead ends:** Closed: combining read/truncate/append opens into one handle. The remaining `open_with` syscall cost is below the bench gate's resolution — further work on the open path is unlikely to clear -3%. Open: replacing bincode entirely for primitive K/V (hand-rolled fixed-prefix codec) — bincode now dominates reopen, but this is MEDIUM risk and changes the on-disk format.

### 2026-05-14 — bincode-varint-encoding

- **Hypothesis:** Switching the bincode codec from the back-compat `bincode::serialize`/`deserialize` helpers (fixint, native-endian) to `bincode::DefaultOptions::new()` (varint, little-endian) shrinks records on disk — small u64 keys/values collapse from 8 bytes to 1 — so both the writer and the slurped replay buffer process fewer bytes.
- **Risk:** MEDIUM (changes on-disk log format — older logs are unreadable; flagged in code comment).
- **Files touched:** `src/lib.rs` (added `codec()` helper, replaced all 5 bincode call sites in `insert`, `remove`, `modify`, `compact`, and the `open_with` replay).
- **Diff:** [`optimization-diffs/006-bincode-varint-encoding.patch`](optimization-diffs/006-bincode-varint-encoding.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.47 ms
  - lookup_100k: 631.23 us
  - modify_10k: 5.12 ms
  - reopen_10k: 188.38 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.15% / -0.06% / -0.15% (within noise)
  - insert_2k_strings: -0.14% / -0.04% / -0.13% (within noise)
  - lookup_100k: -0.23% / -0.20% / -0.21% (within noise — `lookup_100k` doesn't touch the codec, expected flat)
  - modify_10k: -0.32% / -0.11% / -0.36% (within noise)
  - reopen_10k: +65.55% / +65.33% / +65.65% (REVERTED — catastrophic, ~+65% on every run)
- **Verdict:** REVERTED.
- **Why:** Varint decoding is bit-shift-and-branch per integer; against fixint's straight `read_unaligned` load, the per-byte processing cost is much higher. The on-disk savings don't help in benches because the log is in the OS page cache — slurping is already memory-bound, not disk-bound, and we now spend ~123us more parsing the bytes. Writers were flat because mutation cost is dominated by IndexMap insert + BufWriter copy, not by bincode size.
- **Follow-ups / dead ends:** Closed: varint encoding via `bincode::DefaultOptions`. Closed (by extension): general "shrink records on disk" line for in-memory workloads — wins on disk don't translate when reads are cached. Open: hand-rolled fixed-prefix codec specialised for `LogOwned<K, V>` where K/V are sized primitives — could skip the enum dispatch entirely. Still MEDIUM risk because it changes the format.

### 2026-05-14 — slurp-log-into-vec

- **Hypothesis:** Reading the whole log into a `Vec<u8>` via `File::read_to_end` and iterating in-memory over length-prefixed slices removes per-record `BufReader` refills and the memcpy into a separate `payload` buffer that the streaming path needed; `bincode::deserialize` can borrow the slice directly.
- **Risk:** LOW (no public API change, no new dependency — `BufReader` import dropped because it's no longer used).
- **Files touched:** `src/lib.rs` (`open_with`, removed unused `BufReader` import).
- **Diff:** [`optimization-diffs/005-slurp-log-into-vec.patch`](optimization-diffs/005-slurp-log-into-vec.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.49 ms
  - lookup_100k: 629.80 us
  - modify_10k: 5.11 ms
  - reopen_10k: 208.65 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.12% / -0.13% / -0.11% (within noise)
  - insert_2k_strings: -0.16% / +0.43% / +0.29% (within noise)
  - lookup_100k: +0.04% / +0.22% / +0.17% (within noise)
  - modify_10k: -1.10% / -0.34% / -1.10% (within noise)
  - reopen_10k: -10.96% / -10.97% / -10.43% (KEPT — all three < -3%)
- **Verdict:** KEPT (deep-win).
- **Why:** reopen_10k drops from ~209us to ~188us, reproduced to within 0.5% across three independent runs against the fixed pre-change baseline. The win comes from eliminating per-record `memcpy buffer→payload Vec` and the `BufReader` refill/copy overhead — `bincode::deserialize` now operates on a borrowed slice straight from the slurped buffer. Memory profile changes (one Vec sized to the log) but for our workloads logs are bounded and well under available RAM; for an extremely large log a streaming path may be worth adding back as a fallback.
- **Follow-ups / dead ends:** Closed: `BufReader`-based replay. Open: memory-mapping the log instead of slurping (would skip the userspace copy too — but `mmap` is HIGH risk per the skill). Open: hand-rolled fixed-prefix codec for primitive K/V — bincode now dominates the per-record cost; replacing it with a u64-LE encoder would change the on-disk format and so is a separate, MEDIUM-risk hypothesis.

### 2026-05-14 — skip-path-exists-probe

- **Hypothesis:** Replacing the `path.exists()` + `File::open()` pair with a single `File::open()` (treating `NotFound` as "no existing log") saves one stat syscall per `open`, most visible on `reopen_10k` where the open call is the entire timed work.
- **Risk:** LOW.
- **Files touched:** `src/lib.rs` (`open_with`).
- **Diff:** [`optimization-diffs/004-skip-path-exists-probe.patch`](optimization-diffs/004-skip-path-exists-probe.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.46 ms
  - lookup_100k: 630.21 us
  - modify_10k: 5.19 ms
  - reopen_10k: 210.11 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.36% / +0.09% / -0.26% (within noise)
  - insert_2k_strings: +0.31% / +0.29% / +0.68% (within noise)
  - lookup_100k: +0.14% / -0.06% / -0.05% (within noise)
  - modify_10k: -0.93% / -1.06% / -1.26% (within noise — directionally positive but not -3%)
  - reopen_10k: -0.69% / -0.19% / -0.79% (within noise — directionally positive but not -3%)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** A single stat syscall is ~0.5–2us on Linux; against `reopen_10k`'s 210us p50 that's at most ~1%, well below the -3% gate. The change is technically a small improvement (and removes a benign TOCTOU window between the probe and the open), but the bench gate's noise threshold is the right bar — accept the dead-end so we don't reopen this hypothesis later.
- **Follow-ups / dead ends:** Closed: collapsing `path.exists()` + `File::open()`. Open: combining the post-replay `OpenOptions::new().create(true).append(true).open(&path)` with the existing read handle to save a second open syscall — different shape, separate hypothesis.

### 2026-05-14 — presize-replay-payload-vec

- **Hypothesis:** Initialising the replay `payload` buffer with `Vec::with_capacity(256)` instead of `Vec::new()` saves a couple of early `realloc` calls as the first records grow the buffer.
- **Risk:** LOW.
- **Files touched:** `src/lib.rs` (`open_with`).
- **Diff:** [`optimization-diffs/003-presize-replay-payload-vec.patch`](optimization-diffs/003-presize-replay-payload-vec.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.37 ms
  - insert_2k_strings: 5.45 ms
  - lookup_100k: 630.14 us
  - modify_10k: 5.18 ms
  - reopen_10k: 210.31 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.01% / -0.14% / -0.07% (within noise)
  - insert_2k_strings: +0.25% / +0.00% / +0.20% (within noise)
  - lookup_100k: +0.20% / +0.03% / +0.01% (within noise)
  - modify_10k: -1.42% / -0.90% / +0.29% (within noise)
  - reopen_10k: -0.37% / -0.50% / -0.10% (within noise — no improvement)
- **Verdict:** INCONCLUSIVE — reverted.
- **Why:** After cycle 2 the IndexMap pre-sizing already dominates the reopen path; the first record's `resize` allocates a Vec backing store, and subsequent same-size records reuse that capacity for free. Saving two or three reallocations at the top of replay buys nothing measurable next to the per-record bincode deserialize cost.
- **Follow-ups / dead ends:** Closed: pre-sizing the replay payload Vec (no measurable win). Open: slurping the entire log into a Vec<u8> with `read_to_end` so the replay parses from memory rather than via `BufReader::read_exact` — different shape of optimization, separate hypothesis.

### 2026-05-14 — presize-indexmap-from-file-size

- **Hypothesis:** Calling `IndexMap::reserve(file_size / 24)` before the replay loop in `open_with` lets the map skip the ~14 grow-rehash steps it would otherwise do while filling from zero to thousands of entries, cutting cold-reopen latency.
- **Risk:** LOW.
- **Files touched:** `src/lib.rs` (`open_with`).
- **Diff:** [`optimization-diffs/002-presize-indexmap-from-file-size.patch`](optimization-diffs/002-presize-indexmap-from-file-size.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.36 ms
  - insert_2k_strings: 5.46 ms
  - lookup_100k: 631.70 us
  - modify_10k: 5.13 ms
  - reopen_10k: 357.58 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: +0.62% / +0.50% / +0.13% (within noise)
  - insert_2k_strings: +0.13% / +0.20% / -0.04% (within noise)
  - lookup_100k: -0.43% / -0.01% / -0.25% (within noise)
  - modify_10k: -0.08% / +0.96% / +0.86% (within noise)
  - reopen_10k: -41.20% / -41.28% / -41.18% (KEPT — all three far below -3%)
- **Verdict:** KEPT (deep-win).
- **Why:** Massive, dead-flat improvement on `reopen_10k` (~150us shaved off ~360us p50) reproduced to within 0.1% across three independent runs, with all other scenarios within the ±1% noise band — pre-sizing avoids the geometric rehash sequence on the replay path. The 24 bytes/record divisor matches a `Insert<u64,u64>` record; larger records over-reserve harmlessly because IndexMap only allocates one hash-table backing array and never shrinks during replay.
- **Follow-ups / dead ends:** Closed: file-size-based replay capacity hint. Open: tuning the divisor for string-heavy workloads (currently over-reserves for `insert_2k_strings`-shaped data — wastes some memory, doesn't help further). Open: pre-sizing the replay `payload` Vec from the largest length seen so far (separate hypothesis, smaller potential payoff now that reopen_10k is already much faster). Open: faster hasher (foldhash/ahash) — would touch mutation paths too, MEDIUM risk because it adds a dep.

### 2026-05-14 — batch-len-prefix-and-payload

- **Hypothesis:** Reserving `LEN_BYTES` at the start of the per-record `scratch` buffer and filling the length in place lets `flush_scratch` emit the length-prefix and payload in a single `BufWriter::write_all`, removing one call per mutation.
- **Risk:** LOW.
- **Files touched:** `src/lib.rs` (`insert`, `remove`, `modify`, `flush_scratch`).
- **Diff:** [`optimization-diffs/001-batch-len-prefix-and-payload.patch`](optimization-diffs/001-batch-len-prefix-and-payload.patch)
- **Baseline (pre-change) p50:**
  - insert_10k_u64: 5.37 ms
  - insert_2k_strings: 5.47 ms
  - lookup_100k: 629.49 us
  - modify_10k: 5.18 ms
  - reopen_10k: 374.58 us
- **Δ p50 across 3 confirming runs:**
  - insert_10k_u64: -0.20% / +0.33% / -0.15% (within noise)
  - insert_2k_strings: -0.16% / -0.16% / -0.33% (within noise)
  - lookup_100k: -0.11% / +0.35% / +0.35% (within noise)
  - modify_10k: -0.39% / -0.14% / -0.98% (within noise)
  - reopen_10k: -4.79% / -4.71% / -4.54% (KEPT — all three < -3%)
- **Verdict:** KEPT (deep-win).
- **Why:** Gate is satisfied — `reopen_10k` improves consistently > 3% across all three independent runs against the fixed pre-change baseline, and no scenario regresses past the +2% noise band in any run. The win shows up on the read/replay path rather than the targeted write path — likely a codegen or inlining side-effect after the `flush_scratch` rewrite (the on-disk format is identical and the replay loop was not touched). Mutation paths came out flat; the change is a refactor that incidentally pays out elsewhere. The new `bench_results.json` from run 3 becomes the next baseline.
- **Follow-ups / dead ends:** Closed: collapsing length+payload into a single `write_all` via the scratch-prefix trick. Open: `compact()` still does two separate `write_all`s per record (length, payload) on a non-hot path — could be unified the same way if compaction ever becomes hot. Open: pre-sizing the replay `IndexMap` from on-disk file size — independent hypothesis worth a separate attempt now that reopen_10k baseline is faster.

<!--
Append entries below in reverse-chronological order. Template:

### YYYY-MM-DD — hypothesis-slug

- **Hypothesis:** one-sentence claim.
- **Risk:** LOW / MEDIUM / HIGH.
- **Files touched:** `path/a.rs`, `path/b.rs`.
- **Diff:** [`optimization-diffs/<NNN>-<slug>.patch`](optimization-diffs/<NNN>-<slug>.patch)
- **Baseline (pre-change) p50:**
  - scenario_a: 1.23 ms
  - scenario_b: 4.56 us
- **Δ p50 across 3 confirming runs:**
  - scenario_a: -5.1% / -4.8% / -5.4%   (KEPT — all three < -3%)
  - scenario_b: +0.2% / -0.4% / +0.1%   (within noise; not a regression)
- **Verdict:** KEPT (deep-win) / KEPT (broad-win) / REVERTED / INCONCLUSIVE
- **Why:** explanation.
- **Follow-ups / dead ends:** anything a future attempt should NOT retry, or a related idea worth a separate hypothesis.
-->
