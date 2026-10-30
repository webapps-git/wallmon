//! `--demo`: made-up data for every panel, with an optional failure scenario.
//! Names of sites, containers, services and jails come from the configuration.

use std::f64::consts::PI;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{Shared, spawn};
use crate::config::Config;
use crate::state::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    Normal,
    /// CPU and Apache overloaded, MariaDB near its limit, the first container stopped,
    /// UPS on battery, sites slow or down, fail2ban not answering.
    Failure,
}

/// Seconds of history simulated at start, so graphs are full at once.
const WARMUP: u64 = 300;
/// When the failure begins, in demo seconds.
const FAILURE_AT: f64 = 255.0;

const MIB: f64 = 1024.0 * 1024.0;
const GIB: f64 = MIB * 1024.0;

pub struct Demo {
    scenario: Scenario,
    start: Instant,
    interface: String,
    containers: Vec<String>,
    sites: Vec<crate::config::Site>,
    units: Vec<String>,
    jails: Vec<String>,
    mariadb: bool,
    postgres: bool,
}

impl Demo {
    pub fn new(scenario: Scenario, cfg: &Config) -> Self {
        let jails = match &cfg.fail2ban {
            Some(f) if !f.jails.is_empty() => f.jails.clone(),
            _ => ["sshd", "apache-auth", "apache-badbots"].map(String::from).to_vec(),
        };
        Demo {
            scenario,
            start: Instant::now(),
            interface: cfg.network.interfaces.first().cloned().unwrap_or_else(|| "eth0".into()),
            containers: cfg.docker.as_ref().map(|d| d.expected.clone()).unwrap_or_default(),
            sites: cfg.sites.clone(),
            units: cfg.services.as_ref().map(|s| s.units.clone()).unwrap_or_default(),
            jails,
            mariadb: cfg.mariadb.is_some(),
            postgres: cfg.postgresql.is_some(),
        }
    }

    /// Demo time in seconds; the live demo continues right after the warm-up.
    fn now(&self) -> f64 {
        WARMUP as f64 + self.start.elapsed().as_secs_f64()
    }

    /// 0 while everything is fine, rising to 1 when the failure is in full swing.
    fn failing(&self, t: f64) -> f64 {
        match self.scenario {
            Scenario::Normal => 0.0,
            Scenario::Failure => ((t - FAILURE_AT) / 15.0).clamp(0.0, 1.0),
        }
    }

    pub fn host(&self, t: f64) -> Host {
        let f = self.failing(t);
        Host {
            uptime: Duration::from_secs(285_120 + t as u64),
            load: [
                lerp(0.42, 23.4, f) + noise(1, t) * 0.1,
                lerp(0.38, 18.1, f) + noise(2, t) * 0.05,
                lerp(0.35, 9.7, f),
            ]
            .map(|v: f64| v.max(0.0)),
            threads: 12,
        }
    }

    pub fn cpu(&self, t: f64) -> Cpu {
        let f = self.failing(t);
        let usage = (lerp(8.0, 96.0, f) + noise(10, t) * lerp(6.0, 3.0, f)).clamp(0.0, 100.0);
        Cpu {
            model: "i5-10400".into(),
            usage,
            cores: (0..12).map(|i| (usage + noise(100 + i, t) * lerp(7.0, 4.0, f)).clamp(0.0, 100.0)).collect(),
            temp: Some(lerp(34.0, 86.0, f) + noise(11, t)),
            power: Some(lerp(14.0, 64.0, f) + noise(12, t) * 2.0),
        }
    }

    pub fn processes(&self, t: f64) -> Vec<Process> {
        let f = self.failing(t);
        let p = |pid, user: &str, cpu: f64, mib: f64, name: &str, args: &str| Process {
            pid,
            user: user.into(),
            cpu: (cpu + noise(pid as u64, t) * cpu * 0.1).max(0.0),
            memory: (mib * MIB) as u64,
            name: name.into(),
            args: args.into(),
        };
        let mut list = if f < 0.5 {
            vec![
                p(1116, "mysql", 3.1, 487.0, "mariadbd", ""),
                p(316031, "www-data", 2.4, 118.0, "apache2", "-k start"),
                p(315616, "www-data", 1.9, 112.0, "apache2", "-k start"),
                p(1086, "root", 1.2, 159.0, "containerd", ""),
                p(2911, "root", 0.7, 87.0, "app", ""),
                p(4242, "root", 0.4, 9.0, "wallmon", ""),
                p(1043, "root", 0.3, 110.0, "dockerd", "-H fd:// --containerd=/run/containerd/containerd.sock"),
                p(1131, "postgres", 0.2, 38.0, "postgres", "17/main: checkpointer"),
                p(1059, "root", 0.1, 30.0, "fail2ban-server", "-xf start"),
                p(1005, "root", 0.1, 3.0, "apcupsd", ""),
            ]
        } else {
            let mut v = vec![p(1116, "mysql", 312.4, 1946.0, "mariadbd", "")];
            for (i, pid) in [316031, 315616, 315040, 311197, 310293, 310210, 310207, 309169].into_iter().enumerate() {
                v.push(p(pid, "www-data", 94.0 - i as f64 * 3.0, 135.0 + i as f64, "apache2", "-k start"));
            }
            v.push(p(1086, "root", 3.1, 161.0, "containerd", ""));
            v
        };
        list.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
        list
    }

    pub fn apache(&self, t: f64) -> Apache {
        let f = self.failing(t);
        let busy = (lerp(6.0, 40.0, f) + noise(20, t) * 2.0).round().clamp(1.0, 40.0) as u32;
        Apache {
            mpm: "prefork".into(),
            busy,
            idle: if busy >= 40 { 0 } else { 6 },
            max: 40,
            requests_per_sec: (lerp(4.2, 182.0, f) + noise(21, t) * lerp(3.0, 20.0, f)).max(0.0),
        }
    }

    pub fn containers(&self, t: f64) -> Vec<Container> {
        let f = self.failing(t);
        self.containers
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let down = f > 0.5 && i == 0;
                Container {
                    name: name.clone(),
                    expected: true,
                    state: if down { ContainerState::Stopped } else { ContainerState::Running },
                    health: (!down).then_some(Health::Healthy),
                    cpu: (!down).then(|| lerp(0.4, 12.3, f) * (1.0 + noise(30 + i as u64, t) * 0.2)),
                    memory: (!down).then(|| ((60.0 + 25.0 * hash(31, i as i64).abs() + 140.0 * f) * MIB) as u64),
                }
            })
            .collect()
    }

    pub fn ups(&self, t: f64) -> Ups {
        let f = self.failing(t);
        let on_battery = f > 0.3;
        Ups {
            model: "APC Back-UPS BX950MI".into(),
            on_battery,
            on_battery_for: Duration::from_secs((t - FAILURE_AT).max(0.0) as u64),
            load: lerp(21.0, 38.0, f),
            charge: lerp(100.0, 64.0, f),
            time_left: Duration::from_secs((lerp(27.0, 14.0, f) * 60.0) as u64),
            line_voltage: if on_battery { 0.0 } else { 230.0 + noise(40, t) },
        }
    }

    pub fn memory(&self, t: f64) -> Memory {
        let f = self.failing(t);
        let total = 15.5 * GIB;
        let used = (lerp(4.6, 11.8, f) + noise(50, t) * 0.05) * GIB;
        let cache = lerp(9.2, 2.9, f) * GIB;
        Memory {
            total: total as u64,
            used: used as u64,
            cache: cache as u64,
            free: (total - used - cache).max(0.0) as u64,
            swap_total: (15.8 * GIB) as u64,
            swap_used: (lerp(0.0, 0.4, f) * GIB) as u64,
        }
    }

    pub fn disk(&self, t: f64) -> Disk {
        let f = self.failing(t);
        Disk {
            model: "NVMe 512 GB".into(),
            mount: "/".into(),
            size: (452.0 * GIB) as u64,
            used: (22.0 * GIB) as u64,
            read: (lerp(0.2, 12.4, f) * (1.0 + noise(60, t) * 0.8)).max(0.0) * MIB,
            write: (lerp(1.1, 48.1, f) * (1.0 + noise(61, t) * 0.6)).max(0.0) * MIB,
            temp: Some(lerp(29.0, 47.0, f)),
        }
    }

    pub fn network(&self, t: f64) -> Network {
        let f = self.failing(t);
        Network {
            interface: self.interface.clone(),
            address: "192.0.2.10".into(),
            rx: (lerp(0.3, 4.8, f) * (1.0 + noise(70, t) * 0.6)).max(0.0) * 1e6,
            tx: (lerp(1.2, 26.4, f) * (1.0 + noise(71, t) * 0.6)).max(0.0) * 1e6,
            tcp: lerp(142.0, 1904.0, f) as u32,
            established: lerp(87.0, 1388.0, f) as u32,
        }
    }

    pub fn sensors(&self, t: f64) -> Sensors {
        let f = self.failing(t);
        let temps = [("acpitz", 28.0, 41.0), ("temp1", 26.0, 39.0), ("temp2", 46.0, 71.0), ("temp4", 32.0, 45.0),
                     ("temp5", 25.0, 33.0), ("temp6", 25.0, 31.0), ("temp7", 24.0, 29.0)];
        Sensors {
            source: "dell_smm · acpitz".into(),
            fan: Some(Fan { rpm: lerp(1132.0, 2950.0, f) as u32, max: Some(3230) }),
            temps: temps.iter().map(|(n, a, b)| (n.to_string(), lerp(*a, *b, f))).collect(),
        }
    }

    pub fn services(&self, _t: f64) -> Services {
        Services { failed: 0, units: self.units.iter().map(|u| (u.clone(), UnitState::Active)).collect() }
    }

    pub fn sites(&self, t: f64) -> Vec<Site> {
        let f = self.failing(t);
        let n = self.sites.len();
        self.sites
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let code = s.ok.first().copied().unwrap_or(200);
                let normal_ms = 80.0 + 150.0 * hash(80, i as i64).abs();
                let outcome = if f > 0.5 && n > 1 && i == n - 1 {
                    SiteOutcome::Response { code: 502, time: Duration::from_millis(310) }
                } else if f > 0.5 && (i == 3 || i == 6) {
                    SiteOutcome::Timeout
                } else {
                    let ms = lerp(normal_ms, 1900.0 + 4200.0 * hash(81, i as i64).abs(), f);
                    SiteOutcome::Response { code, time: Duration::from_millis(ms as u64) }
                };
                let accepted = match &outcome {
                    SiteOutcome::Response { code, .. } => s.accepts(*code),
                    _ => false,
                };
                Site { name: s.name().to_string(), outcome, accepted }
            })
            .collect()
    }

    pub fn databases(&self, t: f64) -> Databases {
        let f = self.failing(t);
        Databases {
            mariadb: self.mariadb.then(|| MariaDb {
                version: "11.8".into(),
                connections: lerp(9.0, 148.0, f) as u32,
                max_connections: 151,
                threads_running: lerp(2.0, 41.0, f) as u32,
                queries_per_sec: (lerp(84.0, 1240.0, f) * (1.0 + noise(90, t) * 0.2)).round(),
                slow_per_min: lerp(0.0, 14.0, f).round(),
            }),
            postgres: self.postgres.then(|| Postgres {
                version: "17".into(),
                connections: lerp(7.0, 9.0, f) as u32,
                max_connections: 100,
                active: lerp(1.0, 2.0, f) as u32,
                idle: 6,
            }),
            errors: vec![],
        }
    }

    /// Fails during the failure scenario, as a hung `fail2ban-client` would.
    pub fn fail2ban(&self, t: f64) -> Result<Vec<Jail>, String> {
        if self.failing(t) > 0.5 {
            return Err("brak odpowiedzi".into());
        }
        Ok(self
            .jails
            .iter()
            .enumerate()
            .map(|(i, name)| Jail { name: name.clone(), banned: (hash(95, i as i64).abs() * 6.0) as u32 })
            .collect())
    }

    /// Fills the state as if the demo had already been running for a few minutes.
    pub fn prefill(&self, s: &mut State, now: Instant) {
        let at = |secs_ago: u64| now.checked_sub(Duration::from_secs(secs_ago)).unwrap_or(now);
        for i in 0..WARMUP {
            let t = i as f64;
            let when = at(WARMUP - i);
            s.set_cpu(Ok(self.cpu(t)), when);
            s.set_memory(Ok(self.memory(t)), when);
            s.set_disk(Ok(self.disk(t)), when);
            s.set_network(Ok(self.network(t)), when);
            if i % every::APACHE.as_secs() == 0 {
                s.set_apache(Ok(self.apache(t)), when);
            }
        }
        let t = WARMUP as f64;
        s.host.apply(Ok(self.host(t)), now);
        s.processes.apply(Ok(self.processes(t)), now);
        s.containers.apply(Ok(self.containers(t)), now);
        s.ups.apply(Ok(self.ups(t)), now);
        s.disk_wear.apply(Ok(1.0), now);
        s.sensors.apply(Ok(self.sensors(t)), now);
        s.services.apply(Ok(self.services(t)), now);
        s.databases.apply(Ok(self.databases(t)), now);
        s.sites.apply(Ok(self.sites(t)), at(23));
        s.fail2ban.apply(self.fail2ban(t - 120.0), at(120));
        s.fail2ban.apply(self.fail2ban(t), now);
    }
}

/// Starts a demo task for every source.
pub fn spawn_all(state: &Shared, demo: Arc<Demo>) {
    const LIMIT: Duration = Duration::from_secs(5);
    macro_rules! source {
        ($every:expr, $method:ident, $apply:expr) => {{
            let d = demo.clone();
            spawn(state, $every, LIMIT, move || {
                let d = d.clone();
                async move { Ok(d.$method(d.now())) }
            }, $apply);
        }};
    }
    source!(every::HOST, host, |s, r, now| s.host.apply(r, now));
    source!(every::CPU, cpu, |s, r, now| s.set_cpu(r, now));
    source!(every::PROCESSES, processes, |s, r, now| s.processes.apply(r, now));
    source!(every::APACHE, apache, |s, r, now| s.set_apache(r, now));
    source!(every::CONTAINERS, containers, |s, r, now| s.containers.apply(r, now));
    source!(every::UPS, ups, |s, r, now| s.ups.apply(r, now));
    source!(every::MEMORY, memory, |s, r, now| s.set_memory(r, now));
    source!(every::DISK, disk, |s, r, now| s.set_disk(r, now));
    spawn(state, every::DISK_WEAR, LIMIT, || async { Ok(1.0) }, |s, r, now| s.disk_wear.apply(r, now));
    source!(every::NETWORK, network, |s, r, now| s.set_network(r, now));
    source!(every::SENSORS, sensors, |s, r, now| s.sensors.apply(r, now));
    source!(every::SERVICES, services, |s, r, now| s.services.apply(r, now));
    source!(every::SITES, sites, |s, r, now| s.sites.apply(r, now));
    source!(every::DATABASES, databases, |s, r, now| s.databases.apply(r, now));

    // fail2ban hangs in the failure scenario, so the time limit kicks in.
    let d = demo.clone();
    spawn(state, every::FAIL2BAN, LIMIT, move || {
        let d = d.clone();
        async move {
            match d.fail2ban(d.now()) {
                Ok(v) => Ok(v),
                Err(_) => std::future::pending().await,
            }
        }
    }, |s, r, now| s.fail2ban.apply(r, now));
}

fn lerp(a: f64, b: f64, f: f64) -> f64 {
    a + (b - a) * f
}

/// Deterministic pseudo-random number in -1..1.
fn hash(seed: u64, i: i64) -> f64 {
    let mut x = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ seed.wrapping_mul(0xD1B5_4A32_D192_ED03);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

/// Smooth noise in -1..1 that changes over a few seconds, with a little jitter.
fn noise(seed: u64, t: f64) -> f64 {
    let k = t / 5.0;
    let i = k.floor();
    let s = (1.0 - ((k - i) * PI).cos()) / 2.0;
    let a = hash(seed, i as i64);
    let b = hash(seed, i as i64 + 1);
    0.7 * (a + (b - a) * s) + 0.3 * hash(seed ^ 0xABCD, t as i64)
}
