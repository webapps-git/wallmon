//! CPU usage from `/proc/stat`, temperature from hwmon, power from Intel RAPL.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use super::{read, read_number};
use crate::state::Cpu;

/// Jiffies spent busy and in total, for the whole CPU (index 0) and each logical CPU.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Times {
    pub busy: u64,
    pub total: u64,
}

struct Rapl {
    path: PathBuf,
    max: u64,
    prev: Option<(u64, Instant)>,
}

pub struct Reader {
    model: String,
    temp: Option<PathBuf>,
    rapl: Option<Rapl>,
    prev: Vec<Times>,
}

impl Reader {
    pub fn new() -> Self {
        let model = read("/proc/cpuinfo")
            .ok()
            .and_then(|s| s.lines().find_map(|l| l.strip_prefix("model name").map(|r| r.trim_start_matches([' ', '\t', ':']).to_string())))
            .map(|m| short_model(&m))
            .unwrap_or_else(|| "CPU".into());
        let rapl = PathBuf::from("/sys/class/powercap/intel-rapl:0");
        let rapl = read_number(rapl.join("energy_uj")).map(|_| Rapl {
            max: read_number(rapl.join("max_energy_range_uj")).unwrap_or(0.0) as u64,
            path: rapl.join("energy_uj"),
            prev: None,
        });
        Reader { model, temp: find_temp(), rapl, prev: vec![] }
    }

    pub fn read(&mut self) -> Result<Cpu, String> {
        let now = parse_stat(&read("/proc/stat")?)?;
        let base = |i: usize| if self.prev.len() == now.len() { self.prev[i] } else { Times::default() };
        let usage = percent(base(0), now[0]);
        let cores = (1..now.len()).map(|i| percent(base(i), now[i])).collect();
        self.prev = now;
        Ok(Cpu {
            model: self.model.clone(),
            usage,
            cores,
            temp: self.temp.as_ref().and_then(read_number).map(|m| m / 1000.0),
            power: self.rapl.as_mut().and_then(power),
        })
    }
}

impl Default for Reader {
    fn default() -> Self {
        Self::new()
    }
}

/// Watts since the previous call. RAPL counts microjoules and wraps at `max`.
fn power(r: &mut Rapl) -> Option<f64> {
    let e = read_number(&r.path)? as u64;
    let now = Instant::now();
    let watts = r.prev.and_then(|(pe, pt)| {
        let de = if e >= pe { e - pe } else { e + r.max.saturating_sub(pe) };
        let dt = now.duration_since(pt).as_secs_f64();
        (dt > 0.0).then(|| de as f64 / 1e6 / dt)
    });
    r.prev = Some((e, now));
    watts
}

pub fn parse_stat(s: &str) -> Result<Vec<Times>, String> {
    let times: Vec<Times> = s
        .lines()
        .filter(|l| l.starts_with("cpu"))
        .map(|l| {
            // user nice system idle iowait irq softirq steal (guest time is already in user)
            let v: Vec<u64> = l.split_whitespace().skip(1).take(8).filter_map(|x| x.parse().ok()).collect();
            let total: u64 = v.iter().sum();
            let idle = v.get(3).copied().unwrap_or(0) + v.get(4).copied().unwrap_or(0);
            Times { busy: total - idle, total }
        })
        .collect();
    if times.is_empty() { Err("/proc/stat: brak linii cpu".into()) } else { Ok(times) }
}

fn percent(a: Times, b: Times) -> f64 {
    let total = b.total.saturating_sub(a.total);
    if total == 0 { 0.0 } else { b.busy.saturating_sub(a.busy) as f64 * 100.0 / total as f64 }
}

/// "Intel(R) Core(TM) i5-10400 CPU @ 2.90GHz" → "i5-10400".
pub fn short_model(name: &str) -> String {
    let name = name.split(" @ ").next().unwrap_or(name).replace("(R)", "").replace("(TM)", "");
    let words: Vec<&str> = name
        .split_whitespace()
        .filter(|w| !matches!(*w, "Intel" | "AMD" | "Core" | "CPU" | "Processor"))
        .collect();
    if words.is_empty() { name.trim().to_string() } else { words.join(" ") }
}

/// Package temperature: coretemp "Package id 0", k10temp "Tctl", else the chip's first sensor.
fn find_temp() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir("/sys/class/hwmon").ok()?.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for dir in dirs {
        let name = read(dir.join("name")).unwrap_or_default();
        let (chip_label, chip) = match name.trim() {
            "coretemp" => ("Package id 0", true),
            "k10temp" | "zenpower" => ("Tctl", true),
            _ => ("", false),
        };
        if !chip {
            continue;
        }
        for i in 1..=64 {
            let label = read(dir.join(format!("temp{i}_label"))).unwrap_or_default();
            if label.trim() == chip_label {
                return Some(dir.join(format!("temp{i}_input")));
            }
        }
        let first = dir.join("temp1_input");
        if first.exists() {
            return Some(first);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_model_names() {
        assert_eq!(short_model("Intel(R) Core(TM) i5-10400 CPU @ 2.90GHz"), "i5-10400");
        assert_eq!(short_model("AMD Ryzen 7 5800X 8-Core Processor"), "Ryzen 7 5800X 8-Core");
    }

    #[test]
    fn computes_usage_between_two_reads() {
        let a = parse_stat("cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 50 0 50 400 0 0 0 0 0 0\n").unwrap();
        let b = parse_stat("cpu  200 0 200 1400 200 0 0 0 0 0\ncpu0 150 0 50 400 0 0 0 0 0 0\n").unwrap();
        assert_eq!(a.len(), 2);
        assert!((percent(a[0], b[0]) - 20.0).abs() < 1e-9);
        assert!((percent(a[1], b[1]) - 100.0).abs() < 1e-9);
    }
}
