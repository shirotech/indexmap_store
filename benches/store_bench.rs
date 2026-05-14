//! Custom benchmark harness for `IndexMapStore`.
//!
//! Run with `cargo bench`. Each invocation:
//!   1. Warms up each scenario (default 5 iterations) — discards timings.
//!   2. Collects N timed samples (default 1001, odd so the median is unambiguous).
//!   3. Reports min / p50 / p90 per scenario.
//!   4. Loads `bench_results.json` (at the crate root, survives `cargo clean`)
//!      from the previous run, prints the Δ against the current p50, then
//!      overwrites the file with the new run.
//!
//! Tune via env:
//!   * `BENCH_WARMUP`, `BENCH_SAMPLES`, `BENCH_THRESHOLD` (percent).
//!   * `BENCH_FILTER` — substring filter on scenario names.
//!   * `BENCH_BASELINE` — path to a results JSON used as the "previous" reference
//!     instead of `bench_results.json`. The current run is still written to
//!     `bench_results.json`. Used by `/optimize` to compare three independent
//!     runs against one fixed pre-change snapshot.
//!   * `BENCH_PRINT_ONLY` — if set (and not "0"), skip benchmarking and just
//!     reprint the last results from `bench_results.json` (comparing against
//!     `BENCH_BASELINE` if set). Does not overwrite the JSON.
//!
//! `p50` is the consistency anchor — robust to OS jitter — and the Δ tag flags
//! `[+]` improvement, `[-]` regression, `[~]` within noise threshold so you can
//! quickly tell whether a change paid off.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use index_map_store::IndexMapStore;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy)]
struct Stats {
    samples: usize,
    min_ns: u128,
    p50_ns: u128,
    p90_ns: u128,
    max_ns: u128,
    mean_ns: u128,
}

#[derive(Serialize, Deserialize, Default)]
struct Results {
    timestamp_unix: u64,
    benchmarks: BTreeMap<String, Stats>,
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    let n = sorted.len();
    if n == 0 {
        return Duration::ZERO;
    }
    let idx = (((n - 1) as f64) * p).round() as usize;
    sorted[idx]
}

fn finalize(mut times: Vec<Duration>) -> Stats {
    times.sort();
    let n = times.len();
    let sum_ns: u128 = times.iter().map(|d| d.as_nanos()).sum();
    Stats {
        samples: n,
        min_ns: times[0].as_nanos(),
        p50_ns: percentile(&times, 0.50).as_nanos(),
        p90_ns: percentile(&times, 0.90).as_nanos(),
        max_ns: times[n - 1].as_nanos(),
        mean_ns: sum_ns / n as u128,
    }
}

fn run<S, T, F>(
    name: &str,
    warmup: usize,
    samples: usize,
    mut setup: S,
    mut work: F,
) -> (String, Stats)
where
    S: FnMut() -> T,
    F: FnMut(&mut T),
{
    for _ in 0..warmup {
        let mut s = setup();
        work(&mut s);
        drop(s);
    }
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let mut s = setup();
        let t = Instant::now();
        work(&mut s);
        let elapsed = t.elapsed();
        drop(s);
        times.push(elapsed);
    }
    (name.into(), finalize(times))
}

fn fmt_ns(ns: u128) -> String {
    let f = ns as f64;
    if ns >= 1_000_000_000 {
        format!("{:.2} s", f / 1e9)
    } else if ns >= 1_000_000 {
        format!("{:.2} ms", f / 1e6)
    } else if ns >= 1_000 {
        format!("{:.2} us", f / 1e3)
    } else {
        format!("{} ns", ns)
    }
}

fn fmt_delta(prev: u128, cur: u128, threshold: f64) -> String {
    if prev == 0 {
        return "n/a".into();
    }
    let pct = (cur as f64 - prev as f64) * 100.0 / prev as f64;
    let tag = if pct < -threshold {
        "[+]"
    } else if pct > threshold {
        "[-]"
    } else {
        "[~]"
    };
    format!("{:+6.2}% {}", pct, tag)
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn results_path() -> PathBuf {
    // CARGO_MANIFEST_DIR is set by cargo at compile time and points at the
    // crate root, so the results file lives next to Cargo.toml and is not
    // touched by `cargo clean`. Falls back to a relative path if unset.
    match option_env!("CARGO_MANIFEST_DIR") {
        Some(dir) => PathBuf::from(dir).join("bench_results.json"),
        None => PathBuf::from("bench_results.json"),
    }
}

fn load_previous() -> Option<Results> {
    let p = match env::var("BENCH_BASELINE") {
        Ok(s) if !s.is_empty() => PathBuf::from(s),
        _ => results_path(),
    };
    let s = fs::read_to_string(&p).ok()?;
    serde_json::from_str(&s).ok()
}

fn save(r: &Results) -> std::io::Result<()> {
    let p = results_path();
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir)?;
    }
    let s = serde_json::to_string_pretty(r).unwrap();
    fs::write(&p, s)
}

fn print_table(current: &Results, previous: Option<&Results>, threshold: f64) {
    let samples = current
        .benchmarks
        .values()
        .next()
        .map(|s| s.samples)
        .unwrap_or(0);
    println!();
    println!("IndexMapStore benchmark - {samples} samples per scenario");
    match previous {
        Some(p) => println!(
            "Comparing against previous run (unix {}); noise threshold = +/-{:.1}%",
            p.timestamp_unix, threshold
        ),
        None => println!("No previous results - this run becomes the baseline."),
    }
    println!();
    println!(
        "{:<26} {:>12} {:>16} {:>12} {:>12}",
        "name", "p50", "delta p50", "p90", "min"
    );
    println!("{:-<82}", "");
    for (name, s) in &current.benchmarks {
        let delta = previous
            .and_then(|p| p.benchmarks.get(name))
            .map(|prev| fmt_delta(prev.p50_ns, s.p50_ns, threshold))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<26} {:>12} {:>16} {:>12} {:>12}",
            name,
            fmt_ns(s.p50_ns),
            delta,
            fmt_ns(s.p90_ns),
            fmt_ns(s.min_ns),
        );
    }
    println!();
}

struct ModState {
    _dir: tempfile::TempDir,
    store: IndexMapStore<u64, u64>,
}

struct GetState {
    _dir: tempfile::TempDir,
    store: IndexMapStore<u64, u64>,
}

struct OpenState {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn main() {
    let warmup: usize = env::var("BENCH_WARMUP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let samples: usize = env::var("BENCH_SAMPLES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1001);
    let threshold: f64 = env::var("BENCH_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2.0);
    let filter = env::var("BENCH_FILTER").ok();
    let print_only = env::var("BENCH_PRINT_ONLY")
        .ok()
        .is_some_and(|s| !s.is_empty() && s != "0");

    if print_only {
        let current: Results = match fs::read_to_string(results_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(r) => r,
            None => {
                eprintln!(
                    "BENCH_PRINT_ONLY: no {} to read",
                    results_path().display()
                );
                return;
            }
        };
        print_table(&current, load_previous().as_ref(), threshold);
        return;
    }

    let mut current = Results {
        timestamp_unix: now_unix(),
        benchmarks: BTreeMap::new(),
    };

    let want = |name: &str| -> bool {
        filter
            .as_deref()
            .map(|f| name.contains(f))
            .unwrap_or(true)
    };

    if want("insert_10k_u64") {
        let (n, s) = run(
            "insert_10k_u64",
            warmup,
            samples,
            || tempfile::tempdir().unwrap(),
            |dir| {
                let path = dir.path().join("store.log");
                let mut store: IndexMapStore<u64, u64> = IndexMapStore::open(&path).unwrap();
                for i in 0..10_000u64 {
                    black_box(
                        store
                            .insert(black_box(i), black_box(i.wrapping_mul(7)))
                            .unwrap(),
                    );
                }
                store.flush().unwrap();
            },
        );
        current.benchmarks.insert(n, s);
    }

    if want("insert_2k_strings") {
        let (n, s) = run(
            "insert_2k_strings",
            warmup,
            samples,
            || tempfile::tempdir().unwrap(),
            |dir| {
                let path = dir.path().join("store.log");
                let mut store: IndexMapStore<String, String> =
                    IndexMapStore::open(&path).unwrap();
                for i in 0..2_000u32 {
                    let k = format!("key:{:06}", i);
                    let v = format!("value-{}-{:032}", i, i);
                    black_box(store.insert(black_box(k), black_box(v)).unwrap());
                }
                store.flush().unwrap();
            },
        );
        current.benchmarks.insert(n, s);
    }

    if want("modify_10k") {
        let (n, s) = run(
            "modify_10k",
            warmup,
            samples,
            || {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("store.log");
                let mut store: IndexMapStore<u64, u64> = IndexMapStore::open(&path).unwrap();
                for i in 0..10_000u64 {
                    store.insert(i, 0).unwrap();
                }
                store.flush().unwrap();
                ModState { _dir: dir, store }
            },
            |state| {
                for i in 0..10_000u64 {
                    state
                        .store
                        .modify(&black_box(i), |v| {
                            *v = v.wrapping_add(1);
                        })
                        .unwrap();
                }
                state.store.flush().unwrap();
            },
        );
        current.benchmarks.insert(n, s);
    }

    if want("lookup_100k") {
        let (n, s) = run(
            "lookup_100k",
            warmup,
            samples,
            || {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("store.log");
                let mut store: IndexMapStore<u64, u64> = IndexMapStore::open(&path).unwrap();
                for i in 0..10_000u64 {
                    store.insert(i, i).unwrap();
                }
                GetState { _dir: dir, store }
            },
            |state| {
                let mut acc: u64 = 0;
                for i in 0..100_000u64 {
                    let k = i % 10_000;
                    if let Some(v) = state.store.get(&black_box(k)) {
                        acc = acc.wrapping_add(*v);
                    }
                }
                black_box(acc);
            },
        );
        current.benchmarks.insert(n, s);
    }

    if want("reopen_10k") {
        let (n, s) = run(
            "reopen_10k",
            warmup,
            samples,
            || {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("store.log");
                {
                    let mut store: IndexMapStore<u64, u64> =
                        IndexMapStore::open(&path).unwrap();
                    for i in 0..10_000u64 {
                        store.insert(i, i.wrapping_mul(7)).unwrap();
                    }
                    store.flush().unwrap();
                }
                OpenState { _dir: dir, path }
            },
            |state| {
                let store: IndexMapStore<u64, u64> =
                    IndexMapStore::open(&state.path).unwrap();
                black_box(store.len());
            },
        );
        current.benchmarks.insert(n, s);
    }

    let previous = load_previous();
    print_table(&current, previous.as_ref(), threshold);
    save(&current).expect("write bench results");
    println!("Results saved to {}", results_path().display());
}
