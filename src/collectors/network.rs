//! Traffic of one interface from `/proc/net/dev`, TCP connections from `/proc/net`.

use std::ffi::CStr;
use std::net::Ipv4Addr;
use std::time::Instant;

use super::read;
use crate::state::Network;

pub struct Reader {
    interface: Option<String>,
    prev: Option<(u64, u64, Instant)>,
}

impl Reader {
    /// Without a configured interface, the one with the default route is used.
    pub fn new(interface: Option<String>) -> Self {
        Reader { interface, prev: None }
    }

    pub fn read(&mut self) -> Result<Network, String> {
        if self.interface.is_none() {
            self.interface = read("/proc/net/route").ok().and_then(|r| default_route(&r));
        }
        let iface = self.interface.clone().ok_or("nie znaleziono interfejsu z trasą domyślną")?;
        let (rx_total, tx_total) =
            parse_dev(&read("/proc/net/dev")?, &iface).ok_or(format!("brak interfejsu {iface}"))?;
        let now = Instant::now();
        let (mut rx, mut tx) = (0.0, 0.0);
        if let Some((prx, ptx, pt)) = self.prev {
            let dt = now.duration_since(pt).as_secs_f64();
            if dt > 0.0 {
                rx = rx_total.saturating_sub(prx) as f64 / dt;
                tx = tx_total.saturating_sub(ptx) as f64 / dt;
            }
        }
        self.prev = Some((rx_total, tx_total, now));
        let inuse = |path: &str, key: &str| read(path).ok().and_then(|s| sockstat_inuse(&s, key)).unwrap_or(0);
        Ok(Network {
            address: ipv4(&iface).unwrap_or_default(),
            interface: iface,
            rx,
            tx,
            tcp: inuse("/proc/net/sockstat", "TCP:") + inuse("/proc/net/sockstat6", "TCP6:"),
            established: read("/proc/net/snmp").ok().and_then(|s| snmp_established(&s)).unwrap_or(0),
        })
    }
}

/// Bytes received and sent since boot.
pub fn parse_dev(s: &str, iface: &str) -> Option<(u64, u64)> {
    s.lines().find_map(|l| {
        let (name, rest) = l.split_once(':')?;
        if name.trim() != iface {
            return None;
        }
        let f: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        Some((*f.first()?, *f.get(8)?))
    })
}

pub fn default_route(s: &str) -> Option<String> {
    s.lines().skip(1).find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        (f.get(1) == Some(&"00000000")).then(|| f[0].to_string())
    })
}

fn sockstat_inuse(s: &str, key: &str) -> Option<u32> {
    let line = s.lines().find(|l| l.starts_with(key))?;
    let f: Vec<&str> = line.split_whitespace().collect();
    f.iter().position(|w| *w == "inuse").and_then(|i| f.get(i + 1)?.parse().ok())
}

/// `CurrEstab` from the two `Tcp:` lines (names, then values).
pub fn snmp_established(s: &str) -> Option<u32> {
    let mut tcp = s.lines().filter(|l| l.starts_with("Tcp:"));
    let (names, values) = (tcp.next()?, tcp.next()?);
    let i = names.split_whitespace().position(|n| n == "CurrEstab")?;
    values.split_whitespace().nth(i)?.parse().ok()
}

fn ipv4(iface: &str) -> Option<String> {
    let mut found = None;
    // SAFETY: getifaddrs fills a linked list that we only read and then free with freeifaddrs.
    unsafe {
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) != 0 {
            return None;
        }
        let mut p = list;
        while !p.is_null() {
            let ifa = &*p;
            if !ifa.ifa_addr.is_null()
                && i32::from((*ifa.ifa_addr).sa_family) == libc::AF_INET
                && CStr::from_ptr(ifa.ifa_name).to_str() == Ok(iface)
            {
                let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                found = Some(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)).to_string());
                break;
            }
            p = ifa.ifa_next;
        }
        libc::freeifaddrs(list);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_interface_counters() {
        let s = "Inter-|   Receive    |  Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes\n\
                 \x20   lo: 100 1 0 0 0 0 0 0 100 1 0 0 0 0 0 0\n enp2s0: 5000 40 0 0 0 0 0 0 9000 50 0 0 0 0 0 0\n";
        assert_eq!(parse_dev(s, "enp2s0"), Some((5000, 9000)));
        assert_eq!(parse_dev(s, "eth9"), None);
    }

    #[test]
    fn finds_default_route_and_tcp_counts() {
        let r = "Iface\tDestination\tGateway\nbr-1\t0012A8C0\t00000000\nenp2s0\t00000000\t0197A4BC\n";
        assert_eq!(default_route(r).as_deref(), Some("enp2s0"));
        let snmp = "Tcp: RtoAlgorithm RtoMin RtoMax MaxConn ActiveOpens PassiveOpens AttemptFails EstabResets CurrEstab\n\
                    Tcp: 1 200 120000 -1 10 20 0 0 87\n";
        assert_eq!(snmp_established(snmp), Some(87));
        assert_eq!(sockstat_inuse("sockets: used 300\nTCP: inuse 25 orphan 0 tw 3 alloc 30 mem 1\n", "TCP:"), Some(25));
    }
}
