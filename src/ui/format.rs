//! Numbers, sizes and times written the way the screen shows them (Polish).

use std::time::Duration;

const MIB: f64 = 1024.0 * 1024.0;
const GIB: f64 = MIB * 1024.0;

pub const WEEKDAYS: [&str; 7] = ["poniedziałek", "wtorek", "środa", "czwartek", "piątek", "sobota", "niedziela"];
pub const MONTHS: [&str; 12] = [
    "stycznia", "lutego", "marca", "kwietnia", "maja", "czerwca",
    "lipca", "sierpnia", "września", "października", "listopada", "grudnia",
];

pub fn gb(bytes: u64) -> f64 {
    bytes as f64 / GIB
}

pub fn bytes(b: u64) -> String {
    let mb = b as f64 / MIB;
    if mb < 1000.0 { format!("{mb:.0} MB") } else { format!("{:.1} GB", mb / 1024.0) }
}

/// Bytes per second, for disks.
pub fn byte_rate(bps: f64) -> String {
    if bps >= MIB { format!("{:.1} MB/s", bps / MIB) } else { format!("{:.0} kB/s", bps / 1024.0) }
}

/// Bytes per second shown as bits, for networks.
pub fn bit_rate(bps: f64) -> String {
    let b = bps * 8.0;
    if b >= 1e9 {
        format!("{:.2} Gb/s", b / 1e9)
    } else if b >= 100e6 {
        format!("{:.0} Mb/s", b / 1e6)
    } else if b >= 1e6 {
        format!("{:.1} Mb/s", b / 1e6)
    } else {
        format!("{:.0} kb/s", b / 1e3)
    }
}

pub fn age(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min", s / 60)
    } else {
        format!("{} h", s / 3600)
    }
}

pub fn uptime(d: Duration) -> String {
    let s = d.as_secs();
    let (days, hours, mins) = (s / 86400, s / 3600 % 24, s / 60 % 60);
    if days > 0 {
        format!("{days} {} {hours} h {mins} min", if days == 1 { "dzień" } else { "dni" })
    } else {
        format!("{hours} h {mins} min")
    }
}

/// Polish plural form: 1 wątek, 2 wątki, 5 wątków.
pub fn plural<'a>(n: usize, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    if n == 1 {
        one
    } else if (2..=4).contains(&(n % 10)) && !(12..=14).contains(&(n % 100)) {
        few
    } else {
        many
    }
}
