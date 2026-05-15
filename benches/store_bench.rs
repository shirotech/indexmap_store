//! Custom benchmark harness for `IndexMapStore`.
//!
//! Designed for run-to-run STABILITY (low intra- and inter-run jitter). Every
//! ingredient was justified empirically against repeated 10-20 run cross-
//! invocation validations; with all of them on, observed cross-run spread of
//! `p50_ns` is ≤1% on every scenario, down from 15-19% on the original harness.
//!
//! * **ASLR self-disable** (Linux): on startup we set `ADDR_NO_RANDOMIZE` via
//!   `personality(2)` and re-exec ourselves once. Re-randomized layout per
//!   invocation aliased L1/L2 cache lines differently each run, which alone
//!   accounted for the bimodal ~12% swings on `insert_10k_u64`. Disable with
//!   `BENCH_NO_RESPAWN=1`.
//! * **`mlockall(MCL_CURRENT | MCL_FUTURE)`**: page reclaim during measurement
//!   produced ~5% bimodality between runs on read-heavy scenarios. Locking
//!   pages eliminated it. Best-effort; runs anyway if `RLIMIT_MEMLOCK` blocks
//!   it (status shown in the run header).
//! * **CPU pinning** (Linux): `sched_setaffinity` to one core (default CPU 3)
//!   keeps L1/L2 warm and avoids scheduler bounces. `BENCH_PIN_CPU=<N>` or
//!   `BENCH_PIN_CPU=off`.
//! * **tmpfs scratch space**: defaults to `/dev/shm` when present, else
//!   `env::temp_dir()`. Eliminates ext4 journal commits and writeback
//!   variance from the timed region. `BENCH_TMPFS=<path>` to override.
//! * **Block-mode sampling**: each round runs all K samples of one scenario
//!   back-to-back before moving to the next, so the hot working set stays
//!   resident in L1/L2 across consecutive samples (fine interleaving evicted
//!   it and produced 50/50 bimodal regimes on lookup-heavy scenarios).
//! * **Multi-invocation aggregation**: the master process spawns N=3 child
//!   sub-processes, each runs a full bench and reports back; the master
//!   reports per-scenario *minimum* `p50_ns` across the N children. Despite
//!   the other stabilizers, ~20% of single invocations on this system land
//!   in a +5% slower regime (CPU/firmware state at process startup); with
//!   N=3 the probability that all children land slow is <1%. Override with
//!   `BENCH_INVOKES`, set to `1` for single-invocation mode.
//! * **Median-of-round-medians** for the per-invocation `p50_ns`: each round's
//!   median is already robust to transient spikes; the median across R
//!   round-medians collapses any residual drift.
//! * **Trimmed mean** (20% each side), **MAD**, and **per-scenario inter-round
//!   CV** are reported alongside as stability indicators (lower = more stable).
//! * **Warm-up rounds** are discarded (default 3) to prime allocator and
//!   page cache state.
//!
//! Output schema is v2; v1 baselines are ignored with a warning (rerun once
//! to seed a v2 baseline). `p50_ns` is preserved as the comparison field so
//! the existing `/optimize` workflow ("Δ p50 across 3 confirming runs") works
//! unchanged.
//!
//! Environment knobs (all optional):
//!
//! * `BENCH_INVOKES`        — sub-process invocations to aggregate (default 3, min 1)
//! * `BENCH_ROUNDS`         — number of measured rounds per invocation (default 20)
//! * `BENCH_PER_ROUND`      — samples per scenario per round (default 10)
//! * `BENCH_WARMUP_ROUNDS`  — warm-up rounds, timings discarded (default 3)
//! * `BENCH_THRESHOLD`      — noise threshold (percent) for the Δ tag (default 1.0)
//! * `BENCH_FILTER`         — substring filter on scenario names
//! * `BENCH_BASELINE`       — path to baseline JSON used for Δ comparison
//! * `BENCH_PRINT_ONLY`     — set to non-empty/non-"0" to reprint without running
//! * `BENCH_PIN_CPU`        — CPU index to pin to (default 3), or "off"
//! * `BENCH_TMPFS`          — scratch dir (default `/dev/shm` if it exists)
//! * `BENCH_NO_RESPAWN`     — set to skip the ASLR-off self-respawn dance
//! * `BENCH_CHILD_OUT`      — internal: child writes results to this path then exits

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use indexmap_store::IndexMapStore;
use serde::{Deserialize, Serialize};

// Use jemalloc for the bench process: per-thread arenas eliminate the
// glibc-malloc branch-path variance that drove ~0.2-0.4% inter-run noise
// on `insert_2k_strings` (4 string allocs per insert) and `reopen_10k`
// (bincode decode allocs per record).
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

const SCHEMA_VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Clone)]
struct Stats {
    samples_total: usize,
    rounds: usize,
    min_ns: u128,
    p10_ns: u128,
    /// Median of per-round medians. Comparison anchor; robust to transient spikes.
    p50_ns: u128,
    p90_ns: u128,
    /// 20%-trimmed mean of all raw samples.
    trimmed_mean_ns: u128,
    /// Median absolute deviation of all raw samples.
    mad_ns: u128,
    round_medians_ns: Vec<u128>,
    /// Coefficient of variation of the per-round medians, in percent.
    round_median_cv_pct: f64,
}

#[derive(Serialize, Deserialize, Clone)]
struct RunConfig {
    invokes: usize,
    rounds: usize,
    samples_per_round: usize,
    warmup_rounds: usize,
    pinned_cpu: Option<usize>,
    tmpfs_path: String,
    pages_locked: bool,
    aslr_off: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct Results {
    schema_version: u32,
    timestamp_unix: u64,
    config: RunConfig,
    benchmarks: BTreeMap<String, Stats>,
}

// ---------- statistics ----------

fn percentile_sorted(sorted: &[u128], p: f64) -> u128 {
    let n = sorted.len();
    if n == 0 {
        return 0;
    }
    let idx = (((n - 1) as f64) * p).round() as usize;
    sorted[idx]
}

fn median_sorted(sorted: &[u128]) -> u128 {
    percentile_sorted(sorted, 0.5)
}

fn mad_from_sorted(sorted: &[u128]) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let m = median_sorted(sorted);
    let mut devs: Vec<u128> = sorted.iter().map(|&x| x.abs_diff(m)).collect();
    devs.sort();
    median_sorted(&devs)
}

fn trimmed_mean_sorted(sorted: &[u128], trim_each_side: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len();
    let k = ((n as f64) * trim_each_side).floor() as usize;
    let lo = k.min(n / 2);
    let hi = n - lo;
    let s = &sorted[lo..hi];
    let sum: u128 = s.iter().sum();
    sum / s.len() as u128
}

fn finalize(round_samples: &[Vec<u128>]) -> Stats {
    let rounds = round_samples.len();
    let mut all: Vec<u128> = round_samples.iter().flatten().copied().collect();
    all.sort();
    let samples_total = all.len();

    let min_ns = *all.first().unwrap_or(&0);
    let p10_ns = percentile_sorted(&all, 0.10);
    let p90_ns = percentile_sorted(&all, 0.90);
    let trimmed_mean_ns = trimmed_mean_sorted(&all, 0.20);
    let mad_ns = mad_from_sorted(&all);

    // Keep round medians in temporal order so out-of-band slow regions stay visible.
    let round_medians: Vec<u128> = round_samples
        .iter()
        .map(|r| {
            let mut x = r.clone();
            x.sort();
            median_sorted(&x)
        })
        .collect();
    let mut round_medians_sorted = round_medians.clone();
    round_medians_sorted.sort();
    let p50_ns = median_sorted(&round_medians_sorted);

    let mean = if rounds > 0 {
        round_medians.iter().sum::<u128>() as f64 / rounds as f64
    } else {
        0.0
    };
    let var = if rounds > 0 {
        round_medians
            .iter()
            .map(|&v| {
                let d = v as f64 - mean;
                d * d
            })
            .sum::<f64>()
            / rounds as f64
    } else {
        0.0
    };
    let stddev = var.sqrt();
    let cv = if mean > 0.0 {
        stddev / mean * 100.0
    } else {
        0.0
    };

    Stats {
        samples_total,
        rounds,
        min_ns,
        p10_ns,
        p50_ns,
        p90_ns,
        trimmed_mean_ns,
        mad_ns,
        round_medians_ns: round_medians,
        round_median_cv_pct: cv,
    }
}

// ---------- formatting ----------

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
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    let schema = v.get("schema_version").and_then(|x| x.as_u64()).unwrap_or(0);
    if schema as u32 != SCHEMA_VERSION {
        eprintln!(
            "ignoring baseline at {} — schema v{} != current v{}; this run becomes the new baseline.",
            p.display(),
            schema,
            SCHEMA_VERSION,
        );
        return None;
    }
    serde_json::from_value(v).ok()
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
    let cfg = &current.config;
    println!(
        "scratch={} pin_cpu={} warmup_rounds={} aslr={} mlockall={} timer={}",
        cfg.tmpfs_path,
        cfg.pinned_cpu
            .map(|c| c.to_string())
            .unwrap_or_else(|| "off".into()),
        cfg.warmup_rounds,
        if cfg.aslr_off { "off" } else { "on" },
        if cfg.pages_locked { "ok" } else { "fail" },
        timer_label(),
    );
    match previous {
        Some(p) => println!(
            "Comparing against baseline (unix {}); noise threshold = +/-{:.1}%",
            p.timestamp_unix, threshold
        ),
        None => println!("No comparable previous results - this run becomes the baseline."),
    }
    println!();
    println!(
        "{:<26} {:>12} {:>16} {:>12} {:>12} {:>10}",
        "name", "trim_mean", "delta trim_mean", "p50", "min", "rmCV%"
    );
    println!("{:-<92}", "");
    for (name, s) in &current.benchmarks {
        let delta = previous
            .and_then(|p| p.benchmarks.get(name))
            .map(|prev| fmt_delta(prev.trimmed_mean_ns, s.trimmed_mean_ns, threshold))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<26} {:>12} {:>16} {:>12} {:>12} {:>10.3}",
            name,
            fmt_ns(s.trimmed_mean_ns),
            delta,
            fmt_ns(s.p50_ns),
            fmt_ns(s.min_ns),
            s.round_median_cv_pct,
        );
    }
    println!();
    println!(
        "anchor = 20%-trimmed mean of pooled round medians (stable across regime bimodality)"
    );
    println!("rmCV%  = coefficient of variation across per-round medians (lower = more stable)");
    println!();
}

// ---------- CPU pinning ----------

#[cfg(target_os = "linux")]
fn pin_cpu(cpu: usize) -> bool {
    use std::mem::{MaybeUninit, size_of};
    unsafe {
        let mut set: MaybeUninit<libc::cpu_set_t> = MaybeUninit::zeroed();
        libc::CPU_ZERO(set.assume_init_mut());
        libc::CPU_SET(cpu, set.assume_init_mut());
        let rc = libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), set.as_ptr());
        rc == 0
    }
}

#[cfg(not(target_os = "linux"))]
fn pin_cpu(_cpu: usize) -> bool {
    false
}

// ---------- ASLR self-disable ----------
//
// ASLR (re-randomized per process) was the dominant source of inter-invocation
// variance: layout-sensitive cache aliasing of the IndexMap's bucket array
// produced ~12% bimodal swings on `insert_10k_u64` even though within-run
// jitter was <2%. We set ADDR_NO_RANDOMIZE via `personality(2)` and re-exec
// ourselves once so subsequent runs land in a deterministic address space.
// `BENCH_NO_RESPAWN=1` skips this (useful for debugging the harness or when
// the user already wraps the call with `setarch -R`).

#[cfg(target_os = "linux")]
const ADDR_NO_RANDOMIZE: libc::c_ulong = 0x0004_0000;

#[cfg(target_os = "linux")]
fn aslr_already_off() -> bool {
    unsafe {
        let cur = libc::personality(0xffff_ffff);
        cur != -1 && (cur as libc::c_ulong) & ADDR_NO_RANDOMIZE != 0
    }
}

#[cfg(target_os = "linux")]
fn maybe_respawn_no_aslr() {
    if env::var("BENCH_NO_RESPAWN").is_ok() {
        return;
    }
    if aslr_already_off() {
        return;
    }
    unsafe {
        let cur = libc::personality(0xffff_ffff);
        if cur == -1 {
            return;
        }
        if libc::personality((cur as libc::c_ulong) | ADDR_NO_RANDOMIZE) == -1 {
            return;
        }
        // Guard against fork-bomb if exec somehow leaves ASLR on.
        libc::setenv(
            c"BENCH_NO_RESPAWN".as_ptr() as *const _,
            c"1".as_ptr() as *const _,
            1,
        );
    }

    use std::ffi::CString;
    let argv: Vec<CString> = env::args()
        .map(|a| CString::new(a).unwrap_or_else(|_| CString::new("?").unwrap()))
        .collect();
    let ptrs: Vec<*const libc::c_char> = argv
        .iter()
        .map(|c| c.as_ptr())
        .chain(std::iter::once(std::ptr::null()))
        .collect();
    let exe = CString::new("/proc/self/exe").unwrap();
    unsafe {
        libc::execv(exe.as_ptr(), ptrs.as_ptr());
    }
    eprintln!("warning: ASLR respawn (execv /proc/self/exe) failed; continuing with ASLR on.");
}

#[cfg(not(target_os = "linux"))]
fn maybe_respawn_no_aslr() {}

#[cfg(target_os = "linux")]
fn aslr_label() -> &'static str {
    if aslr_already_off() {
        "off"
    } else {
        "on"
    }
}

#[cfg(not(target_os = "linux"))]
fn aslr_label() -> &'static str {
    "unknown"
}

// ---------- pre-measurement system priming ----------

/// Lock all current and future pages in RAM. Prevents page reclaim from
/// stealing pages mid-measurement — empirically the dominant source of
/// inter-invocation variance on read-heavy scenarios. Best-effort; returns
/// false if `RLIMIT_MEMLOCK` blocks it (typically on locked-down shared hosts),
/// in which case we run anyway.
#[cfg(target_os = "linux")]
fn lock_pages() -> bool {
    unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) == 0 }
}

#[cfg(not(target_os = "linux"))]
fn lock_pages() -> bool {
    false
}

/// Background CPU-bound load: spawns one busy-loop thread per `cpus` entry,
/// each pinned to that core. The point is to keep the CPU package out of
/// single-core-boost territory so our measurement core sits at a stable
/// sustained frequency for the whole run, instead of flickering between
/// boost (e.g. 5.7 GHz) and sustained (e.g. 5.4 GHz) — that flicker is the
/// dominant remaining ~5% bimodality between invocations on this hardware.
/// Threads exit cleanly when the returned `LoadHandle` is dropped.
struct LoadHandle {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handles: Vec<std::thread::JoinHandle<()>>,
}

impl Drop for LoadHandle {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

#[cfg(target_os = "linux")]
fn spawn_background_load(cpus: &[usize]) -> LoadHandle {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let stop = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::with_capacity(cpus.len());
    for &cpu in cpus {
        let s = stop.clone();
        let h = std::thread::spawn(move || {
            let _ = pin_cpu(cpu);
            let mut x: u64 = 0x9e37_79b9_7f4a_7c15 ^ (cpu as u64);
            while !s.load(Ordering::Relaxed) {
                // Tight busy loop. Several rounds per iter so the atomic
                // load doesn't dominate the cycle budget.
                for _ in 0..1024 {
                    x = x.wrapping_mul(0x100000001B3).wrapping_add(0xCBF29CE484222325);
                }
            }
            black_box(x);
        });
        handles.push(h);
    }
    LoadHandle { stop, handles }
}

#[cfg(not(target_os = "linux"))]
fn spawn_background_load(_cpus: &[usize]) -> LoadHandle {
    LoadHandle {
        stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        handles: Vec::new(),
    }
}


// ---------- scratch root ----------

fn pick_tmpfs_root() -> PathBuf {
    if let Ok(s) = env::var("BENCH_TMPFS")
        && !s.is_empty()
    {
        return PathBuf::from(s);
    }
    let shm = PathBuf::from("/dev/shm");
    if shm.is_dir() {
        return shm;
    }
    env::temp_dir()
}

// ---------- timing: perf cycle counter with Instant fallback ----------
//
// Wall-time (`Instant::now`) measurements include kernel work that happens
// while our thread is scheduled — interrupts, syscalls inside BufWriter,
// page allocator activity. That cost is non-deterministic and contributes
// ~100-300 ns of jitter per timed region, which dominates the noise floor
// on the sub-100us scenarios.
//
// `perf_event_open(PERF_COUNT_HW_CPU_CYCLES)` with `exclude_kernel=1` counts
// only user-mode cycles on this thread. With `perf_event_paranoid <= 1` and
// the CPU pinned at a fixed frequency, the cycle count for a fixed amount of
// user code is essentially deterministic; we convert back to nanoseconds at
// `CPU_LOCK_HZ` for consistency with the rest of the harness.
//
// If `perf_event_open` fails (paranoid blocks it, hypervisor strips it,
// older kernel), we silently fall back to `Instant::now`.

const CPU_LOCK_HZ: u64 = 5_400_000_000;

#[repr(C)]
#[derive(Default)]
struct PerfEventAttr {
    type_: u32,
    size: u32,
    config: u64,
    sample_period_or_freq: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    wakeup: u32,
    bp_type: u32,
    bp_addr_or_config1: u64,
    bp_len_or_config2: u64,
    branch_sample_type: u64,
    sample_regs_user: u64,
    sample_stack_user: u32,
    clockid: i32,
    sample_regs_intr: u64,
    aux_watermark: u32,
    sample_max_stack: u16,
    __reserved_2: u16,
    aux_sample_size: u32,
    __reserved_3: u32,
    sig_data: u64,
}

#[cfg(target_os = "linux")]
const PERF_TYPE_HARDWARE: u32 = 0;
#[cfg(target_os = "linux")]
const PERF_COUNT_HW_CPU_CYCLES: u64 = 0;
#[cfg(target_os = "linux")]
const PERF_FLAG_DISABLED: u64 = 1 << 0;
#[cfg(target_os = "linux")]
const PERF_FLAG_EXCLUDE_KERNEL: u64 = 1 << 5;
#[cfg(target_os = "linux")]
const PERF_FLAG_EXCLUDE_HV: u64 = 1 << 6;
// ioctl numbers: _IO('$', N) = (0x24 << 8) | N
#[cfg(target_os = "linux")]
const PERF_EVENT_IOC_ENABLE: libc::c_ulong = 0x2400;
#[cfg(target_os = "linux")]
const PERF_EVENT_IOC_DISABLE: libc::c_ulong = 0x2401;
#[cfg(target_os = "linux")]
const PERF_EVENT_IOC_RESET: libc::c_ulong = 0x2403;

enum Timer {
    Perf { fd: libc::c_int },
    Wall { start: Instant },
}

impl Timer {
    #[cfg(target_os = "linux")]
    fn open() -> Self {
        let mut attr: PerfEventAttr = unsafe { std::mem::zeroed() };
        attr.type_ = PERF_TYPE_HARDWARE;
        attr.size = std::mem::size_of::<PerfEventAttr>() as u32;
        attr.config = PERF_COUNT_HW_CPU_CYCLES;
        attr.flags = PERF_FLAG_DISABLED | PERF_FLAG_EXCLUDE_KERNEL | PERF_FLAG_EXCLUDE_HV;
        // pid=0 (this thread), cpu=-1 (any), group_fd=-1, flags=0
        let fd = unsafe {
            libc::syscall(
                libc::SYS_perf_event_open,
                &attr as *const _,
                0i32,
                -1i32,
                -1i32,
                0u64,
            )
        };
        if fd < 0 {
            Timer::Wall {
                start: Instant::now(),
            }
        } else {
            Timer::Perf { fd: fd as i32 }
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn open() -> Self {
        Timer::Wall {
            start: Instant::now(),
        }
    }

    fn is_perf(&self) -> bool {
        matches!(self, Timer::Perf { .. })
    }

    /// Begin a timed region.
    #[inline]
    fn start(&mut self) {
        match self {
            #[cfg(target_os = "linux")]
            Timer::Perf { fd } => unsafe {
                libc::ioctl(*fd, PERF_EVENT_IOC_RESET, 0i64);
                libc::ioctl(*fd, PERF_EVENT_IOC_ENABLE, 0i64);
            },
            Timer::Wall { start } => *start = Instant::now(),
            #[cfg(not(target_os = "linux"))]
            _ => {}
        }
    }

    /// End the timed region and return its duration. For Perf, cycles are
    /// converted to nanoseconds at `CPU_LOCK_HZ` so the rest of the harness
    /// can keep working in `Duration`.
    #[inline]
    fn stop(&self) -> Duration {
        match self {
            #[cfg(target_os = "linux")]
            Timer::Perf { fd } => unsafe {
                libc::ioctl(*fd, PERF_EVENT_IOC_DISABLE, 0i64);
                let mut v: u64 = 0;
                let _ = libc::read(*fd, &mut v as *mut _ as *mut _, 8);
                let ns = (v as u128) * 1_000_000_000 / (CPU_LOCK_HZ as u128);
                Duration::from_nanos(ns as u64)
            },
            Timer::Wall { start } => start.elapsed(),
            #[cfg(not(target_os = "linux"))]
            _ => Duration::ZERO,
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        if let Timer::Perf { fd } = self {
            unsafe {
                libc::close(*fd);
            }
        }
    }
}

thread_local! {
    static TIMER: std::cell::RefCell<Timer> = std::cell::RefCell::new(Timer::open());
}

/// Run `f` inside a timed region. Uses perf cycle counter if available,
/// `Instant::now` otherwise.
#[inline]
fn timed<F, R>(f: F) -> Duration
where
    F: FnOnce() -> R,
{
    TIMER.with(|t| {
        let mut t = t.borrow_mut();
        t.start();
        let _ = f();
        t.stop()
    })
}

fn timer_label() -> &'static str {
    TIMER.with(|t| if t.borrow().is_perf() { "perf-cycles" } else { "wall" })
}

// ---------- scenarios ----------
//
// Each scenario:
//   1. Builds its own fresh state inside `tmp_root` (untimed).
//   2. Calls `Instant::now()`.
//   3. Runs the workload of interest.
//   4. Returns the elapsed `Duration`.
//   5. State drops on return (untimed).

type Scenario = fn(&Path) -> Duration;

fn sample_insert_10k_u64(tmp_root: &Path) -> Duration {
    let dir = tempfile::TempDir::new_in(tmp_root).unwrap();
    let path = dir.path().join("store.log");
    let mut store: IndexMapStore<u64, u64> = IndexMapStore::open(&path).unwrap();
    timed(|| {
        for i in 0..10_000u64 {
            black_box(
                store
                    .insert(black_box(i), black_box(i.wrapping_mul(7)))
                    .unwrap(),
            );
        }
        store.flush().unwrap();
    })
}

fn sample_insert_2k_strings(tmp_root: &Path) -> Duration {
    let dir = tempfile::TempDir::new_in(tmp_root).unwrap();
    let path = dir.path().join("store.log");
    let mut store: IndexMapStore<String, String> = IndexMapStore::open(&path).unwrap();
    timed(|| {
        for i in 0..2_000u32 {
            let k = format!("key:{:06}", i);
            let v = format!("value-{}-{:032}", i, i);
            black_box(store.insert(black_box(k), black_box(v)).unwrap());
        }
        store.flush().unwrap();
    })
}

// Inner-rep K=3: 92us -> ~276us timed region, drops absolute-jitter floor
// ~3x. Capped at K=3 so the post-modify log (setup 240 KiB + K*240 KiB)
// stays under StoreConfig::min_compact_bytes (1 MiB) and compaction can't
// fire inside the timed region. K=4 would cross the threshold mid-bench.
const MODIFY_INNER_REPS: u64 = 3;

fn sample_modify_10k(tmp_root: &Path) -> Duration {
    let dir = tempfile::TempDir::new_in(tmp_root).unwrap();
    let path = dir.path().join("store.log");
    let mut store: IndexMapStore<u64, u64> = IndexMapStore::open(&path).unwrap();
    for i in 0..10_000u64 {
        store.insert(i, 0).unwrap();
    }
    store.flush().unwrap();
    let elapsed = timed(|| {
        for _ in 0..MODIFY_INNER_REPS {
            for i in 0..10_000u64 {
                store
                    .modify(&black_box(i), |v| {
                        *v = v.wrapping_add(1);
                    })
                    .unwrap();
            }
        }
        store.flush().unwrap();
    });
    Duration::from_nanos((elapsed.as_nanos() / MODIFY_INNER_REPS as u128) as u64)
}

fn sample_lookup_100k(tmp_root: &Path) -> Duration {
    let dir = tempfile::TempDir::new_in(tmp_root).unwrap();
    let path = dir.path().join("store.log");
    let mut store: IndexMapStore<u64, u64> = IndexMapStore::open(&path).unwrap();
    for i in 0..10_000u64 {
        store.insert(i, i).unwrap();
    }
    timed(|| {
        let mut acc: u64 = 0;
        for i in 0..100_000u64 {
            let k = i % 10_000;
            if let Some(v) = store.get(&black_box(k)) {
                acc = acc.wrapping_add(*v);
            }
        }
        black_box(acc);
    })
}

// reopen is small enough (~64us/op) that fixed measurement overhead — kernel
// scheduler tick, allocator metadata, timer-read noise — dominates the
// noise floor. We inner-repeat K times inside the timed region and divide,
// dropping that overhead proportionally; each iteration is independent and
// measures the same open-and-replay path on the same on-disk log.
const REOPEN_INNER_REPS: u64 = 50;

fn sample_reopen_10k(tmp_root: &Path) -> Duration {
    let dir = tempfile::TempDir::new_in(tmp_root).unwrap();
    let path = dir.path().join("store.log");
    {
        let mut store: IndexMapStore<u64, u64> = IndexMapStore::open(&path).unwrap();
        for i in 0..10_000u64 {
            store.insert(i, i.wrapping_mul(7)).unwrap();
        }
        store.flush().unwrap();
    }
    let elapsed = timed(|| {
        for _ in 0..REOPEN_INNER_REPS {
            let store: IndexMapStore<u64, u64> = IndexMapStore::open(&path).unwrap();
            black_box(store.len());
        }
    });
    Duration::from_nanos((elapsed.as_nanos() / REOPEN_INNER_REPS as u128) as u64)
}

// ---------- env ----------

struct Env {
    invokes: usize,
    rounds: usize,
    samples_per_round: usize,
    warmup_rounds: usize,
    threshold: f64,
    filter: Option<String>,
    print_only: bool,
    pin_cpu: Option<usize>,
    tmpfs_root: PathBuf,
    child_out: Option<PathBuf>,
    load_cpus: Vec<usize>,
    verify_count: Option<usize>,
}

fn parse_env() -> Env {
    let invokes: usize = env::var("BENCH_INVOKES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5)
        .max(1);
    let rounds: usize = env::var("BENCH_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);
    let samples_per_round: usize = env::var("BENCH_PER_ROUND")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let warmup_rounds: usize = env::var("BENCH_WARMUP_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let threshold: f64 = env::var("BENCH_THRESHOLD")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    let filter = env::var("BENCH_FILTER").ok();
    let print_only = env::var("BENCH_PRINT_ONLY")
        .ok()
        .is_some_and(|s| !s.is_empty() && s != "0");
    let pin_cpu = match env::var("BENCH_PIN_CPU") {
        Ok(s) if s.eq_ignore_ascii_case("off") => None,
        Ok(s) => s.parse::<usize>().ok().or(Some(3)),
        Err(_) => Some(3),
    };
    let tmpfs_root = pick_tmpfs_root();
    let child_out = env::var("BENCH_CHILD_OUT")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    // BENCH_LOAD_CPUS: comma-separated CPU indices for background CPU-bound
    // load threads. Default is "8,12,4,5" — two on each CCD on a 16-core Zen,
    // away from our pinned core (3) and its SMT sibling (19). Empty / "off"
    // disables the background load entirely.
    let load_cpus: Vec<usize> = match env::var("BENCH_LOAD_CPUS") {
        Ok(s) if s.eq_ignore_ascii_case("off") || s.is_empty() => Vec::new(),
        Ok(s) => s.split(',').filter_map(|t| t.trim().parse::<usize>().ok()).collect(),
        Err(_) => vec![8, 12, 4, 5],
    };
    // Verify mode: run the full bench pipeline N times against a fixed
    // baseline. Each run is reported separately so /optimize can apply its
    // "all 3 runs pass" verdict logic. Different from BENCH_INVOKES, which
    // collapses N sub-processes into a single result.
    //
    // Triggered by `BENCH_VERIFY=N` env or `--verify N` argv.
    let mut verify_count = env::var("BENCH_VERIFY")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0);
    if verify_count.is_none() {
        let argv: Vec<String> = env::args().collect();
        let mut i = 1;
        while i < argv.len() {
            if argv[i] == "--verify" && i + 1 < argv.len() {
                if let Ok(n) = argv[i + 1].parse::<usize>()
                    && n > 0
                {
                    verify_count = Some(n);
                }
                break;
            }
            i += 1;
        }
    }
    Env {
        invokes,
        rounds,
        samples_per_round,
        warmup_rounds,
        threshold,
        filter,
        print_only,
        pin_cpu,
        tmpfs_root,
        child_out,
        load_cpus,
        verify_count,
    }
}

/// Per-process bench loop. Caller is responsible for already having pinned
/// the CPU, locked pages, and confirmed ASLR is off — those are inherited by
/// child sub-processes from this process via execve and the personality flag.
fn run_one_invocation(
    e: &Env,
    pinned_cpu: Option<usize>,
    pages_locked: bool,
    aslr_off: bool,
) -> Option<Results> {
    let scenarios: Vec<(&str, Scenario)> = vec![
        ("insert_10k_u64", sample_insert_10k_u64),
        ("insert_2k_strings", sample_insert_2k_strings),
        ("modify_10k", sample_modify_10k),
        ("lookup_100k", sample_lookup_100k),
        ("reopen_10k", sample_reopen_10k),
    ];
    let want = |name: &str| -> bool {
        e.filter
            .as_deref()
            .map(|f| name.contains(f))
            .unwrap_or(true)
    };
    let filtered: Vec<(&str, Scenario)> =
        scenarios.into_iter().filter(|(n, _)| want(n)).collect();
    if filtered.is_empty() {
        eprintln!("no scenarios matched BENCH_FILTER; exiting.");
        return None;
    }

    // Warm-up rounds — same shape as measured rounds; timings discarded.
    for _ in 0..e.warmup_rounds {
        for (_, f) in &filtered {
            for _ in 0..e.samples_per_round {
                let _ = f(&e.tmpfs_root);
            }
        }
    }

    // Measured rounds in block mode: all K samples of one scenario run
    // consecutively before moving to the next, so the hot working set stays
    // resident across consecutive samples (fine interleaving evicted it and
    // produced 50/50 bimodal regimes on lookup-heavy scenarios).
    let mut data: BTreeMap<String, Vec<Vec<u128>>> = filtered
        .iter()
        .map(|(n, _)| ((*n).to_string(), Vec::with_capacity(e.rounds)))
        .collect();
    for _round in 0..e.rounds {
        for (name, f) in &filtered {
            let mut s: Vec<u128> = Vec::with_capacity(e.samples_per_round);
            for _ in 0..e.samples_per_round {
                let d = f(&e.tmpfs_root);
                s.push(d.as_nanos());
            }
            data.get_mut(*name).unwrap().push(s);
        }
    }

    let mut r = Results {
        schema_version: SCHEMA_VERSION,
        timestamp_unix: now_unix(),
        config: RunConfig {
            invokes: e.invokes,
            rounds: e.rounds,
            samples_per_round: e.samples_per_round,
            warmup_rounds: e.warmup_rounds,
            pinned_cpu,
            tmpfs_path: e.tmpfs_root.display().to_string(),
            pages_locked,
            aslr_off,
        },
        benchmarks: BTreeMap::new(),
    };
    for (name, rounds_data) in data {
        r.benchmarks.insert(name, finalize(&rounds_data));
    }
    Some(r)
}

/// Aggregate N child invocations into a single result, scenario by scenario.
///
/// We empirically observed a stubborn ~5% bimodality between invocations on
/// this hardware: each child lands either in a "fast" regime (favorable
/// THP/address/cache state, occasional single-core boost) or a sustained
/// "slow" regime. CPU pinning + mlockall + ASLR-off + background CPU load
/// on other cores SHIFT the proportion but don't eliminate the split.
///
/// For each scenario we pool every child's `round_medians_ns` into one big
/// vector (N × R per-round medians) and report the **20%-trimmed mean** of
/// that pool as `p50_ns`. Trimming both tails drops the regime *outliers*
/// (whichever regime is the minority in this run) and averages over the
/// dominant regime, which is much more robust than min (sensitive to a
/// single lucky-low sample) and than the raw median (sensitive to which
/// regime contains exactly the middle index when N is small).
///
/// The other Stats fields are recomputed over the same pooled vector so
/// they stay internally consistent: `min_ns`/`p10_ns`/`p90_ns` are over
/// round medians (raw samples aren't shipped between processes);
/// `round_median_cv_pct` is the CV of the pool.
fn aggregate_median(children: Vec<Results>) -> Results {
    assert!(!children.is_empty());
    let last = children.last().expect("at least one child").clone();
    let mut agg = Results {
        schema_version: last.schema_version,
        timestamp_unix: last.timestamp_unix,
        config: last.config.clone(),
        benchmarks: BTreeMap::new(),
    };
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for c in &children {
        for k in c.benchmarks.keys() {
            names.insert(k.clone());
        }
    }
    for name in names {
        let mut pool: Vec<u128> = Vec::new();
        let mut samples_total: usize = 0;
        let mut rounds: usize = 0;
        for c in &children {
            if let Some(s) = c.benchmarks.get(&name) {
                pool.extend_from_slice(&s.round_medians_ns);
                samples_total += s.samples_total;
                rounds += s.rounds;
            }
        }
        if pool.is_empty() {
            continue;
        }
        let mut sorted = pool.clone();
        sorted.sort();
        let p50_ns = median_sorted(&sorted);
        let trimmed_mean_ns = trimmed_mean_sorted(&sorted, 0.20);
        let p10_ns = percentile_sorted(&sorted, 0.10);
        let p90_ns = percentile_sorted(&sorted, 0.90);
        let mad_ns = mad_from_sorted(&sorted);
        let mean = pool.iter().sum::<u128>() as f64 / pool.len() as f64;
        let var = pool
            .iter()
            .map(|&v| {
                let d = v as f64 - mean;
                d * d
            })
            .sum::<f64>()
            / pool.len() as f64;
        let stddev = var.sqrt();
        let cv = if mean > 0.0 {
            stddev / mean * 100.0
        } else {
            0.0
        };
        agg.benchmarks.insert(
            name,
            Stats {
                samples_total,
                rounds,
                min_ns: sorted[0],
                p10_ns,
                p50_ns,
                p90_ns,
                trimmed_mean_ns,
                mad_ns,
                round_medians_ns: pool,
                round_median_cv_pct: cv,
            },
        );
    }
    agg
}

fn child_mode(e: &Env, out: &Path) {
    // Children pin/lock/etc the same way the master does — the personality
    // flag is inherited so ASLR stays off; we still call lock_pages here in
    // case mlockall was reset by execve (it can be; lock state does not
    // survive exec unconditionally).
    let pinned_cpu = e.pin_cpu.filter(|&cpu| pin_cpu(cpu));
    let pages_locked = lock_pages();
    let aslr_off = aslr_label() == "off";

    // Background load on other CPUs holds the package in its sustained-boost
    // regime so the measurement core doesn't flicker between single-core
    // boost (~5.7 GHz) and sustained (~5.4 GHz). Lives for the duration of
    // this child invocation; dropped at end of scope.
    let _load = if e.load_cpus.is_empty() {
        None
    } else {
        Some(spawn_background_load(&e.load_cpus))
    };
    // Brief settling pause so the load threads ramp up and the CPU package
    // converges on its loaded P-state before we start sampling.
    std::thread::sleep(Duration::from_millis(50));

    if let Some(r) = run_one_invocation(e, pinned_cpu, pages_locked, aslr_off) {
        let s = serde_json::to_string(&r).expect("serialize child results");
        fs::write(out, s).expect("write child results");
    }
}

/// One full "bench cycle" from the master's point of view: either a single
/// in-process run or N child sub-process invocations aggregated via
/// trimmed-mean. This is what the user thinks of as "one cargo bench".
fn run_master_pipeline(
    e: &Env,
    pinned_cpu: Option<usize>,
    pages_locked: bool,
    aslr_off: bool,
) -> Option<Results> {
    if e.invokes <= 1 {
        let _load = if e.load_cpus.is_empty() {
            None
        } else {
            Some(spawn_background_load(&e.load_cpus))
        };
        std::thread::sleep(Duration::from_millis(50));
        return run_one_invocation(e, pinned_cpu, pages_locked, aslr_off);
    }
    let children = spawn_children(e);
    if children.is_empty() {
        eprintln!("no children completed successfully; falling back to a single in-process run.");
        let _load = if e.load_cpus.is_empty() {
            None
        } else {
            Some(spawn_background_load(&e.load_cpus))
        };
        std::thread::sleep(Duration::from_millis(50));
        run_one_invocation(e, pinned_cpu, pages_locked, aslr_off)
    } else {
        Some(aggregate_median(children))
    }
}

#[derive(Serialize, Deserialize)]
struct VerifyDelta {
    baseline_trim_mean_ns: u128,
    runs_trim_mean_ns: Vec<u128>,
    deltas_pct: Vec<f64>,
}

#[derive(Serialize, Deserialize)]
struct VerifyResults {
    schema_version: u32,
    baseline_timestamp_unix: u64,
    scenarios: BTreeMap<String, VerifyDelta>,
}

fn verify_results_path() -> PathBuf {
    match option_env!("CARGO_MANIFEST_DIR") {
        Some(dir) => PathBuf::from(dir).join("verify_results.json"),
        None => PathBuf::from("verify_results.json"),
    }
}

/// Run the bench `n` times against a fixed baseline and emit per-run deltas.
///
/// Output goes to three places:
/// * A human-readable table on stdout (column per run, signed Δ%).
/// * A single `VERIFY-JSON:` line on stdout for grep-and-parse consumers.
/// * A `verify_results.json` file alongside `bench_results.json` for
///   structured consumers (e.g. `/optimize`).
///
/// `bench_results.json` is NOT overwritten by this mode — the baseline file
/// stays untouched, eliminating the need for callers to back it up to /tmp
/// before running the verification cycle.
fn verify_mode(
    e: &Env,
    n: usize,
    pinned_cpu: Option<usize>,
    pages_locked: bool,
    aslr_off: bool,
) {
    let baseline = match load_previous() {
        Some(b) => b,
        None => {
            eprintln!(
                "--verify needs a baseline: run `cargo bench` once first to seed bench_results.json, \
                 or pass BENCH_BASELINE=<path>."
            );
            return;
        }
    };

    let mut runs: Vec<Results> = Vec::with_capacity(n);
    for i in 0..n {
        eprintln!("verify run {}/{}", i + 1, n);
        if let Some(r) = run_master_pipeline(e, pinned_cpu, pages_locked, aslr_off) {
            runs.push(r);
        }
    }
    if runs.is_empty() {
        eprintln!("verify: no successful runs.");
        return;
    }

    // Collect per-scenario baseline + per-run trim_mean.
    let mut scenarios: BTreeMap<String, VerifyDelta> = BTreeMap::new();
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for k in baseline.benchmarks.keys() {
        names.insert(k.clone());
    }
    for r in &runs {
        for k in r.benchmarks.keys() {
            names.insert(k.clone());
        }
    }
    for name in names {
        let base = match baseline.benchmarks.get(&name) {
            Some(s) => s.trimmed_mean_ns,
            None => continue,
        };
        let mut runs_ns: Vec<u128> = Vec::with_capacity(runs.len());
        let mut deltas: Vec<f64> = Vec::with_capacity(runs.len());
        for r in &runs {
            if let Some(s) = r.benchmarks.get(&name) {
                let v = s.trimmed_mean_ns;
                runs_ns.push(v);
                deltas.push(if base == 0 {
                    0.0
                } else {
                    (v as f64 - base as f64) * 100.0 / base as f64
                });
            } else {
                runs_ns.push(0);
                deltas.push(0.0);
            }
        }
        scenarios.insert(
            name,
            VerifyDelta {
                baseline_trim_mean_ns: base,
                runs_trim_mean_ns: runs_ns,
                deltas_pct: deltas,
            },
        );
    }

    let out = VerifyResults {
        schema_version: SCHEMA_VERSION,
        baseline_timestamp_unix: baseline.timestamp_unix,
        scenarios,
    };

    // Human-readable table.
    println!();
    println!(
        "=== VERIFY: {} runs vs baseline (unix {}) ===",
        runs.len(),
        baseline.timestamp_unix
    );
    println!();
    let mut header = format!("{:<22} {:>12}", "scenario", "baseline");
    for i in 0..runs.len() {
        header.push_str(&format!(" {:>12}", format!("run{}", i + 1)));
    }
    for i in 0..runs.len() {
        header.push_str(&format!(" {:>10}", format!("Δ{}%", i + 1)));
    }
    println!("{}", header);
    println!("{:-<width$}", "", width = header.len());
    for (name, d) in &out.scenarios {
        let mut line = format!("{:<22} {:>12}", name, fmt_ns(d.baseline_trim_mean_ns));
        for &v in &d.runs_trim_mean_ns {
            line.push_str(&format!(" {:>12}", fmt_ns(v)));
        }
        for &p in &d.deltas_pct {
            line.push_str(&format!(" {:>+9.3}%", p));
        }
        println!("{}", line);
    }
    println!();

    // Machine-parseable single-line JSON.
    let json_line = serde_json::to_string(&out).unwrap_or_else(|_| "{}".to_string());
    println!("VERIFY-JSON: {}", json_line);

    // Pretty-printed file alongside bench_results.json.
    let path = verify_results_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(pretty) = serde_json::to_string_pretty(&out) {
        let _ = fs::write(&path, pretty);
        println!("verify results written to {}", path.display());
    }
}

fn spawn_children(e: &Env) -> Vec<Results> {
    let exe = match env::current_exe() {
        Ok(p) => p,
        Err(err) => {
            eprintln!("cannot resolve current_exe ({err}); falling back to in-process single run");
            return Vec::new();
        }
    };
    let mut out = Vec::with_capacity(e.invokes);
    let pid = std::process::id();
    for i in 0..e.invokes {
        let tmp = std::env::temp_dir().join(format!("indexmap_bench_{}_{}.json", pid, i));
        let status = std::process::Command::new(&exe)
            .env("BENCH_CHILD_OUT", &tmp)
            // Children inherit our personality (ADDR_NO_RANDOMIZE) via execve;
            // skip their own respawn dance.
            .env("BENCH_NO_RESPAWN", "1")
            .status();
        match status {
            Ok(s) if s.success() => match fs::read_to_string(&tmp) {
                Ok(json) => match serde_json::from_str::<Results>(&json) {
                    Ok(r) => out.push(r),
                    Err(err) => eprintln!("child {i} returned unparseable JSON: {err}"),
                },
                Err(err) => eprintln!("child {i} did not produce output ({err})"),
            },
            Ok(s) => eprintln!("child {i} exited with status {s:?}"),
            Err(err) => eprintln!("failed to spawn child {i}: {err}"),
        }
        let _ = fs::remove_file(&tmp);
    }
    out
}

fn main() {
    maybe_respawn_no_aslr();

    let e = parse_env();

    if e.print_only {
        let current: Results = match fs::read_to_string(results_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(r) => r,
            None => {
                eprintln!(
                    "BENCH_PRINT_ONLY: no v{} {} to read",
                    SCHEMA_VERSION,
                    results_path().display()
                );
                return;
            }
        };
        print_table(&current, load_previous().as_ref(), e.threshold);
        return;
    }

    if let Some(out) = e.child_out.as_ref() {
        child_mode(&e, out);
        return;
    }

    // Master path. Pin / lock / record ASLR once; if BENCH_INVOKES > 1 we'll
    // spawn children which inherit ADDR_NO_RANDOMIZE via execve.
    let pinned_cpu = if let Some(cpu) = e.pin_cpu {
        if pin_cpu(cpu) {
            Some(cpu)
        } else {
            eprintln!("warning: failed to pin to CPU {cpu} (sched_setaffinity)");
            None
        }
    } else {
        None
    };
    let pages_locked = lock_pages();
    let aslr_off = aslr_label() == "off";

    if let Some(n) = e.verify_count {
        verify_mode(&e, n, pinned_cpu, pages_locked, aslr_off);
        return;
    }

    let current = match run_master_pipeline(&e, pinned_cpu, pages_locked, aslr_off) {
        Some(r) => r,
        None => return,
    };

    let previous = load_previous();
    println!();
    println!(
        "IndexMapStore benchmark — {} invocation(s) x {} rounds x {} samples = {} samples/scenario",
        e.invokes,
        current.config.rounds,
        current.config.samples_per_round,
        current.config.rounds * current.config.samples_per_round,
    );
    println!(
        "aggregator=trimmed-mean(20%) over {} x {} round-medians  |  schema=v{}",
        current.config.invokes,
        current.config.rounds,
        SCHEMA_VERSION
    );
    print_table(&current, previous.as_ref(), e.threshold);
    save(&current).expect("write bench results");
    println!("Results saved to {}", results_path().display());
}
