//! The whole screen, redrawn from the shared state once a second.

mod format;
mod paint;
mod panels;

use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;

use crate::config::Config;
use crate::state::{Reading, State};
use paint::{Panel, Tone, frame};

pub use paint::Look;

/// Smaller terminals get a short message instead of a broken layout.
pub const MIN_WIDTH: u16 = 100;
pub const MIN_HEIGHT: u16 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Id {
    Cpu,
    Processes,
    Apache,
    Containers,
    Ups,
    Memory,
    Disk,
    Network,
    Sensors,
    Services,
    Sites,
    Databases,
    Fail2ban,
}

/// Rows of panels with their share of the height, and each panel's share of the row width.
const BANDS: &[(f64, &[(Id, f64)])] = &[
    (0.25, &[(Id::Cpu, 0.5), (Id::Processes, 0.5)]),
    (0.234, &[(Id::Apache, 0.4), (Id::Containers, 0.3), (Id::Ups, 0.3)]),
    (0.234, &[(Id::Memory, 0.25), (Id::Disk, 0.25), (Id::Network, 0.25), (Id::Sensors, 0.25)]),
    (0.282, &[(Id::Services, 0.2167), (Id::Sites, 0.3167), (Id::Databases, 0.2333), (Id::Fail2ban, 0.2333)]),
];

/// Panels for sources that are neither configured nor detected are left out.
fn shown(id: Id, cfg: &Config) -> bool {
    match id {
        Id::Apache => cfg.apache.is_some(),
        Id::Containers => cfg.docker.is_some(),
        Id::Ups => cfg.ups.is_some(),
        Id::Services => cfg.services.is_some(),
        Id::Sites => !cfg.sites.is_empty(),
        Id::Databases => cfg.mariadb.is_some() || cfg.postgresql.is_some(),
        Id::Fail2ban => cfg.fail2ban.is_some(),
        Id::Cpu | Id::Processes | Id::Memory | Id::Disk | Id::Network | Id::Sensors => true,
    }
}

/// Splits `total` cells by weights; rounding leftovers go to the last part.
fn split(total: u16, weights: &[f64]) -> Vec<u16> {
    let sum: f64 = weights.iter().sum();
    let mut parts: Vec<u16> = weights.iter().map(|w| (total as f64 * w / sum).round() as u16).collect();
    if let Some((last, rest)) = parts.split_last_mut() {
        *last = total.saturating_sub(rest.iter().sum());
    }
    parts
}

pub fn draw(f: &mut Frame, st: &State, cfg: &Config, now: Instant, local: DateTime<Local>, look: Look) {
    let area = f.area();
    let buf = f.buffer_mut();
    buf.set_style(area, Style::default().bg(look.background()));

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = format!("Terminal too small: {}x{}, needs {MIN_WIDTH}x{MIN_HEIGHT}", area.width, area.height);
        buf.set_stringn(area.x, area.y, msg, area.width as usize, Style::default().fg(Tone::Warn.color(look)));
        return;
    }

    let head = Rect { height: 3, ..area };
    let mut p = frame(buf, head, &st.hostname, None, look);
    panels::header(&mut p, st.host.value.as_ref(), &local);

    let bands: Vec<(f64, Vec<(Id, f64)>)> = BANDS
        .iter()
        .map(|(h, ps)| (*h, ps.iter().copied().filter(|(id, _)| shown(*id, cfg)).collect::<Vec<_>>()))
        .filter(|(_, ps)| !ps.is_empty())
        .collect();
    let heights = split(area.height - 3, &bands.iter().map(|b| b.0).collect::<Vec<_>>());
    let mut y = area.y + 3;
    for ((_, ps), h) in bands.iter().zip(heights) {
        let widths = split(area.width, &ps.iter().map(|p| p.1).collect::<Vec<_>>());
        let mut x = area.x;
        for ((id, _), w) in ps.iter().zip(widths) {
            draw_panel(buf, Rect::new(x, y, w, h), *id, st, now, look);
            x += w;
        }
        y += h;
    }
}

fn draw_panel(buf: &mut Buffer, r: Rect, id: Id, st: &State, now: Instant, look: Look) {
    match id {
        Id::Cpu => show(buf, r, "CPU", &st.cpu, now, look, |p, v| panels::cpu(p, v, &st.cpu_history)),
        Id::Processes => show(buf, r, "Processes", &st.processes, now, look, |p, v| panels::processes(p, v)),
        Id::Apache => show(buf, r, "Apache", &st.apache, now, look, |p, v| panels::apache(p, v, &st.apache_history)),
        Id::Containers => show(buf, r, "Containers", &st.containers, now, look, |p, v| panels::containers(p, v)),
        Id::Ups => show(buf, r, "UPS", &st.ups, now, look, panels::ups),
        Id::Memory => show(buf, r, "Memory", &st.memory, now, look, |p, v| panels::memory(p, v, &st.memory_history)),
        Id::Disk => show(buf, r, "Disk", &st.disk, now, look, |p, v| {
            panels::disk(p, v, &st.disk_read_history, &st.disk_write_history, st.disk_wear.value)
        }),
        Id::Network => show(buf, r, "Network", &st.network, now, look, |p, v| {
            panels::network(p, v, &st.rx_history, &st.tx_history)
        }),
        Id::Sensors => show(buf, r, "Sensors", &st.sensors, now, look, panels::sensors),
        Id::Services => show(buf, r, "Services", &st.services, now, look, panels::services),
        Id::Sites => show(buf, r, "Sites", &st.sites, now, look, |p, v| panels::sites(p, v, st.sites.age(now))),
        Id::Databases => show(buf, r, "Databases", &st.databases, now, look, panels::databases),
        Id::Fail2ban => show(buf, r, "fail2ban", &st.fail2ban, now, look, |p, v| panels::fail2ban(p, v)),
    }
}

/// Frame plus contents. Old data is greyed out and its age goes into the title;
/// a source that never answered shows "no data".
fn show<T>(buf: &mut Buffer, r: Rect, title: &str, reading: &Reading<T>, now: Instant, look: Look, body: impl FnOnce(&mut Panel, &T)) {
    let stale: Option<Duration> = reading.value.as_ref().and(reading.stale_for(now));
    let note = stale.map(|age| format!("data {} old", format::age(age)));
    let mut p = frame(buf, r, title, note.as_deref(), look);
    match &reading.value {
        Some(v) => body(&mut p, v),
        None => {
            p.put(0, 0, "no data", Tone::NoData);
            if let Some(e) = &reading.error {
                p.put(0, 1, e, Tone::NoData);
            }
        }
    }
}
