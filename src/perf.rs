//! Server health: `/perf` boss bar (TPS, MSPT, RAM, players, mobs), `/perf parts` (time spent
//! in each part of the server), and the debugging that says why a server died or lagged:
//! - crash reports in crash-reports/ for every panic, with a backtrace (a panic inside one
//!   loop only stops that loop, so without this mobs or liquids could just freeze silently)
//! - a watchdog that logs to logs/watchdog.log when a loop stops beating for 15 s
//! - logs/lag.log: any single part that takes over 50 ms

use crate::players::Server;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Last heartbeat of each server loop.
static BEATS: Mutex<Option<std::collections::HashMap<&'static str, Instant>>> = Mutex::new(None);

/// Time spent per part over the current second: (total, runs, slowest).
static PARTS: Mutex<Option<std::collections::HashMap<&'static str, (Duration, u32, Duration)>>> = Mutex::new(None);
/// The last full second of PARTS, for /perf parts and crash reports.
static LAST_PARTS: Mutex<Vec<(&'static str, Duration, u32, Duration)>> = Mutex::new(Vec::new());

/// A loop says it's alive.
pub fn beat(name: &'static str) {
    BEATS.lock().get_or_insert_with(Default::default).insert(name, Instant::now());
}

/// Runs `f` as part `name`: timed for /perf parts, logged to logs/lag.log if it's slow.
pub fn time<T>(name: &'static str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    let took = start.elapsed();
    {
        let mut parts = PARTS.lock();
        let e = parts.get_or_insert_with(Default::default).entry(name).or_insert((Duration::ZERO, 0, Duration::ZERO));
        e.0 += took;
        e.1 += 1;
        e.2 = e.2.max(took);
    }
    if took > Duration::from_millis(50) {
        log("lag.log", &format!("{name} took {} ms", took.as_millis()));
    }
    out
}

/// Appends a timestamped line to logs/<file>.
fn log(file: &str, line: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all("logs");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::path::Path::new("logs").join(file)) {
        let _ = writeln!(f, "[{}] {line}", stamp());
    }
}

/// Seconds since 1970, as text without a date library.
fn stamp() -> String {
    date(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()))
}

fn date(s: u64) -> String {
    let (days, rem) = (s / 86400, s % 86400);
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem / 60 % 60, rem % 60)
}

/// The last second's parts, slowest first, as text.
pub fn parts_text() -> String {
    let parts = LAST_PARTS.lock();
    if parts.is_empty() {
        return "No timings yet.".into();
    }
    let mut out = String::from("§6Server parts, last second (total ms / runs / slowest ms):");
    for (name, total, runs, max) in parts.iter() {
        out.push_str(&format!("\n§f{name}: §e{:.1} §7/ {runs} / {:.1}", total.as_secs_f32() * 1000.0, max.as_secs_f32() * 1000.0));
    }
    out
}

/// Text without § colour codes, for files.
fn strip_colours(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// Crash reports for every panic, and the watchdog. Call once at startup.
pub fn install() {
    let started = Instant::now();
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let location = info.location().map_or("unknown".into(), |l| format!("{}:{}", l.file(), l.line()));
        let message = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        let report = format!(
            "Rusted {} crash report\n{}\n\nPanic: {message}\nAt: {location}\nThread: {thread}\nUptime: {} s\nMemory: {} MB\n\n{}\n\nBacktrace:\n{}\n",
            env!("CARGO_PKG_VERSION"),
            stamp(),
            started.elapsed().as_secs(),
            memory_mb().map_or("?".into(), |m| format!("{m:.0}")),
            strip_colours(&parts_text()),
            std::backtrace::Backtrace::force_capture()
        );
        let _ = std::fs::create_dir_all("crash-reports");
        let file = std::path::Path::new("crash-reports").join(format!("crash-{}.txt", stamp().replace([' ', ':'], "-")));
        let _ = std::fs::write(&file, &report);
        eprintln!("A part of the server crashed: {message} at {location}. Report: {}", file.display());
        default(info);
    }));
    std::thread::Builder::new()
        .name("watchdog".into())
        .spawn(|| {
            let mut warned: HashSet<&'static str> = HashSet::new();
            loop {
                std::thread::sleep(Duration::from_secs(5));
                let beats: Vec<(&'static str, Duration)> = BEATS.lock().as_ref().map(|b| b.iter().map(|(k, v)| (*k, v.elapsed())).collect()).unwrap_or_default();
                for (name, quiet) in beats {
                    if quiet > Duration::from_secs(15) {
                        if warned.insert(name) {
                            let line = format!("the {name} loop hasn't run for {} s (stuck or crashed; see crash-reports/)", quiet.as_secs());
                            eprintln!("Watchdog: {line}");
                            log("watchdog.log", &line);
                        }
                    } else if warned.remove(name) {
                        log("watchdog.log", &format!("the {name} loop is running again"));
                    }
                }
            }
        })
        .ok();
}

/// Players with the bar on.
static WATCHERS: Mutex<Option<HashSet<i64>>> = Mutex::new(None);

struct Window {
    since: Instant,
    ticks: u32,
    work: Duration,
}

static WINDOW: Mutex<Option<Window>> = Mutex::new(None);

/// Turns the bar on or off for `id`. Returns whether it's now on.
pub fn toggle(server: &Server, id: i64) -> bool {
    let mut w = WATCHERS.lock();
    let w = w.get_or_insert_with(HashSet::new);
    if w.remove(&id) {
        crate::ui::remove_boss_bar(server, id);
        false
    } else {
        w.insert(id);
        true
    }
}

/// The last minute of tick times (1200 ticks), for percentiles.
static RECENT: Mutex<std::collections::VecDeque<Duration>> = Mutex::new(std::collections::VecDeque::new());

/// (p50, p95, p99, max) tick work over the last minute, in ms.
pub fn tick_percentiles() -> (f32, f32, f32, f32) {
    let mut v: Vec<Duration> = RECENT.lock().iter().copied().collect();
    if v.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    v.sort_unstable();
    let at = |q: f32| v[((v.len() - 1) as f32 * q).round() as usize].as_secs_f32() * 1000.0;
    (at(0.5), at(0.95), at(0.99), at(1.0))
}

/// Called after every game tick with how long its work took.
pub fn tick(server: &Server, work: Duration) {
    {
        let mut r = RECENT.lock();
        if r.len() >= 1200 {
            r.pop_front();
        }
        r.push_back(work);
    }
    let mut win = WINDOW.lock();
    let win = win.get_or_insert_with(|| Window { since: Instant::now(), ticks: 0, work: Duration::ZERO });
    win.ticks += 1;
    win.work += work;
    let elapsed = win.since.elapsed();
    if elapsed < Duration::from_secs(1) {
        return;
    }
    let tps = (win.ticks as f32 / elapsed.as_secs_f32()).min(20.0);
    // Roll the part timings over to "last second".
    {
        let mut list: Vec<(&'static str, Duration, u32, Duration)> = PARTS.lock().take().unwrap_or_default().into_iter().map(|(k, (t, n, m))| (k, t, n, m)).collect();
        list.sort_by(|a, b| b.1.cmp(&a.1));
        *LAST_PARTS.lock() = list;
    }
    let mspt = win.work.as_secs_f32() * 1000.0 / win.ticks.max(1) as f32;
    *win = Window { since: Instant::now(), ticks: 0, work: Duration::ZERO };
    // Once a minute, a line in logs/perf.log to look back at.
    static SECONDS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if SECONDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 60 == 0 {
        let top: Vec<String> = LAST_PARTS.lock().iter().take(5).map(|(n, t, _, m)| format!("{n} {:.1}ms (max {:.1})", t.as_secs_f32() * 1000.0, m.as_secs_f32() * 1000.0)).collect();
        let (p50, p95, p99, max) = tick_percentiles();
        log("perf.log", &format!("TPS {tps:.1} MSPT {mspt:.2} p50 {p50:.2} p95 {p95:.2} p99 {p99:.2} max {max:.2} RAM {} MB players {} mobs {} | {}", memory_mb().map_or("?".into(), |m| format!("{m:.0}")), server.all().len(), server.mobs.count(), top.join(", ")));
    }
    let ids: Vec<i64> = WATCHERS.lock().as_ref().map(|w| w.iter().copied().collect()).unwrap_or_default();
    if ids.is_empty() {
        return;
    }
    let colour = if tps >= 19.0 { "§a" } else if tps >= 15.0 { "§e" } else { "§c" };
    let ram = memory_mb().map_or("?".into(), |m| format!("{m:.0} MB"));
    let text = format!(
        "{colour}TPS {tps:.1} §7| §fMSPT {mspt:.1} §7| §bRAM {ram} §7| §fPlayers {} §7| §dMobs {}",
        server.all().len(),
        server.mobs.count()
    );
    // Boss bar colours: 2 red, 3 green, 4 yellow.
    let bar = if tps >= 19.0 { 3 } else if tps >= 15.0 { 4 } else { 2 };
    for id in ids {
        if server.get(id).is_none() {
            if let Some(w) = WATCHERS.lock().as_mut() {
                w.remove(&id);
            }
            continue;
        }
        crate::ui::boss_bar(server, id, &text, tps / 20.0, bar);
    }
}

/// This process's memory in use (resident / working set), in MB.
#[cfg(windows)]
pub fn memory_mb() -> Option<f64> {
    #[repr(C)]
    struct Counters {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        peak_paged_pool: usize,
        paged_pool: usize,
        peak_nonpaged_pool: usize,
        nonpaged_pool: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, page_faults: 0, peak_working_set: 0, working_set: 0, peak_paged_pool: 0, paged_pool: 0, peak_nonpaged_pool: 0, nonpaged_pool: 0, pagefile: 0, peak_pagefile: 0 };
    // SAFETY: c is a correctly sized PROCESS_MEMORY_COUNTERS and cb says so.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (ok != 0).then(|| c.working_set as f64 / 1_048_576.0)
}

#[cfg(not(windows))]
pub fn memory_mb() -> Option<f64> {
    // statm: size resident ... in pages.
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: f64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096.0 / 1_048_576.0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn percentiles_of_recent_ticks() {
        super::RECENT.lock().clear();
        for ms in 1..=100 {
            super::RECENT.lock().push_back(std::time::Duration::from_millis(ms));
        }
        let (p50, p95, p99, max) = super::tick_percentiles();
        assert!((p50 - 51.0).abs() < 1.5 && (p95 - 95.0).abs() < 1.5 && (p99 - 99.0).abs() < 1.5 && max == 100.0, "{p50} {p95} {p99} {max}");
    }

    #[test]
    fn dates() {
        assert_eq!(super::date(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(super::date(1_700_000_000), "2023-11-14 22:13:20 UTC");
        assert_eq!(super::date(951_782_400), "2000-02-29 00:00:00 UTC", "leap day");
    }

    #[test]
    fn reads_memory() {
        let mb = super::memory_mb().expect("memory reading");
        assert!(mb > 1.0 && mb < 100_000.0, "{mb} MB");
    }
}
