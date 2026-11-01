//! Currently banned addresses per jail, via `fail2ban-client` (its socket speaks a Python
//! protocol, so the client is the practical way in).

use std::process::Command;

use crate::state::Jail;

pub struct Reader {
    /// Jails to show, in this order; empty means all.
    jails: Vec<String>,
}

impl Reader {
    pub fn new(jails: Vec<String>) -> Self {
        Reader { jails }
    }

    pub fn read(&mut self) -> Result<Vec<Jail>, String> {
        // One process for all jails; older fail2ban without `banned` gets one call per jail.
        let all = match run(&["banned"]).ok().and_then(|out| parse_banned(&out)) {
            Some(all) => all,
            None => {
                let list = parse_jail_list(&run(&["status"])?).ok_or("fail2ban-client: no jail list")?;
                list.into_iter()
                    .map(|name| {
                        let banned = run(&["status", &name]).ok().and_then(|s| parse_currently_banned(&s)).unwrap_or(0);
                        Jail { name, banned }
                    })
                    .collect()
            }
        };
        if self.jails.is_empty() {
            return Ok(all);
        }
        Ok(self.jails.iter().filter_map(|name| all.iter().find(|j| &j.name == name).cloned()).collect())
    }
}

fn run(args: &[&str]) -> Result<String, String> {
    let out = Command::new("fail2ban-client").args(args).output().map_err(|e| format!("fail2ban-client: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("fail2ban-client: {}", err.lines().next().unwrap_or("error").trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `fail2ban-client banned` prints a Python list: `[{'sshd': ['192.0.2.1']}, {'apache-auth': []}]`.
pub fn parse_banned(s: &str) -> Option<Vec<Jail>> {
    let s = s.trim();
    if !s.starts_with('[') {
        return None;
    }
    let mut jails = vec![];
    let mut rest = s;
    while let Some(start) = rest.find("{'") {
        let after = &rest[start + 2..];
        let end = after.find("':")?;
        let name = after[..end].to_string();
        let list_start = after[end..].find('[')? + end;
        let list_end = after[list_start..].find(']')? + list_start;
        let banned = after[list_start + 1..list_end].matches('\'').count() as u32 / 2;
        jails.push(Jail { name, banned });
        rest = &after[list_end..];
    }
    Some(jails)
}

pub fn parse_jail_list(s: &str) -> Option<Vec<String>> {
    let line = s.lines().find_map(|l| l.split_once("Jail list:").map(|(_, r)| r))?;
    Some(line.split(',').map(|j| j.trim().to_string()).filter(|j| !j.is_empty()).collect())
}

pub fn parse_currently_banned(s: &str) -> Option<u32> {
    s.lines().find_map(|l| l.split_once("Currently banned:").map(|(_, r)| r.trim().parse().ok()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_banned_list() {
        let j = parse_banned("[{'apache-auth': ['192.0.2.1', '198.51.100.7']}, {'apache-denied': []}]\n").unwrap();
        assert_eq!((j[0].name.as_str(), j[0].banned), ("apache-auth", 2));
        assert_eq!((j[1].name.as_str(), j[1].banned), ("apache-denied", 0));
        assert!(parse_banned("Sorry but the command is invalid").is_none());
    }

    #[test]
    fn parses_status_output() {
        let s = "Status\n|- Number of jail:\t2\n`- Jail list:\tapache-auth, apache-wp-login\n";
        assert_eq!(parse_jail_list(s).unwrap(), ["apache-auth", "apache-wp-login"]);
        let j = "Status for the jail: apache-auth\n|- Filter\n|  |- Currently failed:\t0\n`- Actions\n   |- Currently banned:\t3\n   |- Total banned:\t9\n";
        assert_eq!(parse_currently_banned(j), Some(3));
    }
}
