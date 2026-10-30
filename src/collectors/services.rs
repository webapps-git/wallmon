//! State of the listed systemd units and the number of failed units, via `systemctl`.

use std::process::Command;

use crate::state::{Services, UnitState};

pub struct Reader {
    units: Vec<String>,
}

impl Reader {
    pub fn new(units: Vec<String>) -> Self {
        Reader { units }
    }

    pub fn read(&mut self) -> Result<Services, String> {
        let shown = run(Command::new("systemctl").args(["show", "--property=Id,ActiveState", "--"]).args(&self.units))?;
        let failed = run(Command::new("systemctl").args(["list-units", "--state=failed", "--no-legend", "--plain"]))?;
        Ok(Services {
            failed: failed.lines().filter(|l| !l.trim().is_empty()).count() as u32,
            units: self.units.iter().cloned().zip(parse_show(&shown, self.units.len())).collect(),
        })
    }
}

fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("systemctl: {e}"))?;
    if !out.status.success() {
        return Err(format!("systemctl: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `systemctl show` prints one block per unit, in the order given, separated by blank lines.
pub fn parse_show(s: &str, n: usize) -> Vec<UnitState> {
    let mut states: Vec<UnitState> = s
        .split("\n\n")
        .filter(|b| !b.trim().is_empty())
        .map(|b| {
            match b.lines().find_map(|l| l.strip_prefix("ActiveState=")) {
                Some("active" | "reloading") => UnitState::Active,
                Some("activating" | "deactivating") => UnitState::Activating,
                Some("failed") => UnitState::Failed,
                _ => UnitState::Inactive,
            }
        })
        .collect();
    states.resize(n, UnitState::Inactive);
    states
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unit_blocks() {
        let s = "Id=apache2.service\nActiveState=active\n\nActiveState=failed\nId=monit.service\n\n\
                 Id=nope.service\nActiveState=inactive\n";
        assert_eq!(parse_show(s, 3), [UnitState::Active, UnitState::Failed, UnitState::Inactive]);
    }
}
