use crate::util::json_string;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct HostMetrics {
    pub hostname: String,
    pub cpu_percent: f64,
    pub cpu_threads: usize,
    pub load_1: f64,
    pub load_5: f64,
    pub load_15: f64,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disk_total: u64,
    pub disk_used: u64,
    pub disk_available: u64,
    pub disk_percent: f64,
    pub uptime_seconds: u64,
    pub process_rss: u64,
}

impl HostMetrics {
    pub fn collect() -> Result<Self, String> {
        let first = read_cpu_sample()?;
        thread::sleep(Duration::from_millis(150));
        let second = read_cpu_sample()?;

        let total_delta = second.total.saturating_sub(first.total);
        let idle_delta = second.idle.saturating_sub(first.idle);
        let cpu_percent = if total_delta == 0 {
            0.0
        } else {
            100.0 * (total_delta.saturating_sub(idle_delta)) as f64 / total_delta as f64
        };

        let memory = read_meminfo()?;
        let memory_total = memory.get("MemTotal").copied().unwrap_or_default() * 1024;
        // `MemAvailable` es la RAM que una aplicación puede usar de verdad:
        // MemFree más la caché que Linux puede liberar bajo demanda. Derivar
        // `used` a partir de ella garantiza que `used + available == total`.
        let memory_available = memory
            .get("MemAvailable")
            .copied()
            .unwrap_or_else(|| {
                // Núcleos antiguos sin MemAvailable: aproximarla como
                // free + buffers + caché reclaimable − memoria compartida.
                memory.get("MemFree").copied().unwrap_or_default()
                    .saturating_add(memory.get("Buffers").copied().unwrap_or_default())
                    .saturating_add(memory.get("Cached").copied().unwrap_or_default())
                    .saturating_add(memory.get("SReclaimable").copied().unwrap_or_default())
                    .saturating_sub(memory.get("Shmem").copied().unwrap_or_default())
            })
            .min(memory_total / 1024)
            * 1024;
        let memory_used = memory_total.saturating_sub(memory_available);
        let swap_total = memory.get("SwapTotal").copied().unwrap_or_default() * 1024;
        let swap_free = memory.get("SwapFree").copied().unwrap_or_default() * 1024;
        let swap_used = swap_total.saturating_sub(swap_free);

        let (load_1, load_5, load_15) = read_load_average()?;
        let (disk_total, disk_used, disk_available, disk_percent) = read_disk()?;

        Ok(Self {
            hostname: read_hostname(),
            cpu_percent,
            cpu_threads: read_cpu_threads(),
            load_1,
            load_5,
            load_15,
            memory_total,
            memory_used,
            memory_available,
            swap_total,
            swap_used,
            disk_total,
            disk_used,
            disk_available,
            disk_percent,
            uptime_seconds: read_uptime(),
            process_rss: read_process_rss(),
        })
    }

    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"hostname\":{},",
                "\"cpu_percent\":{:.2},",
                "\"cpu_threads\":{},",
                "\"load_1\":{:.2},",
                "\"load_5\":{:.2},",
                "\"load_15\":{:.2},",
                "\"memory_total\":{},",
                "\"memory_used\":{},",
                "\"memory_available\":{},",
                "\"swap_total\":{},",
                "\"swap_used\":{},",
                "\"disk_total\":{},",
                "\"disk_used\":{},",
                "\"disk_available\":{},",
                "\"disk_percent\":{:.2},",
                "\"uptime_seconds\":{},",
                "\"process_rss\":{}",
                "}}"
            ),
            json_string(&self.hostname),
            self.cpu_percent,
            self.cpu_threads,
            self.load_1,
            self.load_5,
            self.load_15,
            self.memory_total,
            self.memory_used,
            self.memory_available,
            self.swap_total,
            self.swap_used,
            self.disk_total,
            self.disk_used,
            self.disk_available,
            self.disk_percent,
            self.uptime_seconds,
            self.process_rss,
        )
    }
}

#[derive(Clone, Copy)]
struct CpuSample {
    idle: u64,
    total: u64,
}

fn read_cpu_sample() -> Result<CpuSample, String> {
    let contents = fs::read_to_string("/proc/stat")
        .map_err(|error| format!("no se pudo leer /proc/stat: {error}"))?;
    let line = contents
        .lines()
        .next()
        .ok_or_else(|| "/proc/stat está vacío".to_owned())?;
    let mut values = line
        .split_whitespace()
        .skip(1)
        .map(|value| value.parse::<u64>().unwrap_or_default());

    let user = values.next().unwrap_or_default();
    let nice = values.next().unwrap_or_default();
    let system = values.next().unwrap_or_default();
    let idle = values.next().unwrap_or_default();
    let io_wait = values.next().unwrap_or_default();
    let irq = values.next().unwrap_or_default();
    let soft_irq = values.next().unwrap_or_default();
    let steal = values.next().unwrap_or_default();

    Ok(CpuSample {
        idle: idle.saturating_add(io_wait),
        total: user
            .saturating_add(nice)
            .saturating_add(system)
            .saturating_add(idle)
            .saturating_add(io_wait)
            .saturating_add(irq)
            .saturating_add(soft_irq)
            .saturating_add(steal),
    })
}

fn read_meminfo() -> Result<HashMap<String, u64>, String> {
    let contents = fs::read_to_string("/proc/meminfo")
        .map_err(|error| format!("no se pudo leer /proc/meminfo: {error}"))?;
    let mut values = HashMap::new();
    for line in contents.lines() {
        let Some((key, remainder)) = line.split_once(':') else {
            continue;
        };
        let value = remainder
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_default();
        values.insert(key.to_owned(), value);
    }
    Ok(values)
}

fn read_load_average() -> Result<(f64, f64, f64), String> {
    let contents = fs::read_to_string("/proc/loadavg")
        .map_err(|error| format!("no se pudo leer /proc/loadavg: {error}"))?;
    let mut values = contents
        .split_whitespace()
        .take(3)
        .map(|value| value.parse::<f64>().unwrap_or_default());
    Ok((
        values.next().unwrap_or_default(),
        values.next().unwrap_or_default(),
        values.next().unwrap_or_default(),
    ))
}

fn read_disk() -> Result<(u64, u64, u64, f64), String> {
    let output = Command::new("df")
        .args(["-B1", "--output=size,used,avail,pcent", "/"])
        .output()
        .map_err(|error| format!("no se pudo ejecutar df: {error}"))?;
    if !output.status.success() {
        return Err("df terminó con error".to_owned());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .ok_or_else(|| "df no devolvió datos".to_owned())?;
    let mut values = line.split_whitespace();
    let total = values.next().and_then(|value| value.parse().ok()).unwrap_or(0);
    let used = values.next().and_then(|value| value.parse().ok()).unwrap_or(0);
    let available = values.next().and_then(|value| value.parse().ok()).unwrap_or(0);
    let percent = values
        .next()
        .map(|value| value.trim_end_matches('%'))
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or_default();
    Ok((total, used, available, percent))
}

fn read_hostname() -> String {
    fs::read_to_string("/etc/hostname")
        .unwrap_or_else(|_| "linux".to_owned())
        .trim()
        .to_owned()
}

fn read_cpu_threads() -> usize {
    fs::read_to_string("/proc/cpuinfo")
        .map(|contents| {
            contents
                .lines()
                .filter(|line| line.starts_with("processor"))
                .count()
        })
        .unwrap_or(1)
        .max(1)
}

fn read_uptime() -> u64 {
    fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|contents| {
            contents
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<f64>().ok())
        })
        .map_or(0, |value| value.max(0.0) as u64)
}

fn read_process_rss() -> u64 {
    let Ok(contents) = fs::read_to_string("/proc/self/status") else {
        return 0;
    };
    contents
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:").and_then(|value| {
                value
                    .split_whitespace()
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .map(|kilobytes| kilobytes * 1024)
            })
        })
        .unwrap_or_default()
}

#[derive(Debug, Clone)]
pub struct ProcessEntry {
    pub pid: u32,
    pub name: String,
    pub user: String,
    pub state: String,
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub command: String,
}

struct ProcessSnapshot {
    pid: u32,
    name: String,
    user: String,
    state: String,
    memory_bytes: u64,
    command: String,
    cpu_jiffies: u64,
}

const CLOCK_TICKS_PER_SECOND: f64 = 100.0;
const PROCESS_SAMPLE_MILLIS: u64 = 150;
const PROCESS_LIMIT: usize = 150;

pub fn collect_processes() -> Result<Vec<ProcessEntry>, String> {
    let users = read_users();
    let started = Instant::now();
    let first = read_process_snapshots(&users);
    thread::sleep(Duration::from_millis(PROCESS_SAMPLE_MILLIS));
    let second = read_process_snapshots(&users);
    let elapsed = started.elapsed().as_secs_f64().max(0.001);

    let mut entries: Vec<ProcessEntry> = first
        .into_iter()
        .filter_map(|snapshot| {
            let other = second
                .iter()
                .find(|candidate| candidate.pid == snapshot.pid)?;
            let delta = other.cpu_jiffies.saturating_sub(snapshot.cpu_jiffies) as f64;
            Some(ProcessEntry {
                pid: snapshot.pid,
                name: snapshot.name,
                user: snapshot.user,
                state: snapshot.state,
                cpu_percent: (100.0 * delta / (CLOCK_TICKS_PER_SECOND * elapsed)).max(0.0),
                memory_bytes: other.memory_bytes,
                command: other.command.clone(),
            })
        })
        .collect();

    entries.sort_by(|a, b| {
        b.cpu_percent
            .partial_cmp(&a.cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.memory_bytes.cmp(&a.memory_bytes))
    });
    entries.truncate(PROCESS_LIMIT);
    Ok(entries)
}

pub fn processes_to_json(entries: &[ProcessEntry]) -> String {
    format!(
        "[{}]",
        entries
            .iter()
            .map(|entry| {
                format!(
                    concat!(
                        "{{",
                        "\"pid\":{},",
                        "\"name\":{},",
                        "\"user\":{},",
                        "\"state\":{},",
                        "\"cpu_percent\":{:.2},",
                        "\"memory_bytes\":{},",
                        "\"command\":{}",
                        "}}"
                    ),
                    entry.pid,
                    json_string(&entry.name),
                    json_string(&entry.user),
                    json_string(&entry.state),
                    entry.cpu_percent,
                    entry.memory_bytes,
                    json_string(&entry.command),
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn read_users() -> HashMap<u32, String> {
    let mut users = HashMap::new();
    let Ok(contents) = fs::read_to_string("/etc/passwd") else {
        return users;
    };
    for line in contents.lines() {
        let mut fields = line.split(':');
        let (Some(name), Some(_), Some(uid)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if let Ok(uid) = uid.parse::<u32>() {
            users.insert(uid, name.to_owned());
        }
    }
    users
}

fn read_process_snapshots(users: &HashMap<u32, String>) -> Vec<ProcessSnapshot> {
    let Ok(directory) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    directory
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
            read_process_snapshot(pid, users)
        })
        .collect()
}

fn read_process_snapshot(pid: u32, users: &HashMap<u32, String>) -> Option<ProcessSnapshot> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let open_paren = stat.find('(')?;
    let close_paren = stat.rfind(')')?;
    let name = stat[open_paren + 1..close_paren].to_owned();
    let mut fields = stat[close_paren + 1..].split_whitespace();
    let state = fields.next()?.to_owned();
    let utime: u64 = fields.nth(10)?.parse().ok()?;
    let stime: u64 = fields.next()?.parse().ok()?;

    let mut memory_bytes = 0u64;
    let mut uid = None;
    if let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) {
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("VmRSS:") {
                memory_bytes = value
                    .split_whitespace()
                    .next()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or_default()
                    * 1024;
            } else if let Some(value) = line.strip_prefix("Uid:") {
                uid = value
                    .split_whitespace()
                    .nth(1)
                    .and_then(|value| value.parse::<u32>().ok());
            }
        }
    }

    let user = uid
        .and_then(|uid| {
            users
                .get(&uid)
                .cloned()
                .or_else(|| Some(uid.to_string()))
        })
        .unwrap_or_else(|| "—".to_owned());

    let command = fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .ok()
        .map(|raw| {
            raw.split('\0')
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|joined| !joined.trim().is_empty())
        .unwrap_or_else(|| format!("[{name}]"));

    Some(ProcessSnapshot {
        pid,
        name,
        user,
        state,
        memory_bytes,
        command,
        cpu_jiffies: utime.saturating_add(stime),
    })
}
