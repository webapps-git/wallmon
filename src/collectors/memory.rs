//! RAM and swap from `/proc/meminfo`.

use std::collections::HashMap;

use super::read;
use crate::state::Memory;

pub struct Reader;

impl Reader {
    pub fn read(&mut self) -> Result<Memory, String> {
        parse(&read("/proc/meminfo")?)
    }
}

pub fn parse(s: &str) -> Result<Memory, String> {
    let kb: HashMap<&str, u64> = s
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            Some((k, v.split_whitespace().next()?.parse().ok()?))
        })
        .collect();
    let get = |k: &str| kb.get(k).copied().map(|v| v * 1024).ok_or(format!("/proc/meminfo: brak {k}"));
    let total = get("MemTotal")?;
    let available = get("MemAvailable")?;
    let cache = get("Buffers")? + get("Cached")? + get("SReclaimable").unwrap_or(0);
    Ok(Memory {
        total,
        used: total.saturating_sub(available),
        cache,
        free: get("MemFree")?,
        swap_total: get("SwapTotal")?,
        swap_used: get("SwapTotal")?.saturating_sub(get("SwapFree")?),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_meminfo() {
        let m = super::parse(
            "MemTotal:       16000000 kB\nMemFree:         1000000 kB\nMemAvailable:   11000000 kB\n\
             Buffers:          500000 kB\nCached:          8000000 kB\nSReclaimable:     700000 kB\n\
             SwapTotal:       4000000 kB\nSwapFree:        3000000 kB\n",
        )
        .unwrap();
        assert_eq!(m.used, 5_000_000 * 1024);
        assert_eq!(m.cache, 9_200_000 * 1024);
        assert_eq!(m.swap_used, 1_000_000 * 1024);
    }
}
