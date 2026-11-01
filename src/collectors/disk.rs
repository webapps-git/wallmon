//! Usage of one mount point, its disk's throughput from `/proc/diskstats`,
//! NVMe temperature from hwmon and wear from `smartctl`.

use std::ffi::CString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use super::{read, read_number};
use crate::state::Disk;

const SECTOR: u64 = 512;

pub struct Reader {
    mount: String,
    /// Whole disk behind the mount, e.g. `nvme0n1`.
    disk: Option<String>,
    model: String,
    temp: Option<PathBuf>,
    prev: Option<(u64, u64, Instant)>,
}

impl Reader {
    pub fn new(mount: &str) -> Self {
        let disk = read("/proc/self/mounts").ok().and_then(|m| mount_source(&m, mount)).and_then(|dev| whole_disk(&dev));
        let model = disk
            .as_ref()
            .and_then(|d| read(format!("/sys/block/{d}/device/model")).ok())
            .map(|m| m.trim().to_string())
            .filter(|m| !m.is_empty())
            .or_else(|| disk.clone())
            .unwrap_or_default();
        let temp = disk.as_deref().and_then(find_temp);
        Reader { mount: mount.into(), disk, model, temp, prev: None }
    }

    pub fn read(&mut self) -> Result<Disk, String> {
        let (size, used) = statvfs(&self.mount)?;
        let (mut read_rate, mut write_rate) = (0.0, 0.0);
        if let Some(disk) = &self.disk
            && let Some((r, w)) = parse_diskstats(&read("/proc/diskstats")?, disk)
        {
            let now = Instant::now();
            if let Some((pr, pw, pt)) = self.prev {
                let dt = now.duration_since(pt).as_secs_f64();
                if dt > 0.0 {
                    read_rate = r.saturating_sub(pr) as f64 / dt;
                    write_rate = w.saturating_sub(pw) as f64 / dt;
                }
            }
            self.prev = Some((r, w, now));
        }
        Ok(Disk {
            model: self.model.clone(),
            mount: self.mount.clone(),
            size,
            used,
            read: read_rate,
            write: write_rate,
            temp: self.temp.as_ref().and_then(read_number).map(|m| m / 1000.0),
        })
    }
}

/// NVMe "percentage used" from `smartctl -j -A`. Needs root; read rarely.
pub struct WearReader {
    device: Option<String>,
}

impl WearReader {
    pub fn new(mount: &str) -> Self {
        let disk = read("/proc/self/mounts").ok().and_then(|m| mount_source(&m, mount)).and_then(|dev| whole_disk(&dev));
        WearReader { device: disk.map(|d| format!("/dev/{d}")) }
    }

    pub fn read(&mut self) -> Result<f64, String> {
        let dev = self.device.as_ref().ok_or("disk not found")?;
        let out = Command::new("smartctl").args(["-j", "-A", dev]).output().map_err(|e| format!("smartctl: {e}"))?;
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("smartctl: {e}"))?;
        json["nvme_smart_health_information_log"]["percentage_used"]
            .as_f64()
            .ok_or_else(|| "smartctl: no percentage_used".into())
    }
}

fn statvfs(path: &str) -> Result<(u64, u64), String> {
    let c = CString::new(path).map_err(|e| e.to_string())?;
    // SAFETY: `c` is a valid C string and `s` is a properly sized, writable struct.
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return Err(format!("{path}: {}", std::io::Error::last_os_error()));
    }
    let unit = s.f_frsize as u64;
    #[allow(clippy::unnecessary_cast)]
    let (blocks, free) = (s.f_blocks as u64, s.f_bfree as u64);
    Ok((blocks * unit, blocks.saturating_sub(free) * unit))
}

/// Device mounted at `mount`, e.g. `/dev/nvme0n1p2`.
pub fn mount_source(mounts: &str, mount: &str) -> Option<String> {
    mounts.lines().rev().find_map(|l| {
        let mut f = l.split_whitespace();
        let (dev, at) = (f.next()?, f.next()?);
        (at == mount && dev.starts_with("/dev/")).then(|| dev.to_string())
    })
}

/// `/dev/nvme0n1p2` → `nvme0n1`: the parent in sysfs when the device is a partition.
fn whole_disk(dev: &str) -> Option<String> {
    let name = fs::canonicalize(dev).ok()?.file_name()?.to_str()?.to_string();
    let sys = PathBuf::from(format!("/sys/class/block/{name}"));
    if sys.join("partition").exists() {
        let target = fs::canonicalize(&sys).ok()?;
        return target.parent()?.file_name()?.to_str().map(String::from);
    }
    Some(name)
}

/// Bytes read and written since boot.
pub fn parse_diskstats(s: &str, disk: &str) -> Option<(u64, u64)> {
    s.lines().find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.get(2) != Some(&disk) {
            return None;
        }
        Some((f.get(5)?.parse::<u64>().ok()? * SECTOR, f.get(9)?.parse::<u64>().ok()? * SECTOR))
    })
}

fn find_temp(disk: &str) -> Option<PathBuf> {
    ["device", "device/device"].iter().find_map(|sub| {
        let dir = PathBuf::from(format!("/sys/block/{disk}/{sub}"));
        fs::read_dir(dir).ok()?.flatten().find_map(|e| {
            let p = e.path().join("temp1_input");
            (e.file_name().to_string_lossy().starts_with("hwmon") && p.exists()).then_some(p)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_root_device() {
        let m = "sysfs /sys sysfs rw 0 0\n/dev/nvme0n1p2 / ext4 rw,relatime 0 0\n/dev/nvme0n1p1 /boot/efi vfat rw 0 0\n";
        assert_eq!(mount_source(m, "/").as_deref(), Some("/dev/nvme0n1p2"));
        assert_eq!(mount_source(m, "/srv"), None);
    }

    #[test]
    fn parses_diskstats() {
        let s = " 259       0 nvme0n1 120000 300 4000000 90000 800000 9000 16000000 700000 0 400000 800000\n\
                 \x20259       1 nvme0n1p1 300 0 9000 100 2 0 10 0 0 100 100\n";
        assert_eq!(parse_diskstats(s, "nvme0n1"), Some((4_000_000 * 512, 16_000_000 * 512)));
    }
}
