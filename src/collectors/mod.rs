//! Data sources. Each one runs as its own task with its own interval and time limit.

pub mod apache;
pub mod cpu;
pub mod databases;
pub mod demo;
pub mod disk;
pub mod docker;
pub mod fail2ban;
pub mod host;
pub mod http;
pub mod memory;
pub mod network;
pub mod processes;
pub mod sensors;
pub mod services;
pub mod sites;
pub mod ups;

use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::time::{MissedTickBehavior, interval, timeout};

use crate::config::Config;
use crate::state::{State, every};

pub type Shared = Arc<Mutex<State>>;

pub(crate) fn read(path: impl AsRef<Path>) -> Result<String, String> {
    let path = path.as_ref();
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Reads a sysfs value such as `/sys/.../temp1_input`.
pub(crate) fn read_number(path: impl AsRef<Path>) -> Option<f64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Runs a reader on the blocking thread pool, so a hung file or command cannot stop the
/// other sources. While a previous read of the same source still hangs, no new one starts.
pub(crate) async fn blocking<C, T>(reader: Arc<Mutex<C>>, f: fn(&mut C) -> Result<T, String>) -> Result<T, String>
where
    C: Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let Ok(mut r) = reader.try_lock() else {
            return Err("previous read still running".into());
        };
        f(&mut r)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Starts every source: the local ones always, the others when configured.
pub fn spawn_local(state: &Shared, cfg: &Config) {
    const LIMIT: Duration = Duration::from_secs(3);
    macro_rules! source {
        ($every:expr, $limit:expr, $reader:expr, $read:path, $apply:expr) => {{
            let reader = Arc::new(Mutex::new($reader));
            spawn(state, $every, $limit, move || blocking(reader.clone(), $read), $apply);
        }};
    }
    let mount = "/";
    source!(every::HOST, LIMIT, host::Reader, host::Reader::read, |s, r, now| s.host.apply(r, now));
    source!(every::CPU, LIMIT, cpu::Reader::new(), cpu::Reader::read, |s, r, now| s.set_cpu(r, now));
    source!(every::MEMORY, LIMIT, memory::Reader, memory::Reader::read, |s, r, now| s.set_memory(r, now));
    source!(every::PROCESSES, LIMIT, processes::Reader::new(), processes::Reader::read, |s, r, now| {
        s.processes.apply(r, now)
    });
    source!(every::DISK, LIMIT, disk::Reader::new(mount), disk::Reader::read, |s, r, now| s.set_disk(r, now));
    source!(every::DISK_WEAR, Duration::from_secs(20), disk::WearReader::new(mount), disk::WearReader::read, |s, r, now| {
        s.disk_wear.apply(r, now)
    });
    source!(every::NETWORK, LIMIT, network::Reader::new(cfg.network.interfaces.first().cloned()),
        network::Reader::read, |s, r, now| s.set_network(r, now));
    source!(every::SENSORS, LIMIT, sensors::Reader::new(cfg.sensors.ignore.clone()), sensors::Reader::read,
        |s, r, now| s.sensors.apply(r, now));

    if let Some(a) = &cfg.apache {
        source!(every::APACHE, LIMIT, apache::Reader::new(a.status_url.clone()), apache::Reader::read,
            |s, r, now| s.set_apache(r, now));
    }
    if let Some(d) = &cfg.docker {
        source!(every::CONTAINERS, Duration::from_secs(10), docker::Reader::new(d.socket.clone(), d.expected.clone()),
            docker::Reader::read, |s, r, now| s.containers.apply(r, now));
    }
    if let Some(u) = &cfg.ups {
        source!(every::UPS, LIMIT, ups::Reader::new(u.address.clone()), ups::Reader::read, |s, r, now| s.ups.apply(r, now));
    }
    if let Some(sv) = &cfg.services {
        source!(every::SERVICES, Duration::from_secs(5), services::Reader::new(sv.units.clone()), services::Reader::read,
            |s, r, now| s.services.apply(r, now));
    }
    if cfg.mariadb.is_some() || cfg.postgresql.is_some() {
        let reader = databases::Reader::new(cfg.mariadb.as_ref().map(|m| m.socket.clone()), cfg.postgresql.is_some());
        source!(every::DATABASES, Duration::from_secs(5), reader, databases::Reader::read,
            |s, r, now| s.databases.apply(r, now));
    }
    if !cfg.sites.is_empty() {
        source!(every::SITES, sites::LIMIT + Duration::from_secs(5), sites::Reader::new(cfg.sites.clone()),
            sites::Reader::read, |s, r, now| s.sites.apply(r, now));
    }
    if let Some(f) = &cfg.fail2ban {
        source!(every::FAIL2BAN, Duration::from_secs(10), fail2ban::Reader::new(f.jails.clone()), fail2ban::Reader::read,
            |s, r, now| s.fail2ban.apply(r, now));
    }
}

/// Reads one source forever. A read that is still running when the next tick comes
/// makes that tick skipped, so reads never pile up. A read that takes longer than
/// `limit` is abandoned and stored as an error.
pub fn spawn<T, F, Fut, A>(state: &Shared, every: Duration, limit: Duration, mut fetch: F, apply: A)
where
    T: Send + 'static,
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<T, String>> + Send + 'static,
    A: Fn(&mut State, Result<T, String>, Instant) + Send + 'static,
{
    let state = state.clone();
    tokio::spawn(async move {
        let mut tick = interval(every);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let result = match timeout(limit, fetch()).await {
                Ok(r) => r,
                Err(_) => Err(format!("no answer within {} s", limit.as_secs())),
            };
            let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
            apply(&mut s, result, Instant::now());
        }
    });
}
