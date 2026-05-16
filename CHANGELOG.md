# Changelog

All notable changes to `indexmap_store` are documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
this crate adheres to [SemVer](https://semver.org/spec/v2.0.0.html) with
pre-1.0 rules (breaking changes bump the minor while the major is 0).

## [0.2.3] — 2026-05-16

Internal cleanup, optimization experiments, and benchmark harness improvements
since 0.2.2; no public API changes.

### Performance
- Faster scratch-buffer preparation in the write path via in-place reuse
  (`unsafe { set_len }`): ~3.4% on `modify_10k`, ~2.0% on `insert_2k_strings`
  (3272771).
