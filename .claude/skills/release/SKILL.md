---
description: Cut a new release of the indexmap_store crate end-to-end. Verifies master is clean, integration tests pass, and clippy is warning-free; determines the prior release baseline from the most recent `release:` commit; classifies every commit since that baseline as user-facing or internal; proposes a SemVer bump (MAJOR/MINOR/PATCH, with pre-1.0 rules); shows the user the proposed version + release notes for approval; then prepends a new section to `CHANGELOG.md` at the project root (Keep a Changelog format), bumps Cargo.toml, refreshes Cargo.lock, runs the publish dry-run, commits as `release: X.Y.Z — <summary>`, and tags `vX.Y.Z`. Stops short of `cargo publish` and `git push` — those require explicit user action.
---

# /release

You are cutting **ONE** new release of the `indexmap_store` crate, end-to-end, in a single invocation. The whole point of this skill is a **safe, reproducible release**: never publish a dirty tree, never publish a tree that fails tests, never invent a version bump the user did not see, and never push or publish without explicit human go-ahead.

The crate currently follows pre-1.0 SemVer (`0.x.y`). Pre-1.0 rules differ from post-1.0 — see §3.

---

## 0. Prerequisites (abort with explanation if any fail)

Run in order; if any check fails, print why and stop. Do not attempt to fix the failure automatically (e.g. do not stash, do not switch branches, do not amend commits).

1. **Working tree clean.**
   ```bash
   git status --porcelain
   ```
   Output must be empty. If anything is staged, unstaged, or untracked, abort.

2. **On `master`.**
   ```bash
   git rev-parse --abbrev-ref HEAD
   ```
   Must print `master`. If not, abort and tell the user to switch first.

3. **Up to date with origin (if a remote exists).**
   ```bash
   git remote get-url origin >/dev/null 2>&1 && git fetch origin master
   ```
   If origin exists, compare `HEAD` with `origin/master`:
   ```bash
   git rev-list --left-right --count HEAD...origin/master
   ```
   Both sides must be `0` (no diverging commits). If you are ahead, ask the user to push first or confirm the release is intentionally local. If you are behind, abort. If no `origin` remote is configured, skip this check and note it in the release summary.

4. **Toolchain works.**
   ```bash
   cargo --version && rustc --version
   ```

5. **`Cargo.toml` parses and exposes a version.**
   ```bash
   grep -E '^version\s*=' Cargo.toml
   ```
   Must match exactly one `version = "X.Y.Z"` line under `[package]`. Capture the value as `CURRENT_VERSION`.

---

## 1. Find the prior release baseline

The baseline is the most recent commit whose subject starts with `release:`. The crate uses subjects of the form `release: 0.2.2 — docs, license, metadata` (em-dash separator). Match on the `release:` prefix alone — the rest is free-form.

```bash
PRIOR_RELEASE_SHA=$(git log --format='%H %s' | awk '/^[0-9a-f]+ release:/{print $1; exit}')
PRIOR_RELEASE_SUBJECT=$(git log -1 --format='%s' "$PRIOR_RELEASE_SHA")
```

If no `release:` commit is found, this is the first release ever cut by this skill. Treat the **initial commit** of the repository (`git rev-list --max-parents=0 HEAD`) as the baseline, and note "first release via /release" in the release notes.

Sanity check: the version in `Cargo.toml` at `PRIOR_RELEASE_SHA` should match the version in the subject. If they disagree, stop and surface the mismatch to the user — likely a previous release was hand-edited and the baseline detection is ambiguous.

---

## 2. Classify every commit since the baseline

Collect commits **after** `PRIOR_RELEASE_SHA` up to `HEAD`:

```bash
git log --format='%H%x09%s%x09%b%x1e' "$PRIOR_RELEASE_SHA..HEAD"
```

(`%x09` = tab between fields, `%x1e` = record separator between commits.)

For each commit, classify by **subject prefix** (the leading token before `:`) and by **body markers**:

| Prefix                                                                                  | Category                       | Default bump contribution |
| --------------------------------------------------------------------------------------- | ------------------------------ | ------------------------- |
| `feat`                                                                                  | **user-facing feature**        | MINOR                     |
| `fix`                                                                                   | **user-facing bug fix**        | PATCH                     |
| `perf`                                                                                  | **user-facing perf**           | PATCH                     |
| `docs` (touches `README.md`, `src/lib.rs` doctest/rustdoc, `LICENSE`, `Cargo.toml` meta) | **user-facing docs**           | PATCH                     |
| `docs` (touches only `OPTIMIZATIONS.md`, `CONTRIBUTING.md`, `.claude/`)                  | internal docs                  | none                      |
| `optimize`                                                                              | internal perf experiment       | none (already shipped if KEPT, but baseline-trim_mean change is not user-visible API) |
| `simplify`, `refactor`, `chore`, `style`                                                 | internal                       | none                      |
| `bench`, `test`, `ci`, `build` (when not changing the published crate surface)           | internal                       | none                      |
| `skill`                                                                                 | internal (skill files only)    | none                      |
| `release`                                                                               | should not appear past baseline; if it does, abort with "release commit found after baseline" | n/a |
| anything else / no prefix                                                                | **unclassified — ask user**    | n/a                       |

**Breaking-change markers** (override the prefix mapping):

- Subject contains `!` immediately before `:` (e.g. `feat!: drop sync API`)
- Commit body contains a line starting with `BREAKING CHANGE:` or `BREAKING-CHANGE:`
- Subject or body explicitly says "removes public", "renames public", "changes signature of pub fn", etc.

A breaking change forces a MAJOR-equivalent bump (see §3 for pre-1.0 mapping).

**Reclassification check:** for every `perf:` / `optimize:` / internal-tagged commit, quickly inspect the diff (`git show --stat <sha>`). If it touches `src/lib.rs` in a way that changes the **public** API surface (added/removed/renamed `pub fn`, `pub struct` field changes, trait bound changes on public items), reclassify as user-facing — the prefix lied. Do this with `cargo public-api` if available, else by grepping the diff for `pub fn `, `pub struct `, `pub enum `, `pub trait `, `pub use `. Optional internal API churn behind `#[doc(hidden)]` does not count.

---

## 3. Propose the version bump

Aggregate the per-commit contributions into one bump category:

- Any breaking change present → **breaking**
- Else any `feat` present → **minor**
- Else any `fix` / `perf` / user-facing `docs` present → **patch**
- Else (only internal commits) → **no release needed** — surface this to the user and ask whether to release anyway (e.g. for a docs-only metadata fix) or abort.

Then map to the next version, **using pre-1.0 SemVer rules** while `CURRENT_VERSION` starts with `0.`:

| Category    | Pre-1.0 bump (0.x.y)         | Post-1.0 bump (≥1.0.0)        |
| ----------- | ---------------------------- | ----------------------------- |
| breaking    | 0.x.y → 0.(x+1).0            | X.Y.Z → (X+1).0.0             |
| minor       | 0.x.y → 0.(x+1).0            | X.Y.Z → X.(Y+1).0             |
| patch       | 0.x.y → 0.x.(y+1)            | X.Y.Z → X.Y.(Z+1)             |

Set `NEXT_VERSION` accordingly. Do not edit `Cargo.toml` yet.

---

## 4. Build the release notes (user-affecting only)

Release notes live in **`CHANGELOG.md` at the crate root** in [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) style. Each release prepends one new `## [<NEXT_VERSION>] — YYYY-MM-DD` section directly under the top-level `# Changelog` heading.

Release notes contain **only** the user-affecting commits identified in §2 (feat, fix, perf, user-facing docs, breaking). Internal commits (optimize, simplify, bench, skill, refactor, chore, internal docs) are **excluded** — they do not change library behavior the user can observe through the public API or documentation.

**If `CHANGELOG.md` does not exist yet**, create it with this header before prepending the first section:

```markdown
# Changelog

All notable changes to `indexmap_store` are documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
this crate adheres to [SemVer](https://semver.org/spec/v2.0.0.html) with
pre-1.0 rules (breaking changes bump the minor while the major is 0).

```

Section format (drop empty subsections entirely):

```markdown
## [<NEXT_VERSION>] — YYYY-MM-DD

### Breaking changes
- <subject without prefix>. (<short-sha>)
  Migration: <one-line guidance from the commit body, if present>

### Added
- <feat subjects without prefix>. (<short-sha>)

### Fixed
- <fix subjects without prefix>. (<short-sha>)

### Performance
- <perf subjects without prefix>. (<short-sha>)

### Documentation
- <user-facing docs subjects without prefix>. (<short-sha>)
```

Use the commit short SHA from `git rev-parse --short <SHA>`. Strip the conventional-commit prefix (`feat:`, `fix:`, etc.) and any leading whitespace; keep the rest of the subject verbatim. If a `BREAKING CHANGE:` body line is present, surface it as the Migration line; otherwise omit Migration.

**Insertion mechanic** — do not append at EOF; the newest release sits at the top:

1. Read `CHANGELOG.md` (or seed it with the header above if missing).
2. Find the first blank line following the `# Changelog` header block.
3. Insert the new section there, separated by a blank line on each side.
4. Write the file back.

Also write the same rendered new section (just that one section, not the whole file) to `/tmp/release-notes-<NEXT_VERSION>.md` — the §9 commit message body and §10 summary read from this scratch file. The committed source of truth is `CHANGELOG.md`; the scratch file exists only so the commit body and the GitHub-Release paste-buffer match what landed in the changelog.

---

## 5. Show the user the plan and get approval

Print, in this exact shape, then **stop and ask for confirmation** before any file edits to `CHANGELOG.md` or `Cargo.toml`:

```
Proposed release
----------------
  Prior:   <CURRENT_VERSION>   @ <PRIOR_RELEASE_SHA short>  (<PRIOR_RELEASE_SUBJECT>)
  Next:    <NEXT_VERSION>      (<bump category>)
  Commits: <N user-facing> / <M total> since baseline

CHANGELOG.md section preview (will be prepended)
------------------------------------------------
<contents of /tmp/release-notes-<NEXT_VERSION>.md>

Proceed? (yes / no / different-version)
```

If the user picks `different-version`, accept their explicit `X.Y.Z` value and re-render §4 against it before proceeding.

If the user says `no`, exit with no changes made. The `/tmp/release-notes-*` scratch file may stay (tmpfs, self-cleans on reboot).

---

## 6. Test gate (mandatory)

Run the full pre-release test sweep against the release profile where applicable. Any failure → stop, do not modify `Cargo.toml`, surface the failure verbatim.

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --doc
cargo test --release --test integration
cargo build --release
cargo doc --no-deps
```

Notes:

- `cargo test --doc` catches broken examples in rustdoc, which are part of the published crate surface and a common silent breakage.
- `cargo test --release --test integration` runs the integration suite under the same optimization level the user will get from crates.io, catching release-only UB or codegen surprises before publishing.
- `cargo build --release` is cheap once the test suite has compiled the release profile; it also primes `cargo package` in §8.
- `cargo doc` failure on stable usually means a broken intra-doc link in a public docstring — also user-facing.

If `cargo fmt --check` is the only failure, fix it with `cargo fmt`, but verify the resulting diff is purely whitespace before continuing.

---

## 7. Bump the version and write CHANGELOG.md

Two file edits, in this order:

1. **`CHANGELOG.md`** — prepend the new `## [<NEXT_VERSION>] — YYYY-MM-DD` section per §4 (create the file with the header block if it does not exist).
2. **`Cargo.toml`** — change exactly the single `version = "<CURRENT_VERSION>"` line under `[package]` to `version = "<NEXT_VERSION>"`. Do not touch anything else (no edition bump, no metadata churn, no dependency updates — those are separate commits).

Refresh `Cargo.lock` by doing a no-op build:

```bash
cargo check --offline 2>/dev/null || cargo check
```

Verify `Cargo.lock` now lists `name = "indexmap_store"` with the new version exactly once:

```bash
grep -A1 '^name = "indexmap_store"$' Cargo.lock
```

If the version in `Cargo.lock` does not match `NEXT_VERSION`, abort with a clear error — something is wrong with the workspace state.

---

## 8. Package dry-run

Confirm the crate would actually publish, against the `include` list in `Cargo.toml`:

```bash
cargo package --list
cargo package --no-verify
```

`--list` prints the files that will be bundled into the `.crate`. Sanity-check that:

- `src/lib.rs` is present.
- `Cargo.toml`, `Cargo.lock` (or `Cargo.toml.orig` after packaging), `README.md`, `LICENSE`, and `CHANGELOG.md` are present.
- No `OPTIMIZATIONS.md`, `bench_results.json`, `verify_results.json`, `optimization-diffs/`, `.claude/`, `tests/`, or `benches/` slip through (the existing `include = […]` in `Cargo.toml` should already exclude these — verify, do not trust).

If `CHANGELOG.md` is missing from the `cargo package --list` output, the `include = […]` array in `Cargo.toml` does not list it. Add `"CHANGELOG.md"` to the `include` array as part of this same release commit (it is a release-coupled metadata change, not unrelated churn) and re-run the dry-run.

`--no-verify` skips the redundant compile (we already built in §6) but still produces the tarball. If `cargo package` errors (e.g. uncommitted changes warning because `Cargo.toml` was just edited), pass `--allow-dirty` and proceed — the change will be committed in §9.

---

## 9. Commit and tag

Stage exactly the version bump and commit using the historical subject style — `release: X.Y.Z — <summary>` with an em-dash separator and a short summary derived from the release-notes section titles (e.g. `release: 0.3.0 — new feat, fix in compact, docs`).

```bash
git add CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "$(cat <<'EOF'
release: <NEXT_VERSION> — <summary>

<contents of /tmp/release-notes-<NEXT_VERSION>.md without the leading `## [X.Y.Z] — DATE` heading>

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git tag -a "v<NEXT_VERSION>" -m "indexmap_store <NEXT_VERSION>"
```

Verify the tag points at the new commit:

```bash
git show --stat "v<NEXT_VERSION>" | head -5
```

---

## 10. Final summary (do NOT publish or push)

Print:

```
Released indexmap_store <NEXT_VERSION> locally.

  Commit:    <new commit short SHA>
  Tag:       v<NEXT_VERSION>
  Changelog: CHANGELOG.md (new section at top)

Next steps (NOT done automatically):
  git push origin master
  git push origin v<NEXT_VERSION>
  cargo publish
```

Stop here. **Do not run** `cargo publish` or any `git push`. Both are externally-visible, irreversible actions that require explicit human go-ahead per the user-confirmation policy in the harness.

---

## Hard rules

- Never run `cargo publish` from this skill. Ever. Printing the command in §10 is the limit.
- Never run `git push` from this skill. Ever.
- Never amend prior commits, force-push, or move existing tags.
- Never weaken or skip the §6 test gate (no `--no-run`, no test filters, no `cargo test -- --ignored=…` shortcuts). All seven commands run, all must pass.
- Never invent a version bump the user has not approved in §5. If unclassified commits exist, ask.
- Never include internal commits (`optimize`, `simplify`, `bench`, `skill`, internal `refactor`, `chore`, internal `docs`) in the user-facing release notes. They live in the git log for archaeology, not in the changelog the user reads.
- Never edit anything other than `CHANGELOG.md`, `Cargo.toml` (and the auto-refreshed `Cargo.lock`) during a bump. The one allowed exception is adding `"CHANGELOG.md"` to the `include` array in `Cargo.toml` on the first release that ships it (§8). If the release needs README updates, doctests, or other metadata changes, those are pre-release commits the user makes before invoking `/release`.
- If the working tree is not clean, you are NOT on `master`, or the remote is divergent — abort. Do not "fix" these conditions silently.
- If only internal commits exist since the prior release, the default answer is "no release needed". Only proceed if the user explicitly overrides.
