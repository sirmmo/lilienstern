//! What the server is doing, for its stats page (`/` and `/stats.json`): counters kept as things
//! happen, a sample of the process every ten seconds for the last hour, and the rooms on disk.
//! Only totals: no room codes (a code is enough to join a game).

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// A counter, since the server started.
pub struct Count(AtomicU64);

impl Count {
    const fn new() -> Self {
        Count(AtomicU64::new(0))
    }
    pub fn add(&self, n: u64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

pub static CONNECTIONS: Count = Count::new();
pub static REFUSED: Count = Count::new();
pub static FLOODS: Count = Count::new();
pub static MESSAGES_IN: Count = Count::new();
pub static BYTES_IN: Count = Count::new();
pub static MESSAGES_OUT: Count = Count::new();
pub static BYTES_OUT: Count = Count::new();
pub static ERRORS: Count = Count::new();
pub static ROOMS_OPENED: Count = Count::new();
pub static DAYS: Count = Count::new();
pub static PAGE_VIEWS: Count = Count::new();

/// Samples kept: one every `EVERY` seconds for an hour.
pub const EVERY: u64 = 10;
const KEEP: usize = 360;

#[derive(Clone, Copy, Default)]
pub struct Sample {
    /// Unix seconds.
    pub at: u64,
    /// Percent of one core, over the last interval.
    pub cpu: f64,
    /// Resident memory, bytes.
    pub rss: u64,
    pub connections: usize,
    pub rooms: usize,
    pub games: usize,
}

/// The rooms on disk, counted now and then (it reads every room's settings).
#[derive(Clone, Default)]
pub struct Disk {
    pub rooms: usize,
    pub lobby: usize,
    pub under_way: usize,
    pub over: usize,
    pub bytes: u64,
    pub by_game: BTreeMap<String, usize>,
}

pub struct Monitor {
    pub started: Instant,
    pub started_at: u64,
    last: Option<(Instant, u64)>,
    pub history: VecDeque<Sample>,
    pub disk: Disk,
    pub peak_connections: usize,
}

impl Monitor {
    pub fn new() -> Self {
        Monitor {
            started: Instant::now(),
            started_at: unix_now(),
            last: None,
            history: VecDeque::new(),
            disk: Disk::default(),
            peak_connections: 0,
        }
    }

    /// Takes a sample of the process (the rooms' numbers come from the caller).
    pub fn sample(&mut self, connections: usize, rooms: usize, games: usize) {
        let now = Instant::now();
        let ticks = cpu_ticks().unwrap_or(0);
        let cpu = match self.last {
            Some((t, before)) => {
                let secs = now.duration_since(t).as_secs_f64();
                if secs > 0.0 {
                    ticks.saturating_sub(before) as f64 / CLOCK_TICKS / secs * 100.0
                } else {
                    0.0
                }
            }
            None => 0.0,
        };
        self.last = Some((now, ticks));
        self.peak_connections = self.peak_connections.max(connections);
        if self.history.len() == KEEP {
            self.history.pop_front();
        }
        self.history.push_back(Sample {
            at: unix_now(),
            cpu,
            rss: status_kb("VmRSS").unwrap_or(0) * 1024,
            connections,
            rooms,
            games,
        });
    }
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Linux counts CPU time in ticks of 1/100 s (USER_HZ is 100 on every common platform).
const CLOCK_TICKS: f64 = 100.0;

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// User + system CPU time of this process, in ticks.
fn cpu_ticks() -> Option<u64> {
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    // The name, in parentheses, may hold spaces: the fields we want come after it.
    let rest = &stat[stat.rfind(')')? + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    // utime and stime are fields 14 and 15 of the line, 12 and 13 after the name.
    Some(f.get(11)?.parse::<u64>().ok()? + f.get(12)?.parse::<u64>().ok()?)
}

/// A line of /proc/self/status, in kB (or a plain number, like Threads).
fn status_kb(key: &str) -> Option<u64> {
    let s = fs::read_to_string("/proc/self/status").ok()?;
    s.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn read_u64(path: &str) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// The container's memory: (used, limit), from cgroup v2 or v1; nothing outside a container
/// with a limit (there the group can be the whole machine's).
fn container_memory() -> (Option<u64>, Option<u64>) {
    let (used, limit) = match read_u64("/sys/fs/cgroup/memory.current") {
        Some(used) => (Some(used), read_u64("/sys/fs/cgroup/memory.max")),
        None => (
            read_u64("/sys/fs/cgroup/memory/memory.usage_in_bytes"),
            read_u64("/sys/fs/cgroup/memory/memory.limit_in_bytes"),
        ),
    };
    // No limit: v2 says "max" (not a number), v1 a huge number.
    match limit.filter(|&l| l < 1 << 60) {
        Some(l) => (used, Some(l)),
        None => (None, None),
    }
}

/// The machine's memory, from /proc/meminfo: (total, available), bytes.
fn host_memory() -> (Option<u64>, Option<u64>) {
    let Ok(s) = fs::read_to_string("/proc/meminfo") else {
        return (None, None);
    };
    let get = |k: &str| {
        s.lines()
            .find_map(|l| l.strip_prefix(k)?.strip_prefix(':'))
            .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
            .map(|kb| kb * 1024)
    };
    (get("MemTotal"), get("MemAvailable"))
}

fn load_average() -> Option<Vec<f64>> {
    let s = fs::read_to_string("/proc/loadavg").ok()?;
    s.split_whitespace()
        .take(3)
        .map(|x| x.parse().ok())
        .collect()
}

/// Counts the rooms in the data directory.
pub fn scan(dir: &Path) -> Disk {
    let mut d = Disk::default();
    let Ok(entries) = fs::read_dir(dir) else {
        return d;
    };
    for e in entries.flatten() {
        let path = e.path();
        d.bytes += e.metadata().map_or(0, |m| m.len());
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Some(m) = fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            continue;
        };
        d.rooms += 1;
        match (m["started"].as_bool(), m["over"].as_bool()) {
            (_, Some(true)) => d.over += 1,
            (Some(true), _) => d.under_way += 1,
            _ => d.lobby += 1,
        }
        let game = m["game"].as_str().unwrap_or("?").to_string();
        *d.by_game.entry(game).or_default() += 1;
    }
    d
}

/// Everything for the page; `rooms` describes the rooms in memory.
pub fn report(m: &Monitor, rooms: Value, data: Option<&Path>) -> Value {
    let (used, limit) = container_memory();
    let (host_total, host_available) = host_memory();
    let last = m.history.back().copied().unwrap_or_default();
    let h = &m.history;
    let series = |f: &dyn Fn(&Sample) -> Value| h.iter().map(f).collect::<Vec<_>>();
    json!({
        "version": std::env::var("LIL_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").into()),
        "now": unix_now(),
        "started_at": m.started_at,
        "uptime": m.started.elapsed().as_secs(),
        "process": {
            "cpu": last.cpu,
            "rss": status_kb("VmRSS").map(|k| k * 1024),
            "peak_rss": status_kb("VmHWM").map(|k| k * 1024),
            "threads": status_kb("Threads"),
        },
        "container": { "memory": used, "limit": limit },
        "host": {
            "cpus": std::thread::available_parallelism().map_or(0, |n| n.get()),
            "load": load_average(),
            "memory": host_total,
            "available": host_available,
        },
        "rooms": rooms,
        "disk": {
            "kept": data.is_some(),
            "rooms": m.disk.rooms,
            "lobby": m.disk.lobby,
            "under_way": m.disk.under_way,
            "over": m.disk.over,
            "bytes": m.disk.bytes,
            "by_game": m.disk.by_game,
            "keep_days": crate::store::KEEP_DAYS,
        },
        "traffic": {
            "connections": CONNECTIONS.get(),
            "peak_connections": m.peak_connections,
            "refused": REFUSED.get(),
            "floods": FLOODS.get(),
            "messages_in": MESSAGES_IN.get(),
            "bytes_in": BYTES_IN.get(),
            "messages_out": MESSAGES_OUT.get(),
            "bytes_out": BYTES_OUT.get(),
            "errors": ERRORS.get(),
            "rooms_opened": ROOMS_OPENED.get(),
            "days": DAYS.get(),
            "page_views": PAGE_VIEWS.get(),
        },
        "history": {
            "every": EVERY,
            "at": series(&|s| json!(s.at)),
            "cpu": series(&|s| json!((s.cpu * 10.0).round() / 10.0)),
            "rss": series(&|s| json!(s.rss)),
            "connections": series(&|s| json!(s.connections)),
            "rooms": series(&|s| json!(s.rooms)),
            "games": series(&|s| json!(s.games)),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_process() {
        assert!(cpu_ticks().is_some(), "/proc/self/stat");
        assert!(status_kb("VmRSS").unwrap() > 0);
        let mut m = Monitor::new();
        m.sample(2, 1, 0);
        m.sample(3, 1, 1);
        assert_eq!((m.history.len(), m.peak_connections), (2, 3));
        let r = report(&m, json!({}), None);
        assert_eq!(r["history"]["connections"], json!([2, 3]));
        assert!(r["process"]["rss"].as_u64().unwrap() > 0);
    }

    #[test]
    fn counts_the_rooms_on_disk() {
        let dir = std::env::temp_dir().join(format!("lil-stats-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let room = |code: &str, game: &str, started: bool, over: bool| {
            let m = json!({ "game": game, "started": started, "over": over });
            fs::write(dir.join(format!("{code}.json")), m.to_string()).unwrap();
        };
        room("AAAAA", "italia", true, false);
        room("BBBBB", "italia", false, false);
        room("CCCCC", "marches", true, true);
        fs::write(dir.join("AAAAA.log"), "{\"op\":\"step\"}\n").unwrap();
        let d = scan(&dir);
        assert_eq!((d.rooms, d.lobby, d.under_way, d.over), (3, 1, 1, 1));
        assert_eq!(d.by_game["italia"], 2);
        assert!(d.bytes > 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
