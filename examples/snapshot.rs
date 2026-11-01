//! Renders demo screens into one HTML page that draws the terminal grid cell by cell
//! (8×16 pixels, as on a 1920×1080 console). Used for screenshots in reviews and the README.
//!
//! cargo run --example snapshot -- OUT.html [CONFIG]
//! snapshot OUT.html --live [CONFIG]    real data of the machine it runs on (Linux)

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::Terminal;
use serde_json::json;

use wallmon::collectors::demo::{Demo, Scenario};
use wallmon::config::Config;
use wallmon::state::State;
use wallmon::ui::{self, Look};

const SHOTS: [(&str, &str, Scenario, Look, u16, u16); 5] = [
    ("Normal operation", "1920×1080 monitor in kmscon, 240×67 characters.", Scenario::Normal, Look::Full, 240, 67),
    ("Failure", "wallmon --demo=failure, same monitor.", Scenario::Failure, Look::Full, 240, 67),
    ("Small terminal over SSH", "150×45 character window.", Scenario::Normal, Look::Full, 150, 45),
    ("Plain Linux console", "wallmon --basic: 16 colours, no braille, 240×67 characters.", Scenario::Normal, Look::Basic, 240, 67),
    ("Plain Linux console, failure", "wallmon --basic --demo=failure.", Scenario::Failure, Look::Basic, 240, 67),
];

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next() else {
        eprintln!("usage: snapshot OUT.html [--live] [CONFIG]");
        std::process::exit(2);
    };
    let mut rest: Vec<String> = args.collect();
    let live = rest.first().is_some_and(|a| a == "--live");
    if live {
        rest.remove(0);
    }
    let config = match rest.first() {
        Some(path) => Config::load(path.as_ref()).unwrap_or_else(|e| panic!("{e}")),
        None if live => Config::default(),
        None => Config::demo(),
    };
    let local = chrono::Local::now();
    let screens = if live { live_screens(&config, local) } else { demo_screens(&config, local) };
    let page = PAGE.replace("__DATA__", &serde_json::to_string(&screens).expect("json"));
    std::fs::write(&out, page).unwrap_or_else(|e| panic!("{out}: {e}"));
}

/// Runs the real sources for a few seconds, then draws what they found.
fn live_screens(config: &Config, local: chrono::DateTime<chrono::Local>) -> Vec<serde_json::Value> {
    let name = config.general.name.clone().unwrap_or_else(|| "localhost".into());
    let state = Arc::new(Mutex::new(State::new(name)));
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    runtime.block_on(async {
        wallmon::collectors::spawn_local(&state, config);
        // Long enough for two reads of the slower sources, which rates need.
        tokio::time::sleep(Duration::from_secs(11)).await;
    });
    let state = state.lock().expect("state");
    [
        ("Real data", "wallmon on Linux, 240×67 characters.", Look::Full, 240, 67),
        ("Real data, small window", "150×45 characters.", Look::Full, 150, 45),
        ("Real data, plain Linux console", "wallmon --basic, 240×67 characters.", Look::Basic, 240, 67),
    ]
        .iter()
        .map(|&(title, desc, look, w, h)| {
            let mut term = Terminal::new(TestBackend::new(w, h)).expect("test backend");
            term.draw(|f| ui::draw(f, &state, config, Instant::now(), local, look)).expect("draw");
            encode(title, desc, term.backend().buffer())
        })
        .collect()
}

fn demo_screens(config: &Config, local: chrono::DateTime<chrono::Local>) -> Vec<serde_json::Value> {
    SHOTS
        .iter()
        .map(|&(name, desc, scenario, look, w, h)| {
            let now = Instant::now();
            let mut state = State::new(config.general.name.clone().unwrap_or_else(|| "demo".into()));
            Demo::new(scenario, config).prefill(&mut state, now);
            let mut term = Terminal::new(TestBackend::new(w, h)).expect("test backend");
            term.draw(|f| ui::draw(f, &state, config, now, local, look)).expect("draw");
            encode(name, desc, term.backend().buffer())
        })
        .collect()
}

/// One screen as rows of symbols plus, per cell, an index into its colour palette.
fn encode(name: &str, desc: &str, buf: &Buffer) -> serde_json::Value {
    let (w, h) = (buf.area.width as usize, buf.area.height as usize);
    let mut palette: Vec<String> = vec![];
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut rows = vec![];
    let mut colors = vec![];
    for y in 0..h {
        let mut row = vec![];
        let mut cols = vec![];
        for x in 0..w {
            let cell = &buf.content()[y * w + x];
            let sym = cell.symbol();
            row.push(if sym.is_empty() { " ".to_string() } else { sym.to_string() });
            let hex = hex(cell.fg);
            let i = *index.entry(hex.clone()).or_insert_with(|| {
                palette.push(hex);
                palette.len() - 1
            });
            cols.push(i);
        }
        rows.push(row);
        colors.push(cols);
    }
    let bg = buf.content().first().map_or("#0d1117".into(), |c| hex(c.bg));
    json!({ "name": name, "desc": desc, "W": w, "H": h, "bg": bg, "pal": palette, "rows": rows, "cols": colors })
}

/// Named colours as the Linux console shows them (the VGA palette).
fn hex(c: Color) -> String {
    let vga = |i: usize| ["#000000", "#aa0000", "#00aa00", "#aa5500", "#0000aa", "#aa00aa", "#00aaaa", "#aaaaaa",
                          "#555555", "#ff5555", "#55ff55", "#ffff55", "#5555ff", "#ff55ff", "#55ffff", "#ffffff"][i].to_string();
    match c {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Black => vga(0),
        Color::Red => vga(1),
        Color::Green => vga(2),
        Color::Yellow => vga(3),
        Color::Blue => vga(4),
        Color::Magenta => vga(5),
        Color::Cyan => vga(6),
        Color::Gray => vga(7),
        Color::DarkGray => vga(8),
        Color::LightRed => vga(9),
        Color::LightGreen => vga(10),
        Color::LightYellow => vga(11),
        Color::LightBlue => vga(12),
        Color::LightMagenta => vga(13),
        Color::LightCyan => vga(14),
        Color::White => vga(15),
        _ => "#c9d1d9".into(),
    }
}

const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>wallmon screens</title>
<style>
  body { background: #05070a; color: #c9d1d9; font: 15px/1.5 -apple-system, system-ui, sans-serif; margin: 0; padding: 24px 16px 64px; }
  h1 { font-size: 22px; margin: 0 0 4px; }
  h2 { font-size: 18px; margin: 40px 0 4px; }
  p { margin: 0 0 12px; color: #8b949e; }
  canvas { display: block; max-width: 100%; height: auto; border: 1px solid #222; }
</style>
</head>
<body>
<h1>wallmon --demo</h1>
<p>Screens drawn by the program from made-up data, on the character grid of a server console (8×16 pixel cells).</p>
<div id="out"></div>
<script>
const DATA = __DATA__;
const CW = 8, CH = 16;
const BRAILLE = [[0,0],[0,1],[0,2],[1,0],[1,1],[1,2],[0,3],[1,3]];
function line(ctx, a, b, c, d) { ctx.beginPath(); ctx.moveTo(a, b); ctx.lineTo(c, d); ctx.stroke(); }
function corner(ctx, ax, ay, cx, cy, bx, by, r = 4) { ctx.beginPath(); ctx.moveTo(ax, ay); ctx.arcTo(cx, cy, bx, by, r); ctx.lineTo(bx, by); ctx.stroke(); }
function cell(ctx, ch, col, x, y) {
  const px = x * CW, py = y * CH, mx = px + 4.5, my = py + 8.5;
  ctx.fillStyle = col; ctx.strokeStyle = col; ctx.lineWidth = 1;
  const code = ch.codePointAt(0);
  if (code >= 0x2800 && code <= 0x28ff) {
    const bits = code - 0x2800;
    for (let i = 0; i < 8; i++) if (bits & (1 << i)) { const [cx, cy] = BRAILLE[i]; ctx.fillRect(px + 1 + cx * 4, py + 1 + cy * 4, 3, 3); }
    return;
  }
  switch (ch) {
    case '─': return line(ctx, px, my, px + CW, my);
    case '│': return line(ctx, mx, py, mx, py + CH);
    case '╭': return corner(ctx, px + CW, my, mx, my, mx, py + CH);
    case '╮': return corner(ctx, px, my, mx, my, mx, py + CH);
    case '╰': return corner(ctx, mx, py, mx, my, px + CW, my);
    case '╯': return corner(ctx, mx, py, mx, my, px, my);
    case '┌': return corner(ctx, px + CW, my, mx, my, mx, py + CH, 0);
    case '┐': return corner(ctx, px, my, mx, my, mx, py + CH, 0);
    case '└': return corner(ctx, mx, py, mx, my, px + CW, my, 0);
    case '┘': return corner(ctx, mx, py, mx, my, px, my, 0);
    case '▆': return ctx.fillRect(px, py + 4, CW, 12);
    case '▄': return ctx.fillRect(px, py + 8, CW, 8);
    case '█': return ctx.fillRect(px, py, CW, CH);
    case '■': return ctx.fillRect(px + 1, py + 5, 6, 6);
    case '●': ctx.beginPath(); ctx.arc(px + 4, py + 8, 3.5, 0, 2 * Math.PI); ctx.fill(); return;
  }
  ctx.fillText(ch, px, py + 9);
}
function paint(v, dpr) {
  const c = document.createElement('canvas');
  c.width = v.W * CW * dpr; c.height = v.H * CH * dpr;
  const ctx = c.getContext('2d'); ctx.scale(dpr, dpr);
  ctx.fillStyle = v.bg; ctx.fillRect(0, 0, v.W * CW, v.H * CH);
  ctx.font = '13px Menlo, "DejaVu Sans Mono", monospace'; ctx.textBaseline = 'middle';
  for (let y = 0; y < v.H; y++)
    for (let x = 0; x < v.W; x++)
      if (v.rows[y][x] !== ' ') cell(ctx, v.rows[y][x], v.pal[v.cols[y][x]], x, y);
  return c;
}
for (const v of DATA) {
  const h = document.createElement('h2'); h.textContent = v.name; out.appendChild(h);
  const p = document.createElement('p'); p.textContent = v.desc + ' '; out.appendChild(p);
  // The saved PNG has one pixel per console pixel, as the screen in the server room.
  const a = document.createElement('a');
  a.textContent = 'Save PNG';
  a.href = '#';
  a.onclick = e => {
    e.preventDefault();
    const bytes = atob(paint(v, 1).toDataURL('image/png').split(',')[1]);
    const blob = new Blob([Uint8Array.from(bytes, ch => ch.charCodeAt(0))], { type: 'image/png' });
    const link = document.createElement('a');
    link.href = URL.createObjectURL(blob);
    link.download = 'wallmon-' + v.name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/-$/, '') + '.png';
    document.body.appendChild(link);
    link.click();
    link.remove();
  };
  p.appendChild(a);
  const c = paint(v, Math.max(2, window.devicePixelRatio || 1));
  c.style.width = (v.W * CW) + 'px';
  out.appendChild(c);
}
</script>
</body>
</html>
"#;
