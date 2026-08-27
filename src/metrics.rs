use crate::util::json_string;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct HostMetrics {
    hostname: String,
    cpu_percent: f64,
    cpu_threads: usize,
    load_1: f64,
    load_5: f64,
    load_15: f64,
    memory_total: u64,
    memory_used: u64,
    memory_available: u64,
    swap_total: u64,
    swap_used: u64,
    disk_total: u64,
    disk_used: u64,
    disk_available: u64,
    disk_percent: f64,
    uptime_seconds: u64,
    process_rss: u64,
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
            100.0 * total_delta.saturating_sub(idle_delta) as f64 / total_delta as f64
        };

        let memory = read_meminfo()?;
        let memory_total = memory.get("MemTotal").copied().unwrap_or_default() * 1024;
        let memory_available_kib = memory.get("MemAvailable").copied().unwrap_or_else(|| {
            memory
                .get("MemFree")
                .copied()
                .unwrap_or_default()
                .saturating_add(memory.get("Buffers").copied().unwrap_or_default())
                .saturating_add(memory.get("Cached").copied().unwrap_or_default())
                .saturating_add(memory.get("SReclaimable").copied().unwrap_or_default())
                .saturating_sub(memory.get("Shmem").copied().unwrap_or_default())
        });
        let memory_available = memory_available_kib.min(memory_total / 1024) * 1024;
        let memory_used = memory_total.saturating_sub(memory_available);
        let swap_total = memory.get("SwapTotal").copied().unwrap_or_default() * 1024;
        let swap_free = memory.get("SwapFree").copied().unwrap_or_default() * 1024;
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
            swap_used: swap_total.saturating_sub(swap_free),
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
                "{{\"hostname\":{},\"cpu_percent\":{:.2},\"cpu_threads\":{},",
                "\"load_1\":{:.2},\"load_5\":{:.2},\"load_15\":{:.2},",
                "\"memory_total\":{},\"memory_used\":{},\"memory_available\":{},",
                "\"swap_total\":{},\"swap_used\":{},",
                "\"disk_total\":{},\"disk_used\":{},\"disk_available\":{},",
                "\"disk_percent\":{:.2},\"uptime_seconds\":{},\"process_rss\":{}}}"
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
    let mut values = contents
        .lines()
        .next()
        .ok_or_else(|| "/proc/stat está vacío".to_owned())?
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
    let mut values = stdout
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .ok_or_else(|| "df no devolvió datos".to_owned())?
        .split_whitespace();
    let total = values
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let used = values
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let available = values
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
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
        .and_then(|contents| contents.split_whitespace().next()?.parse::<f64>().ok())
        .map_or(0, |value| value.max(0.0) as u64)
}

fn read_process_rss() -> u64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("VmRSS:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
                    .map(|kilobytes| kilobytes * 1024)
            })
        })
        .unwrap_or_default()
}
