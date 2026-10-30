//! Fans and board temperatures from hwmon. CPU and disk sensors have their own panels.

use std::fs;
use std::path::{Path, PathBuf};

use super::{read, read_number};
use crate::state::{Fan, Sensors};

/// Chips shown elsewhere: CPU package and disks.
const ELSEWHERE: [&str; 5] = ["coretemp", "k10temp", "zenpower", "nvme", "drivetemp"];

pub struct Reader {
    ignore: Vec<String>,
}

impl Reader {
    pub fn new(ignore: Vec<String>) -> Self {
        Reader { ignore }
    }

    pub fn read(&mut self) -> Result<Sensors, String> {
        let mut dirs: Vec<PathBuf> = fs::read_dir("/sys/class/hwmon")
            .map_err(|e| format!("/sys/class/hwmon: {e}"))?
            .flatten()
            .map(|e| e.path())
            .collect();
        dirs.sort_by_key(|d| hwmon_number(d));
        let mut chips = vec![];
        let mut fan = None;
        let mut temps = vec![];
        for dir in dirs {
            let chip = read(dir.join("name")).unwrap_or_default().trim().to_string();
            if chip.is_empty() || ELSEWHERE.contains(&chip.as_str()) {
                continue;
            }
            let mut found = false;
            let chip_temps: Vec<(String, f64)> = indexed(&dir, "temp", "_input")
                .into_iter()
                .filter(|i| !self.ignore.contains(&format!("{chip}/temp{i}")))
                .filter_map(|i| {
                    let value = read_number(dir.join(format!("temp{i}_input")))? / 1000.0;
                    let label = read(dir.join(format!("temp{i}_label"))).map(|l| l.trim().to_string()).ok();
                    Some((label.unwrap_or_else(|| format!("temp{i}")), value))
                })
                .collect();
            if chip_temps.len() == 1 {
                // A chip with a single sensor is named after the chip ("acpitz").
                temps.push((chip.clone(), chip_temps[0].1));
            } else {
                temps.extend(chip_temps.iter().cloned());
            }
            found |= !chip_temps.is_empty();
            for i in indexed(&dir, "fan", "_input") {
                if fan.is_some() || self.ignore.contains(&format!("{chip}/fan{i}")) {
                    continue;
                }
                if let Some(rpm) = read_number(dir.join(format!("fan{i}_input"))) {
                    let max = read_number(dir.join(format!("fan{i}_max"))).filter(|m| *m > 0.0).map(|m| m as u32);
                    fan = Some(Fan { rpm: rpm as u32, max });
                    found = true;
                }
            }
            if found && !chips.contains(&chip) {
                chips.push(chip);
            }
        }
        if fan.is_none() && temps.is_empty() {
            return Err("brak czujników".into());
        }
        Ok(Sensors { source: chips.join(" · "), fan, temps })
    }
}

fn hwmon_number(dir: &Path) -> u32 {
    dir.file_name().and_then(|n| n.to_str()?.strip_prefix("hwmon")?.parse().ok()).unwrap_or(u32::MAX)
}

/// Numbers N of files named `{prefix}N{suffix}` in `dir`, sorted.
fn indexed(dir: &Path, prefix: &str, suffix: &str) -> Vec<u32> {
    let mut v: Vec<u32> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str()?.strip_prefix(prefix)?.strip_suffix(suffix)?.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    v.sort_unstable();
    v
}
