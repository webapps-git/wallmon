//! MariaDB and PostgreSQL through their own command-line clients, run as root on the
//! local socket. No database accounts or roles need to be created for wallmon.

use std::collections::HashMap;
use std::process::Command;
use std::time::Instant;

use crate::state::{Databases, MariaDb, Postgres};

/// Counters that become rates: (questions, slow queries, when).
type MariaCounters = (u64, u64, Instant);

pub struct Reader {
    mariadb_socket: Option<String>,
    postgres: bool,
    prev: Option<MariaCounters>,
}

const MARIADB_SQL: &str = "SHOW GLOBAL STATUS WHERE Variable_name IN \
    ('Threads_connected','Threads_running','Questions','Slow_queries'); \
    SHOW GLOBAL VARIABLES WHERE Variable_name IN ('max_connections','version')";

/// Client connections only; our own `psql` session is subtracted afterwards.
const POSTGRES_SQL: &str = "SELECT current_setting('server_version_num')::int / 10000, \
    current_setting('max_connections'), \
    count(*) FILTER (WHERE backend_type = 'client backend'), \
    count(*) FILTER (WHERE backend_type = 'client backend' AND state = 'active'), \
    count(*) FILTER (WHERE backend_type = 'client backend' AND state LIKE 'idle%') \
    FROM pg_stat_activity";

impl Reader {
    pub fn new(mariadb_socket: Option<String>, postgres: bool) -> Self {
        Reader { mariadb_socket, postgres, prev: None }
    }

    pub fn read(&mut self) -> Result<Databases, String> {
        let mut d = Databases { mariadb: None, postgres: None, errors: vec![] };
        if let Some(socket) = self.mariadb_socket.clone() {
            match run(Command::new("mariadb").args(["--batch", "--skip-column-names", "--socket", &socket, "-e", MARIADB_SQL]))
                .and_then(|out| self.mariadb(&out))
            {
                Ok(m) => d.mariadb = Some(m),
                Err(e) => d.errors.push(format!("MariaDB: {e}")),
            }
        }
        if self.postgres {
            match run(Command::new("setpriv").args(["--reuid=postgres", "--regid=postgres", "--init-groups", "--", "psql", "-XAtq", "-c", POSTGRES_SQL]))
                .and_then(|out| parse_postgres(&out))
            {
                Ok(p) => d.postgres = Some(p),
                Err(e) => d.errors.push(format!("PostgreSQL: {e}")),
            }
        }
        if d.mariadb.is_none() && d.postgres.is_none() && !d.errors.is_empty() {
            return Err(d.errors.join("; "));
        }
        Ok(d)
    }

    fn mariadb(&mut self, out: &str) -> Result<MariaDb, String> {
        let v: HashMap<&str, &str> = out.lines().filter_map(|l| l.split_once('\t')).collect();
        let num = |k: &str| v.get(k).and_then(|s| s.trim().parse::<u64>().ok()).ok_or(format!("brak {k}"));
        let (questions, slow) = (num("Questions")?, num("Slow_queries")?);
        let now = Instant::now();
        let (qps, slow_per_min) = match self.prev {
            Some((pq, ps, at)) if now > at => {
                let dt = now.duration_since(at).as_secs_f64();
                (questions.saturating_sub(pq) as f64 / dt, slow.saturating_sub(ps) as f64 / dt * 60.0)
            }
            _ => (0.0, 0.0),
        };
        self.prev = Some((questions, slow, now));
        // Our own `mariadb` session is one connection and one running thread.
        Ok(MariaDb {
            version: short_version(v.get("version").copied().unwrap_or("")),
            connections: num("Threads_connected")?.saturating_sub(1) as u32,
            max_connections: num("max_connections")? as u32,
            threads_running: num("Threads_running")?.saturating_sub(1) as u32,
            queries_per_sec: qps,
            slow_per_min,
        })
    }
}

fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().next().unwrap_or("błąd").trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// "11.8.3-MariaDB-0+deb13u1" → "11.8".
fn short_version(v: &str) -> String {
    v.split(['.', '-']).take(2).collect::<Vec<_>>().join(".")
}

pub fn parse_postgres(out: &str) -> Result<Postgres, String> {
    let f: Vec<u32> = out.trim().split('|').filter_map(|v| v.trim().parse().ok()).collect();
    let [version, max, total, active, idle] = f[..] else {
        return Err(format!("nieznany wynik: {}", out.trim()));
    };
    Ok(Postgres {
        version: version.to_string(),
        connections: total.saturating_sub(1),
        max_connections: max,
        active: active.saturating_sub(1),
        idle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_mariadb_status() {
        let mut r = Reader::new(Some("/run/mysqld/mysqld.sock".into()), false);
        let out = "Questions\t1000\nSlow_queries\t37\nThreads_connected\t9\nThreads_running\t2\n\
                   max_connections\t151\nversion\t11.8.3-MariaDB-0+deb13u1\n";
        let m = r.mariadb(out).unwrap();
        assert_eq!((m.version.as_str(), m.connections, m.max_connections, m.threads_running), ("11.8", 8, 151, 1));
        assert_eq!(m.queries_per_sec, 0.0);
        assert!(r.mariadb("Questions\t1\n").is_err());
    }

    #[test]
    fn reads_postgres_activity() {
        let p = parse_postgres("17|100|8|2|6\n").unwrap();
        assert_eq!((p.version.as_str(), p.connections, p.max_connections, p.active, p.idle), ("17", 7, 100, 1, 6));
        assert!(parse_postgres("psql: error").is_err());
    }
}
