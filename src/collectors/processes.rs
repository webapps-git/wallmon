//! Top processes by CPU from `/proc/<pid>/stat`; user and command line only for the top ones.

use std::collections::HashMap;
use std::fs;
use std::time::Instant;

use super::read;
use crate::state::Process;

const TOP: usize = 10;

pub struct Reader {
    ticks_per_sec: f64,
    page_size: u64,
    users: HashMap<u32, String>,
    prev: HashMap<u32, u64>,
    prev_at: Option<Instant>,
}

/// The parts of `/proc/<pid>/stat` we need.
#[derive(Debug, PartialEq)]
pub struct Stat {
    pub comm: String,
    pub ticks: u64,
    pub rss_pages: u64,
}

impl Reader {
    pub fn new() -> Self {
        // SAFETY: sysconf only reads system configuration.
        let (hz, page) = unsafe { (libc::sysconf(libc::_SC_CLK_TCK), libc::sysconf(libc::_SC_PAGESIZE)) };
        Reader {
            ticks_per_sec: if hz > 0 { hz as f64 } else { 100.0 },
            page_size: if page > 0 { page as u64 } else { 4096 },
            users: read("/etc/passwd").map(|s| parse_passwd(&s)).unwrap_or_default(),
            prev: HashMap::new(),
            prev_at: None,
        }
    }

    pub fn read(&mut self) -> Result<Vec<Process>, String> {
        let now = Instant::now();
        let dt = self.prev_at.map_or(0.0, |t| now.duration_since(t).as_secs_f64());
        let mut seen = HashMap::new();
        let mut all = vec![];
        for entry in fs::read_dir("/proc").map_err(|e| format!("/proc: {e}"))?.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            // A process may end between listing and reading; skip it.
            let Some(stat) = read(format!("/proc/{pid}/stat")).ok().and_then(|s| parse_stat(&s)) else {
                continue;
            };
            let cpu = match self.prev.get(&pid) {
                Some(&p) if dt > 0.0 => stat.ticks.saturating_sub(p) as f64 / self.ticks_per_sec / dt * 100.0,
                _ => 0.0,
            };
            seen.insert(pid, stat.ticks);
            all.push((pid, cpu, stat));
        }
        self.prev = seen;
        self.prev_at = Some(now);

        all.sort_by(|a, b| b.1.total_cmp(&a.1).then(b.2.rss_pages.cmp(&a.2.rss_pages)));
        Ok(all
            .into_iter()
            .take(TOP)
            .map(|(pid, cpu, stat)| {
                let uid = read(format!("/proc/{pid}/status")).ok().and_then(|s| parse_uid(&s));
                let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
                Process {
                    pid,
                    user: uid.map_or("?".into(), |u| self.users.get(&u).cloned().unwrap_or_else(|| u.to_string())),
                    cpu,
                    memory: stat.rss_pages * self.page_size,
                    args: args(&stat.comm, &cmdline),
                    name: stat.comm,
                }
            })
            .collect())
    }
}

impl Default for Reader {
    fn default() -> Self {
        Self::new()
    }
}

pub fn parse_stat(s: &str) -> Option<Stat> {
    // The name is in parentheses and may itself contain spaces and parentheses.
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    let comm = s.get(open + 1..close)?.to_string();
    let f: Vec<&str> = s.get(close + 2..)?.split_whitespace().collect();
    let num = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok());
    Some(Stat { comm, ticks: num(11)? + num(12)?, rss_pages: num(21)? })
}

fn parse_uid(status: &str) -> Option<u32> {
    status.lines().find_map(|l| l.strip_prefix("Uid:"))?.split_whitespace().next()?.parse().ok()
}

fn parse_passwd(s: &str) -> HashMap<u32, String> {
    s.lines()
        .filter_map(|l| {
            let mut f = l.split(':');
            let name = f.next()?;
            Some((f.nth(1)?.parse().ok()?, name.to_string()))
        })
        .collect()
}

/// Arguments to show after the name: the command line without the program itself.
pub fn args(comm: &str, cmdline: &[u8]) -> String {
    let parts: Vec<String> = cmdline
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    let Some((first, rest)) = parts.split_first() else {
        return String::new();
    };
    // Some programs rewrite their whole command line ("postgres: 17/main: checkpointer").
    let program = if first.contains(' ') { first } else { first.rsplit('/').next().unwrap_or(first) };
    let full = std::iter::once(program.to_string()).chain(rest.iter().cloned()).collect::<Vec<_>>().join(" ");
    match full.strip_prefix(comm) {
        Some(r) => r.trim_start_matches([':', ' ']).trim_end().to_string(),
        None => full.trim_end().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stat_with_odd_names() {
        let s = "1116 (mariadbd) S 1 1116 1116 0 -1 4194560 1 0 0 0 300 120 0 0 20 0 30 0 1500 \
                 2000000000 124000 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0";
        assert_eq!(parse_stat(s), Some(Stat { comm: "mariadbd".into(), ticks: 420, rss_pages: 124000 }));
        let odd = "42 (a (b) c) R 1 1 1 0 -1 0 0 0 0 0 7 3 0 0 20 0 1 0 1 1 9 0";
        assert_eq!(parse_stat(odd).unwrap().comm, "a (b) c");
    }

    #[test]
    fn strips_program_from_arguments() {
        assert_eq!(args("apache2", b"/usr/sbin/apache2\0-k\0start\0"), "-k start");
        assert_eq!(args("postgres", b"postgres: 17/main: checkpointer \0"), "17/main: checkpointer");
        assert_eq!(args("mariadbd", b"/usr/sbin/mariadbd\0"), "");
        assert_eq!(args("kworker/0:1", b""), "");
    }

    #[test]
    fn reads_users() {
        let u = parse_passwd("root:x:0:0:root:/root:/bin/bash\nwww-data:x:33:33::/var/www:/usr/sbin/nologin\n");
        assert_eq!(u[&33], "www-data");
        assert_eq!(parse_uid("Name:\tbash\nUid:\t33\t33\t33\t33\n"), Some(33));
    }
}
