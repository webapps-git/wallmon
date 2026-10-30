//! Shared state: the latest reading of every source plus short histories for graphs.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How often each source is read.
pub mod every {
    use std::time::Duration;

    const fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    pub const HOST: Duration = s(1);
    pub const CPU: Duration = s(1);
    pub const PROCESSES: Duration = s(2);
    pub const APACHE: Duration = s(2);
    pub const CONTAINERS: Duration = s(5);
    pub const UPS: Duration = s(5);
    pub const MEMORY: Duration = s(1);
    pub const DISK: Duration = s(1);
    pub const DISK_WEAR: Duration = s(600);
    pub const NETWORK: Duration = s(1);
    pub const SENSORS: Duration = s(5);
    pub const SERVICES: Duration = s(15);
    pub const SITES: Duration = s(60);
    pub const DATABASES: Duration = s(10);
    pub const FAIL2BAN: Duration = s(30);
}

/// Graphs cover this much time.
pub const GRAPH_SPAN: Duration = Duration::from_secs(300);

/// The last result of one source.
#[derive(Debug, Clone)]
pub struct Reading<T> {
    pub value: Option<T>,
    pub updated: Option<Instant>,
    pub error: Option<String>,
    pub interval: Duration,
}

impl<T> Reading<T> {
    pub fn new(interval: Duration) -> Self {
        Reading { value: None, updated: None, error: None, interval }
    }

    /// Stores a result. An error keeps the previous value, which then ages.
    pub fn apply(&mut self, result: Result<T, String>, now: Instant) {
        match result {
            Ok(v) => {
                self.value = Some(v);
                self.updated = Some(now);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Age of the data once it is older than three intervals.
    pub fn stale_for(&self, now: Instant) -> Option<Duration> {
        let age = now.saturating_duration_since(self.updated?);
        (age > self.interval * 3).then_some(age)
    }

    pub fn age(&self, now: Instant) -> Option<Duration> {
        Some(now.saturating_duration_since(self.updated?))
    }
}

/// Recent samples of one value, oldest first.
#[derive(Debug, Clone)]
pub struct History {
    cap: usize,
    values: VecDeque<f64>,
}

impl History {
    /// History long enough to cover `GRAPH_SPAN` when sampled every `interval`.
    pub fn for_interval(interval: Duration) -> Self {
        let cap = (GRAPH_SPAN.as_secs_f64() / interval.as_secs_f64()).round().max(1.0) as usize;
        History { cap, values: VecDeque::with_capacity(cap) }
    }

    pub fn push(&mut self, v: f64) {
        if self.values.len() == self.cap {
            self.values.pop_front();
        }
        self.values.push_back(v);
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn values(&self) -> &VecDeque<f64> {
        &self.values
    }

    pub fn max(&self) -> f64 {
        self.values.iter().copied().fold(0.0, f64::max)
    }
}

#[derive(Debug, Clone)]
pub struct Host {
    pub uptime: Duration,
    pub load: [f64; 3],
    pub threads: usize,
}

#[derive(Debug, Clone)]
pub struct Cpu {
    pub model: String,
    /// Percent of the whole CPU.
    pub usage: f64,
    /// Percent per logical CPU.
    pub cores: Vec<f64>,
    pub temp: Option<f64>,
    pub power: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Process {
    pub pid: u32,
    pub user: String,
    /// Percent of one CPU, as in `top`.
    pub cpu: f64,
    pub memory: u64,
    pub name: String,
    pub args: String,
}

#[derive(Debug, Clone)]
pub struct Apache {
    pub mpm: String,
    pub busy: u32,
    pub idle: u32,
    pub max: u32,
    pub requests_per_sec: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    Running,
    Stopped,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Unhealthy,
    Starting,
}

#[derive(Debug, Clone)]
pub struct Container {
    pub name: String,
    pub expected: bool,
    pub state: ContainerState,
    pub health: Option<Health>,
    pub cpu: Option<f64>,
    pub memory: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Ups {
    pub model: String,
    pub on_battery: bool,
    pub on_battery_for: Duration,
    pub load: f64,
    pub charge: f64,
    pub time_left: Duration,
    pub line_voltage: f64,
}

#[derive(Debug, Clone)]
pub struct Memory {
    pub total: u64,
    pub used: u64,
    pub cache: u64,
    pub free: u64,
    pub swap_total: u64,
    pub swap_used: u64,
}

#[derive(Debug, Clone)]
pub struct Disk {
    pub model: String,
    pub mount: String,
    pub size: u64,
    pub used: u64,
    /// Bytes per second.
    pub read: f64,
    pub write: f64,
    pub temp: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Network {
    pub interface: String,
    pub address: String,
    /// Bytes per second.
    pub rx: f64,
    pub tx: f64,
    pub tcp: u32,
    pub established: u32,
}

#[derive(Debug, Clone)]
pub struct Fan {
    pub rpm: u32,
    pub max: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Sensors {
    pub source: String,
    pub fan: Option<Fan>,
    pub temps: Vec<(String, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitState {
    Active,
    Activating,
    Inactive,
    Failed,
}

#[derive(Debug, Clone)]
pub struct Services {
    /// Failed units in the whole system, not only the listed ones.
    pub failed: u32,
    pub units: Vec<(String, UnitState)>,
}

#[derive(Debug, Clone)]
pub enum SiteOutcome {
    Response { code: u16, time: Duration },
    Timeout,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct Site {
    pub name: String,
    pub outcome: SiteOutcome,
    pub accepted: bool,
}

#[derive(Debug, Clone)]
pub struct MariaDb {
    pub version: String,
    pub connections: u32,
    pub max_connections: u32,
    pub threads_running: u32,
    pub queries_per_sec: f64,
    pub slow_per_min: f64,
}

#[derive(Debug, Clone)]
pub struct Postgres {
    pub version: String,
    pub connections: u32,
    pub max_connections: u32,
    pub active: u32,
    pub idle: u32,
}

#[derive(Debug, Clone)]
pub struct Databases {
    pub mariadb: Option<MariaDb>,
    pub postgres: Option<Postgres>,
    /// Why a configured database could not be read; the other one is still shown.
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Jail {
    pub name: String,
    pub banned: u32,
}

#[derive(Debug, Clone)]
pub struct State {
    pub hostname: String,
    pub host: Reading<Host>,
    pub cpu: Reading<Cpu>,
    pub cpu_history: History,
    pub processes: Reading<Vec<Process>>,
    pub apache: Reading<Apache>,
    pub apache_history: History,
    pub containers: Reading<Vec<Container>>,
    pub ups: Reading<Ups>,
    pub memory: Reading<Memory>,
    pub memory_history: History,
    pub disk: Reading<Disk>,
    pub disk_read_history: History,
    pub disk_write_history: History,
    /// Percent of rated endurance used (NVMe "percentage used").
    pub disk_wear: Reading<f64>,
    pub network: Reading<Network>,
    pub rx_history: History,
    pub tx_history: History,
    pub sensors: Reading<Sensors>,
    pub services: Reading<Services>,
    pub sites: Reading<Vec<Site>>,
    pub databases: Reading<Databases>,
    pub fail2ban: Reading<Vec<Jail>>,
}

impl State {
    pub fn new(hostname: String) -> Self {
        State {
            hostname,
            host: Reading::new(every::HOST),
            cpu: Reading::new(every::CPU),
            cpu_history: History::for_interval(every::CPU),
            processes: Reading::new(every::PROCESSES),
            apache: Reading::new(every::APACHE),
            apache_history: History::for_interval(every::APACHE),
            containers: Reading::new(every::CONTAINERS),
            ups: Reading::new(every::UPS),
            memory: Reading::new(every::MEMORY),
            memory_history: History::for_interval(every::MEMORY),
            disk: Reading::new(every::DISK),
            disk_read_history: History::for_interval(every::DISK),
            disk_write_history: History::for_interval(every::DISK),
            disk_wear: Reading::new(every::DISK_WEAR),
            network: Reading::new(every::NETWORK),
            rx_history: History::for_interval(every::NETWORK),
            tx_history: History::for_interval(every::NETWORK),
            sensors: Reading::new(every::SENSORS),
            services: Reading::new(every::SERVICES),
            sites: Reading::new(every::SITES),
            databases: Reading::new(every::DATABASES),
            fail2ban: Reading::new(every::FAIL2BAN),
        }
    }

    pub fn set_cpu(&mut self, r: Result<Cpu, String>, now: Instant) {
        if let Ok(c) = &r {
            self.cpu_history.push(c.usage);
        }
        self.cpu.apply(r, now);
    }

    pub fn set_apache(&mut self, r: Result<Apache, String>, now: Instant) {
        if let Ok(a) = &r {
            self.apache_history.push(a.requests_per_sec);
        }
        self.apache.apply(r, now);
    }

    pub fn set_memory(&mut self, r: Result<Memory, String>, now: Instant) {
        if let Ok(m) = &r {
            self.memory_history.push(m.used as f64 / m.total.max(1) as f64);
        }
        self.memory.apply(r, now);
    }

    pub fn set_disk(&mut self, r: Result<Disk, String>, now: Instant) {
        if let Ok(d) = &r {
            self.disk_read_history.push(d.read);
            self.disk_write_history.push(d.write);
        }
        self.disk.apply(r, now);
    }

    pub fn set_network(&mut self, r: Result<Network, String>, now: Instant) {
        if let Ok(n) = &r {
            self.rx_history.push(n.rx);
            self.tx_history.push(n.tx);
        }
        self.network.apply(r, now);
    }
}
