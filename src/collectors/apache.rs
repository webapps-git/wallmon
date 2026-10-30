//! Apache workers and request rate from `mod_status` (`server-status?auto`).

use std::time::{Duration, Instant};

use super::http;
use crate::state::Apache;

pub struct Reader {
    url: String,
    prev: Option<(u64, Instant)>,
}

/// The fields of `server-status?auto` we use.
#[derive(Debug, PartialEq)]
pub struct Status {
    pub mpm: String,
    pub busy: u32,
    pub idle: u32,
    /// Worker slots; for prefork this is MaxRequestWorkers.
    pub slots: u32,
    pub total_accesses: u64,
}

impl Reader {
    pub fn new(url: String) -> Self {
        Reader { url, prev: None }
    }

    pub fn read(&mut self) -> Result<Apache, String> {
        let (code, body) = http::get(&self.url, Duration::from_secs(2))?;
        if code != 200 {
            return Err(format!("server-status: HTTP {code}"));
        }
        let s = parse(&body).ok_or("server-status: nieznany format")?;
        let now = Instant::now();
        let rate = self.prev.map_or(0.0, |(total, at)| {
            let dt = now.duration_since(at).as_secs_f64();
            if dt > 0.0 { s.total_accesses.saturating_sub(total) as f64 / dt } else { 0.0 }
        });
        self.prev = Some((s.total_accesses, now));
        Ok(Apache { mpm: s.mpm, busy: s.busy, idle: s.idle, max: s.slots, requests_per_sec: rate })
    }
}

pub fn parse(body: &str) -> Option<Status> {
    let field = |key: &str| body.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':').map(str::trim));
    let busy: u32 = field("BusyWorkers")?.parse().ok()?;
    let idle: u32 = field("IdleWorkers")?.parse().ok()?;
    let slots = field("Scoreboard").map_or(0, |s| s.chars().count() as u32).max(busy + idle);
    Some(Status {
        mpm: field("ServerMPM").unwrap_or("").to_string(),
        busy,
        idle,
        slots,
        total_accesses: field("Total Accesses").and_then(|v| v.parse().ok()).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_status() {
        let body = "127.0.0.1\nServerVersion: Apache/2.4.69 (Debian) OpenSSL/3.5.7\nServerMPM: prefork\n\
                    Total Accesses: 95380\nTotal kBytes: 792661\nBusyWorkers: 4\nGracefulWorkers: 0\nIdleWorkers: 6\n\
                    Scoreboard: _.K_W_...K._.K........_..._.............\nTLSSessionCacheStatus\nCacheType: SHMCB\n";
        let s = super::parse(body).unwrap();
        assert_eq!((s.mpm.as_str(), s.busy, s.idle, s.slots, s.total_accesses), ("prefork", 4, 6, 40, 95380));
    }
}
