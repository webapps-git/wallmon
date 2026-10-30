//! Contents of every panel. Each one adapts to the size of its frame.

use std::time::Duration;

use chrono::{DateTime, Datelike, Local};

use super::format::{self, plural};
use super::paint::{GraphTone, Item, Panel, Tone::{self, *}, fit, hue, lvl, on, pick, resample, t, width};
use crate::state::*;

/// Rows where `None` is a spacer that is dropped first when space runs out.
fn rows_out(p: &mut Panel, rows: Vec<Option<Vec<Item>>>) {
    let rows: Vec<_> = if rows.len() > p.h { rows.into_iter().filter(Option::is_some).collect() } else { rows };
    for (y, r) in rows.iter().enumerate().take(p.h) {
        if let Some(r) = r {
            p.row(y, 0, None, r);
        }
    }
}

/// Lists on screen are alphabetical, whatever the order in the configuration.
fn by_name<'a, T>(items: impl IntoIterator<Item = &'a T>, name: impl Fn(&T) -> &str) -> Vec<&'a T> {
    let mut v: Vec<_> = items.into_iter().collect();
    v.sort_by_cached_key(|i| name(i).to_lowercase());
    v
}

pub fn header(p: &mut Panel, host: Option<&Host>, now: &DateTime<Local>) {
    let date = format!(
        "{}, {} {} {}",
        format::WEEKDAYS[now.weekday().num_days_from_monday() as usize],
        now.day(),
        format::MONTHS[now.month0() as usize],
        now.year()
    );
    let mut items = vec![t(date + "  ", Text), t(now.format("%H:%M:%S").to_string(), Bright)];
    match host {
        Some(h) => {
            items.push(t("     uptime ", Label));
            items.push(t(format::uptime(h.uptime), Text));
            items.push(t("     load ", Label));
            for (i, v) in h.load.iter().enumerate() {
                let per_thread = v / h.threads.max(1) as f64;
                items.push(t(format!("{}{v:.2}", if i == 0 { "" } else { "  " }), lvl(per_thread, 0.7, 1.0)));
            }
        }
        None => items.push(t("     brak danych o systemie", NoData)),
    }
    p.row(0, 0, None, &items);
}

pub fn cpu(p: &mut Panel, c: &Cpu, history: &History) {
    let n = c.cores.len();
    p.title_right(&format!("{} · {n} {}", c.model, plural(n, "wątek", "wątki", "wątków")));
    let cw = 46.min(p.w * 42 / 100);
    let gw = p.w.saturating_sub(cw + 3);
    p.row(0, 0, None, &[
        t("użycie ", Label),
        t(format!("{:.0} %", c.usage), lvl(c.usage, 60.0, 85.0)),
        t("   ostatnie 5 min", Label),
    ]);
    let gh = p.h.saturating_sub(1);
    p.graph(0, 1, gw, gh, &resample(history, gw * 2, 100.0), GraphTone::Level(hue::CPU));

    let x0 = gw + 3;
    let mut info = vec![];
    if let Some(temp) = c.temp {
        info.push(t("temp. ", Label));
        info.push(t(format!("{temp:.0} °C"), lvl(temp, 70.0, 85.0)));
    }
    if let Some(watts) = c.power {
        info.push(t(if info.is_empty() { "moc " } else { "   moc " }, Label));
        info.push(t(format!("{watts:.0} W"), Text));
    }
    p.row(0, x0, Some(cw), &info);
    if n == 0 || gh == 0 {
        return;
    }
    let ncols = n.div_ceil(gh);
    let per = n.div_ceil(ncols);
    let colw = cw.saturating_sub((ncols - 1) * 2) / ncols;
    for (i, u) in c.cores.iter().enumerate() {
        let tone = lvl(*u, 60.0, 85.0);
        p.row(1 + i % per, x0 + (i / per) * (colw + 2), Some(colw), &[
            t(format!("{i:>2} "), Label),
            Item::Bar(u / 100.0, on(tone, hue::CPU)),
            t(format!("{u:>4.0}%"), tone),
        ]);
    }
}

pub fn processes(p: &mut Panel, list: &[Process]) {
    let n = list.len().min(p.h.saturating_sub(1)).min(10);
    p.title_right(&format!("top {n} wg CPU"));
    let wide = p.w >= 80;
    let bar_w = if wide { 12 } else { 0 };
    p.row(0, 0, None, &[
        t("    PID", Label),
        t("  UŻYTKOWNIK", Label),
        t(if wide { format!("  CPU{}", " ".repeat(bar_w + 4)) } else { "      CPU".into() }, Label),
        t("   PAMIĘĆ", Label),
        t("  PROGRAM", Label),
    ]);
    for (i, pr) in list.iter().take(n).enumerate() {
        let y = i + 1;
        let tone = lvl(pr.cpu, 50.0, 90.0);
        let mut x = p.row(y, 0, None, &[
            t(format!("{:>7}", pr.pid), Label),
            t(format!("  {:<10}", fit(&pr.user, 10)), Text),
            t("  ", Text),
        ]);
        if wide {
            p.bar(x, y, bar_w, pr.cpu / 100.0, on(tone, hue::CPU));
            x += bar_w;
        }
        x = p.row(y, x, None, &[
            t(format!("{:6.1}%", pr.cpu), tone),
            t(format!("{:>9}", format::bytes(pr.memory)), Text),
            t("  ", Text),
        ]);
        let rest = p.w.saturating_sub(x);
        p.put(x, y, &fit(&pr.name, rest), Bright);
        let name_len = pr.name.chars().count();
        if !pr.args.is_empty() && name_len + 1 < rest {
            p.put(x + name_len + 1, y, &fit(&pr.args, rest - name_len - 1), Label);
        }
    }
}

pub fn apache(p: &mut Panel, a: &Apache, history: &History) {
    p.title_right(&format!("{} · MaxRequestWorkers {}", a.mpm, a.max));
    let frac = a.busy as f64 / a.max.max(1) as f64;
    let tone = lvl(frac, 0.75, 0.95);
    p.row(0, 0, None, &[t("workery  ", Label), Item::Bar(frac, on(tone, hue::APACHE)), t(format!("  {:>2} / {}", a.busy, a.max), tone)]);
    let mut y = 1;
    if p.h >= 7 {
        p.row(1, 0, None, &[
            t("zajęte ", Label),
            t(a.busy.to_string(), tone),
            t("    wolne ", Label),
            t(a.idle.to_string(), Text),
            t("    maks. ", Label),
            t(a.max.to_string(), Text),
        ]);
        y = 2;
    }
    p.row(y, 0, None, &[t("zapytania/s ", Label), t(format!("{:.1}", a.requests_per_sec), Bright)]);
    let peak = history.max();
    p.right(y, &[t("szczyt 5 min ", Label), t(format!("{peak:.0}/s"), Text)]);
    let gh = p.h.saturating_sub(y + 1);
    let w = p.w;
    p.graph(0, y + 1, w, gh, &resample(history, w * 2, (peak * 1.15).max(1.0)), GraphTone::Fixed(hue::APACHE));
}

pub fn containers(p: &mut Panel, list: &[Container]) {
    let list = by_name(list, |c| &c.name);
    let running = list.iter().filter(|c| c.state == ContainerState::Running).count();
    let expected = list.iter().filter(|c| c.expected).count();
    let expected_running = list.iter().filter(|c| c.expected && c.state == ContainerState::Running).count();
    p.title_right(&format!("{running} z {} działa", list.len()));
    let cols: &[(&str, usize)] = if p.w >= 64 {
        &[("STAN", 13), ("HEALTH", 10), ("CPU", 7), ("PAMIĘĆ", 9)]
    } else {
        &[("STAN", 13), ("CPU", 7), ("PAMIĘĆ", 9)]
    };
    let name_w = p.w.saturating_sub(cols.iter().map(|c| c.1).sum::<usize>() + 2);
    let numeric = |h: &str| matches!(h, "CPU" | "PAMIĘĆ");
    p.put(2, 0, "NAZWA", Label);
    let mut x = 2 + name_w;
    for &(head, w) in cols {
        p.put(if numeric(head) { x + w - head.chars().count() } else { x + 1 }, 0, head, Label);
        x += w;
    }
    for (i, c) in list.iter().enumerate() {
        let y = i + 1;
        let (state, st) = match c.state {
            ContainerState::Running => ("działa", Fine),
            ContainerState::Stopped => ("zatrzymany", Bad),
            ContainerState::Missing => ("brak", Bad),
        };
        let (health, ht) = match c.health {
            Some(Health::Healthy) => ("zdrowy", Fine),
            Some(Health::Unhealthy) => ("niezdrowy", Bad),
            Some(Health::Starting) => ("startuje", Warn),
            None => ("—", NoData),
        };
        let dot = if st == Bad || ht == Bad { Bad } else if ht == Warn { Warn } else { Good };
        p.put(0, y, "●", dot);
        p.put(2, y, &fit(&c.name, name_w.saturating_sub(1)), Text);
        let cpu = c.cpu.map_or("—".into(), |v| format!("{v:.1} %"));
        let mem = c.memory.map_or("—".into(), format::bytes);
        let mut x = 2 + name_w;
        for &(head, w) in cols {
            match head {
                "STAN" => p.put(x + 1, y, state, st),
                "HEALTH" => p.put(x + 1, y, health, ht),
                "CPU" => p.put(x + w - cpu.chars().count(), y, &cpu, if c.cpu.is_some() { Text } else { NoData }),
                _ => p.put(x + w - mem.chars().count(), y, &mem, if c.memory.is_some() { Text } else { NoData }),
            };
            x += w;
        }
    }
    if p.h >= list.len() + 3 {
        p.row(list.len() + 2, 0, None, &[
            t("oczekiwane ", Label),
            t(expected.to_string(), Text),
            t("    działa ", Label),
            t(expected_running.to_string(), if expected_running == expected { Fine } else { Bad }),
        ]);
    }
}

pub fn ups(p: &mut Panel, u: &Ups) {
    p.title_right(&u.model);
    let power = if u.on_battery {
        vec![t("zasilanie   ", Label), t("● ", Bad), t(format!("z baterii od {}", format::age(u.on_battery_for)), Bad)]
    } else {
        vec![t("zasilanie   ", Label), t("● ", Good), t("z sieci", Fine)]
    };
    let lt = lvl(u.load, 60.0, 85.0);
    let ct = if u.charge >= 80.0 { Fine } else if u.charge >= 40.0 { Warn } else { Bad };
    let mins = (u.time_left.as_secs() + 30) / 60;
    let tt = if mins >= 20 { Fine } else if mins >= 10 { Warn } else { Bad };
    rows_out(p, vec![
        Some(power),
        None,
        Some(vec![t("obciążenie  ", Label), Item::Bar(u.load / 100.0, on(lt, hue::UPS_LOAD)), t(format!("  {:>3.0} %", u.load), lt)]),
        Some(vec![t("bateria     ", Label), Item::Bar(u.charge / 100.0, on(ct, hue::BATTERY)), t(format!("  {:>3.0} %", u.charge), ct)]),
        Some(vec![
            t("na baterii  ", Label),
            t(format!("{}{mins} min", if u.on_battery { "zostało " } else { "wystarczy na " }), tt),
        ]),
        Some(vec![
            t("napięcie    ", Label),
            t(format!("{:.0} V", u.line_voltage), if u.on_battery { Bad } else { Fine }),
        ]),
    ]);
}

pub fn memory(p: &mut Panel, m: &Memory, history: &History) {
    p.title_right(&format!("{:.1} GB", format::gb(m.total)));
    let f = m.used as f64 / m.total.max(1) as f64;
    let tone = lvl(f, 0.75, 0.9);
    p.row(0, 0, None, &[t("RAM   ", Label), Item::Bar(f, on(tone, hue::RAM)), t(format!("  {:>3.0} %", f * 100.0), tone)]);
    let (used, cache, free) = (format::gb(m.used), format::gb(m.cache), format::gb(m.free));
    let variants = [
        vec![
            t("użyta ", Label), t(format!("{used:.1} GB"), Text),
            t("   cache ", Label), t(format!("{cache:.1} GB"), Text),
            t("   wolna ", Label), t(format!("{free:.1} GB"), Text),
        ],
        vec![
            t("użyta ", Label), t(format!("{used:.1}"), Text),
            t(" cache ", Label), t(format!("{cache:.1}"), Text),
            t(" wolna ", Label), t(format!("{free:.1} GB"), Text),
        ],
    ];
    p.row(1, 0, None, pick(p.w, &variants));
    if m.swap_total == 0 {
        p.row(2, 0, None, &[t("swap  ", Label), t("brak", Label)]);
    } else {
        let sf = m.swap_used as f64 / m.swap_total as f64;
        let st = lvl(sf, 0.25, 0.6);
        p.row(2, 0, None, &[t("swap  ", Label), Item::Bar(sf, on(st, hue::SWAP)), t(format!("  {:>3.0} %", sf * 100.0), st)]);
    }
    let w = p.w;
    let gh = p.h.saturating_sub(3);
    p.graph(0, 3, w, gh, &resample(history, w * 2, 1.0), GraphTone::Level(hue::RAM));
}

/// Two labelled graphs one above the other, on a common scale.
#[allow(clippy::too_many_arguments)]
fn graph_pair(p: &mut Panel, y0: usize, a: (&str, String, &History, Tone), b: (&str, String, &History, Tone), reserve: usize) {
    let k = p.h.saturating_sub(y0 + reserve + 2) / 2;
    let top = (a.2.max().max(b.2.max()) * 1.1).max(1.0);
    let w = p.w;
    p.row(y0, 0, None, &[t(format!("{} ", a.0), Label), t(a.1, Bright)]);
    p.graph(0, y0 + 1, w, k, &resample(a.2, w * 2, top), GraphTone::Fixed(a.3));
    p.row(y0 + 1 + k, 0, None, &[t(format!("{} ", b.0), Label), t(b.1, Bright)]);
    p.graph(0, y0 + 2 + k, w, k, &resample(b.2, w * 2, top), GraphTone::Fixed(b.3));
}

pub fn disk(p: &mut Panel, d: &Disk, read: &History, write: &History, wear: Option<f64>) {
    p.title_right(&d.model);
    let f = d.used as f64 / d.size.max(1) as f64;
    let tone = lvl(f, 0.8, 0.9);
    p.row(0, 0, None, &[
        t(format!("{}  ", d.mount), Label),
        Item::Bar(f, on(tone, hue::DISK)),
        t(format!("  {:>3.0} %", f * 100.0), tone),
        t(format!("  {:.0} / {:.0} GB", format::gb(d.used), format::gb(d.size)), Text),
    ]);
    graph_pair(
        p,
        1,
        ("odczyt", format::byte_rate(d.read), read, hue::DISK),
        ("zapis", format::byte_rate(d.write), write, hue::DISK_WRITE),
        1,
    );
    let mut last = vec![];
    if let Some(temp) = d.temp {
        last.push(t("temp. ", Label));
        last.push(t(format!("{temp:.0} °C"), lvl(temp, 60.0, 70.0)));
    }
    if let Some(wear) = wear {
        last.push(t(if last.is_empty() { "zużycie " } else { "    zużycie " }, Label));
        last.push(t(format!("{wear:.0} %"), lvl(wear, 80.0, 95.0)));
    }
    let y = p.h.saturating_sub(1);
    p.row(y, 0, None, &last);
}

pub fn network(p: &mut Panel, n: &Network, rx: &History, tx: &History) {
    if n.address.is_empty() {
        p.title_right(&n.interface);
    } else {
        p.title_right(&format!("{} · {}", n.interface, n.address));
    }
    graph_pair(
        p,
        0,
        ("↓ odbiór", format::bit_rate(n.rx), rx, hue::NET_RX),
        ("↑ wysyłka", format::bit_rate(n.tx), tx, hue::NET_TX),
        1,
    );
    let variants = [
        vec![t("połączenia TCP ", Label), t(n.tcp.to_string(), Text), t("    nawiązane ", Label), t(n.established.to_string(), Text)],
        vec![t("TCP ", Label), t(n.tcp.to_string(), Text), t("  nawiązane ", Label), t(n.established.to_string(), Text)],
    ];
    let y = p.h.saturating_sub(1);
    p.row(y, 0, None, pick(p.w, &variants));
}

pub fn sensors(p: &mut Panel, s: &Sensors) {
    p.title_right(&s.source);
    let mut rows = vec![];
    if let Some(fan) = &s.fan {
        rows.push(Some(match fan.max {
            Some(max) => {
                let f = fan.rpm as f64 / max.max(1) as f64;
                let tone = lvl(f, 0.7, 0.9);
                vec![t("wentylator  ", Label), Item::Bar(f, on(tone, hue::FAN)), t(format!("  {:>4} obr/min", fan.rpm), tone)]
            }
            None => vec![t("wentylator  ", Label), t(format!("{} obr/min", fan.rpm), Text)],
        }));
        rows.push(None);
    }
    for (name, temp) in &s.temps {
        let tone = lvl(*temp, 60.0, 75.0);
        rows.push(Some(vec![
            t(format!("{:<11} ", fit(name, 11)), Label),
            Item::Bar(temp / 100.0, on(tone, hue::temp(*temp))),
            t(format!("  {temp:>4.0} °C"), tone),
        ]));
    }
    rows_out(p, rows);
}

pub fn services(p: &mut Panel, s: &Services) {
    let n = s.units.len();
    p.title_right(&format!("{n} {}", plural(n, "usługa", "usługi", "usług")));
    p.row(0, 0, None, &[t("uszkodzone (failed) ", Label), t(s.failed.to_string(), if s.failed == 0 { Fine } else { Bad })]);
    if n == 0 {
        return;
    }
    let avail = p.h.saturating_sub(2).max(1);
    let ncols = n.div_ceil(avail);
    let per = n.div_ceil(ncols);
    let colw = p.w.saturating_sub((ncols - 1) * 2) / ncols;
    for (i, (name, state)) in by_name(&s.units, |u| &u.0).into_iter().enumerate() {
        let (x, y) = ((i / per) * (colw + 2), 2 + i % per);
        let (word, tone) = match state {
            UnitState::Active => ("działa", Fine),
            UnitState::Activating => ("startuje", Warn),
            UnitState::Inactive => ("zatrzymana", Bad),
            UnitState::Failed => ("błąd", Bad),
        };
        p.put(x, y, "●", if tone == Fine { Good } else { tone });
        if colw >= 30 {
            p.put(x + 2, y, &fit(name, colw - 4 - word.len()), Text);
            p.put(x + colw - word.chars().count(), y, word, tone);
        } else {
            p.put(x + 2, y, &fit(name, colw.saturating_sub(2)), Text);
        }
    }
}

pub fn sites(p: &mut Panel, list: &[Site], age: Option<Duration>) {
    p.title_right("GET co 60 s · limit 10 s");
    let (code_w, time_w) = (6, 14);
    let list = by_name(list, |s| &s.name);
    let rows: Vec<_> = list
        .iter()
        .map(|s| {
            let (code, time, code_tone, time_tone) = match &s.outcome {
                SiteOutcome::Response { code, time } => {
                    let ms = time.as_millis();
                    let text = if ms < 1000 { format!("{ms} ms") } else { format!("{:.1} s", time.as_secs_f64()) };
                    let tone = if ms < 1000 { Fine } else if ms < 3000 { Warn } else { Bad };
                    let code_tone = match code {
                        _ if !s.accepted => Bad,
                        200..=299 => Fine,
                        300..=399 => hue::REDIRECT,
                        _ => hue::ACCEPTED,
                    };
                    (code.to_string(), text, code_tone, tone)
                }
                SiteOutcome::Timeout => ("—".into(), "limit czasu".into(), Bad, Bad),
                SiteOutcome::Error(e) => ("—".into(), fit(e, time_w - 1), Bad, Bad),
            };
            let dot = if code_tone == Bad || time_tone == Bad { Bad } else if time_tone == Warn { Warn } else { Good };
            (s, code, time, code_tone, time_tone, dot)
        })
        .collect();

    // Too many sites for one per row: names only, in columns; the dot still shows the state.
    if list.len() > p.h {
        let ncols = list.len().div_ceil(p.h.max(1));
        let per = list.len().div_ceil(ncols);
        let colw = p.w.saturating_sub((ncols - 1) * 2) / ncols;
        for (i, (s, .., dot)) in rows.iter().enumerate() {
            let (x, y) = ((i / per) * (colw + 2), i % per);
            p.put(x, y, "●", *dot);
            p.put(x + 2, y, &fit(&s.name, colw.saturating_sub(2)), Text);
        }
        return;
    }

    let header = p.h > list.len();
    let y0 = usize::from(header);
    let name_w = p.w.saturating_sub(2 + code_w + time_w);
    if header {
        p.put(2, 0, "STRONA", Label);
        p.put(2 + name_w + code_w - 3, 0, "KOD", Label);
        p.put(p.w.saturating_sub(4), 0, "CZAS", Label);
    }
    for (i, (s, code, time, code_tone, time_tone, dot)) in rows.iter().enumerate() {
        let (code_tone, time_tone) = (*code_tone, *time_tone);
        let y = y0 + i;
        p.put(0, y, "●", *dot);
        p.put(2, y, &fit(&s.name, name_w.saturating_sub(1)), Text);
        p.put((2 + name_w + code_w).saturating_sub(code.chars().count()), y, code, code_tone);
        p.put(p.w.saturating_sub(time.chars().count()), y, time, time_tone);
    }
    if let Some(age) = age
        && p.h >= list.len() + 3
    {
        let y = p.h - 1;
        p.put(0, y, &format!("ostatnie sprawdzenie {} temu", format::age(age)), Label);
    }
}

pub fn databases(p: &mut Panel, d: &Databases) {
    let names: Vec<String> = [
        d.mariadb.as_ref().map(|m| format!("MariaDB {}", m.version)),
        d.postgres.as_ref().map(|g| format!("PostgreSQL {}", g.version)),
    ]
    .into_iter()
    .flatten()
    .collect();
    p.title_right(&names.join(" · "));
    let connections = |used: u32, max: u32, color: Tone| {
        let f = used as f64 / max.max(1) as f64;
        let tone = lvl(f, 0.7, 0.9);
        vec![t("  połączenia ", Label), Item::Bar(f, on(tone, color)), t(format!("  {used:>3} / {max}"), tone)]
    };
    let mut rows = vec![];
    if let Some(m) = &d.mariadb {
        let tt = lvl(m.threads_running as f64, 12.0, 30.0);
        rows.push(Some(vec![t("MariaDB", Title)]));
        rows.push(Some(connections(m.connections, m.max_connections, hue::MARIADB)));
        let both = vec![
            t("  wątki aktywne ", Label), t(m.threads_running.to_string(), tt),
            t("    zapytania/s ", Label), t(format!("{:.0}", m.queries_per_sec), Text),
        ];
        if width(&both) <= p.w {
            rows.push(Some(both));
        } else {
            rows.push(Some(vec![t("  wątki aktywne ", Label), t(m.threads_running.to_string(), tt)]));
            rows.push(Some(vec![t("  zapytania/s ", Label), t(format!("{:.0}", m.queries_per_sec), Text)]));
        }
        rows.push(Some(vec![
            t("  wolne zapytania ", Label),
            t(format!("{:.0} /min", m.slow_per_min), lvl(m.slow_per_min, 1.0, 10.0)),
        ]));
    }
    if let Some(g) = &d.postgres {
        if !rows.is_empty() {
            rows.push(None);
        }
        rows.push(Some(vec![t("PostgreSQL", Title)]));
        rows.push(Some(connections(g.connections, g.max_connections, hue::POSTGRES)));
        rows.push(Some(vec![
            t("  aktywne ", Label), t(g.active.to_string(), Text),
            t("    bezczynne ", Label), t(g.idle.to_string(), Text),
        ]));
    }
    for e in &d.errors {
        if !rows.is_empty() {
            rows.push(None);
        }
        rows.push(Some(vec![t(e.clone(), NoData)]));
    }
    rows_out(p, rows);
}

pub fn fail2ban(p: &mut Panel, jails: &[Jail]) {
    let n = jails.len();
    let total: u32 = jails.iter().map(|j| j.banned).sum();
    p.title_right(&format!("{n} {}", plural(n, "jail", "jaile", "jaili")));
    p.row(0, 0, None, &[t("zbanowane teraz ", Label), t(total.to_string(), Bright)]);
    let y0 = if p.h >= n + 2 { 2 } else { 1 };
    for (i, j) in by_name(jails, |j| &j.name).into_iter().enumerate() {
        let count = j.banned.to_string();
        p.put(0, y0 + i, &fit(&j.name, p.w.saturating_sub(count.len() + 1)), Text);
        p.put(p.w.saturating_sub(count.len()), y0 + i, &count, Text);
    }
}
