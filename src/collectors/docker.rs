//! Containers from the Docker Engine API on its unix socket: state, health, CPU and memory.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use super::http;
use crate::state::{Container, ContainerState, Health};

const TIMEOUT: Duration = Duration::from_secs(3);

pub struct Reader {
    socket: String,
    expected: Vec<String>,
    /// Container CPU time and host CPU time from the previous read, by container id.
    prev: HashMap<String, (u64, u64)>,
}

/// One entry of `GET /containers/json`.
#[derive(Debug, PartialEq)]
pub struct Listed {
    pub id: String,
    pub name: String,
    pub running: bool,
    pub health: Option<Health>,
}

impl Reader {
    pub fn new(socket: String, expected: Vec<String>) -> Self {
        Reader { socket, expected, prev: HashMap::new() }
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        let (code, body) = http::get_unix(&self.socket, path, TIMEOUT)?;
        if code != 200 {
            return Err(format!("Docker API {path}: HTTP {code}"));
        }
        serde_json::from_str(&body).map_err(|e| format!("Docker API {path}: {e}"))
    }

    pub fn read(&mut self) -> Result<Vec<Container>, String> {
        let listed = parse_list(&self.get("/containers/json?all=1")?);
        let mut out = vec![];
        let mut seen = HashMap::new();
        // Expected containers first, in configured order, then any other running ones.
        let mut order: Vec<(&Listed, bool)> = vec![];
        let mut missing = vec![];
        for name in &self.expected {
            match listed.iter().find(|c| &c.name == name) {
                Some(c) => order.push((c, true)),
                None => missing.push(name.clone()),
            }
        }
        order.extend(listed.iter().filter(|c| c.running && !self.expected.contains(&c.name)).map(|c| (c, false)));

        for (c, expected) in order {
            let (cpu, memory) = if c.running {
                match self.get(&format!("/containers/{}/stats?stream=false&one-shot=true", c.id)) {
                    Ok(stats) => {
                        let (cpu, mem, sample) = parse_stats(&stats, self.prev.get(&c.id).copied());
                        seen.insert(c.id.clone(), sample);
                        (cpu, mem)
                    }
                    Err(_) => (None, None),
                }
            } else {
                (None, None)
            };
            out.push(Container {
                name: c.name.clone(),
                expected,
                state: if c.running { ContainerState::Running } else { ContainerState::Stopped },
                health: c.health,
                cpu,
                memory,
            });
        }
        for name in missing {
            out.push(Container { name, expected: true, state: ContainerState::Missing, health: None, cpu: None, memory: None });
        }
        self.prev = seen;
        Ok(out)
    }
}

pub fn parse_list(v: &Value) -> Vec<Listed> {
    v.as_array()
        .map(|list| {
            list.iter()
                .map(|c| Listed {
                    id: c["Id"].as_str().unwrap_or_default().to_string(),
                    name: c["Names"][0].as_str().unwrap_or_default().trim_start_matches('/').to_string(),
                    running: c["State"].as_str() == Some("running"),
                    health: health(c["Status"].as_str().unwrap_or_default()),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Health from the status text, e.g. "Up 11 hours (healthy)".
fn health(status: &str) -> Option<Health> {
    if status.contains("(healthy)") {
        Some(Health::Healthy)
    } else if status.contains("(unhealthy)") {
        Some(Health::Unhealthy)
    } else if status.contains("(health: starting)") {
        Some(Health::Starting)
    } else {
        None
    }
}

/// CPU percent (100 = one core, as in `docker stats`), memory without page cache,
/// and the CPU counters to compare with next time.
pub fn parse_stats(s: &Value, prev: Option<(u64, u64)>) -> (Option<f64>, Option<u64>, (u64, u64)) {
    let total = s["cpu_stats"]["cpu_usage"]["total_usage"].as_u64().unwrap_or(0);
    let system = s["cpu_stats"]["system_cpu_usage"].as_u64().unwrap_or(0);
    let cpus = s["cpu_stats"]["online_cpus"].as_u64().unwrap_or(1) as f64;
    let cpu = prev.and_then(|(pt, ps)| {
        let ds = system.saturating_sub(ps);
        (ds > 0).then(|| total.saturating_sub(pt) as f64 / ds as f64 * cpus * 100.0)
    });
    let usage = s["memory_stats"]["usage"].as_u64();
    let cache = s["memory_stats"]["stats"]["inactive_file"].as_u64().unwrap_or(0);
    (cpu, usage.map(|u| u.saturating_sub(cache)), (total, system))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_container_list() {
        let v: Value = serde_json::from_str(
            r#"[{"Id":"abc","Names":["/app"],"State":"running","Status":"Up 11 hours (healthy)"},
                {"Id":"def","Names":["/old"],"State":"exited","Status":"Exited (0) 2 days ago"}]"#,
        )
        .unwrap();
        let l = parse_list(&v);
        assert_eq!(l[0], Listed { id: "abc".into(), name: "app".into(), running: true, health: Some(Health::Healthy) });
        assert!(!l[1].running);
        assert_eq!(l[1].health, None);
    }

    #[test]
    fn computes_cpu_like_docker_stats() {
        let v: Value = serde_json::from_str(
            r#"{"cpu_stats":{"cpu_usage":{"total_usage":2000},"system_cpu_usage":100000,"online_cpus":12},
                "memory_stats":{"usage":90000000,"stats":{"inactive_file":10000000}}}"#,
        )
        .unwrap();
        let (cpu, mem, sample) = parse_stats(&v, Some((1000, 50000)));
        assert!((cpu.unwrap() - 24.0).abs() < 1e-9);
        assert_eq!(mem, Some(80_000_000));
        assert_eq!(sample, (2000, 100000));
        assert_eq!(parse_stats(&v, None).0, None);
    }
}
