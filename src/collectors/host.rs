//! Uptime and load from `/proc/uptime` and `/proc/loadavg`.

use std::time::Duration;

use super::read;
use crate::state::Host;

pub struct Reader;

impl Reader {
    pub fn read(&mut self) -> Result<Host, String> {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        parse(&read("/proc/uptime")?, &read("/proc/loadavg")?, threads)
    }
}

pub fn parse(uptime: &str, loadavg: &str, threads: usize) -> Result<Host, String> {
    let secs: f64 = uptime
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or("/proc/uptime: nieznany format")?;
    let load: Vec<f64> = loadavg.split_whitespace().take(3).filter_map(|s| s.parse().ok()).collect();
    let [l1, l5, l15] = load[..] else {
        return Err("/proc/loadavg: nieznany format".into());
    };
    Ok(Host { uptime: Duration::from_secs_f64(secs), load: [l1, l5, l15], threads })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_uptime_and_load() {
        let h = super::parse("285120.57 3330000.12\n", "0.42 0.38 0.35 2/412 316031\n", 12).unwrap();
        assert_eq!(h.uptime.as_secs(), 285120);
        assert_eq!(h.load, [0.42, 0.38, 0.35]);
    }
}
