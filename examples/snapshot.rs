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
use wallmon::ui;

const SHOTS: [(&str, &str, Scenario, u16, u16); 3] = [
    ("Normalna praca", "Monitor 1920×1080, 240×67 znaków.", Scenario::Normal, 240, 67),
    ("Awaria", "wallmon --demo=failure, ten sam monitor.", Scenario::Failure, 240, 67),
    ("Mały terminal przez SSH", "Okno 150×45 znaków.", Scenario::Normal, 150, 45),
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
    [("Prawdziwe dane", "wallmon na Linuksie, 240×67 znaków.", 240, 67), ("Prawdziwe dane, małe okno", "150×45 znaków.", 150, 45)]
        .iter()
        .map(|&(title, desc, w, h)| {
            let mut term = Terminal::new(TestBackend::new(w, h)).expect("test backend");
            term.draw(|f| ui::draw(f, &state, config, Instant::now(), local)).expect("draw");
            encode(title, desc, term.backend().buffer())
        })
        .collect()
}

fn demo_screens(config: &Config, local: chrono::DateTime<chrono::Local>) -> Vec<serde_json::Value> {
    SHOTS
        .iter()
        .map(|&(name, desc, scenario, w, h)| {
            let now = Instant::now();
            let mut state = State::new(config.general.name.clone().unwrap_or_else(|| "demo".into()));
            Demo::new(scenario, config).prefill(&mut state, now);
            let mut term = Terminal::new(TestBackend::new(w, h)).expect("test backend");
            term.draw(|f| ui::draw(f, &state, config, now, local)).expect("draw");
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
            let hex = match cell.fg {
                Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
                _ => "#c9d1d9".into(),
            };
            let i = *index.entry(hex.clone()).or_insert_with(|| {
                palette.push(hex);
                palette.len() - 1
            });
            cols.push(i);
        }
        rows.push(row);
        colors.push(cols);
    }
    json!({ "name": name, "desc": desc, "W": w, "H": h, "pal": palette, "rows": rows, "cols": colors })
}

const PAGE: &str = r#"<!doctype html>
<html lang="pl">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>wallmon zrzuty</title>
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
<p>Zrzuty z programu, dane udawane. Siatka znaków jak w konsoli serwera (komórka 8×16 pikseli).</p>
<div id="out"></div>
<script>
const DATA = __DATA__;
const BG = '#0d1117', CW = 8, CH = 16;
const BRAILLE = [[0,0],[0,1],[0,2],[1,0],[1,1],[1,2],[0,3],[1,3]];
function line(ctx, a, b, c, d) { ctx.beginPath(); ctx.moveTo(a, b); ctx.lineTo(c, d); ctx.stroke(); }
function corner(ctx, ax, ay, cx, cy, bx, by) { ctx.beginPath(); ctx.moveTo(ax, ay); ctx.arcTo(cx, cy, bx, by, 4); ctx.lineTo(bx, by); ctx.stroke(); }
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
    case '▆': return ctx.fillRect(px, py + 4, CW, 12);
    case '●': ctx.beginPath(); ctx.arc(px + 4, py + 8, 3.5, 0, 2 * Math.PI); ctx.fill(); return;
  }
  ctx.fillText(ch, px, py + 9);
}
for (const v of DATA) {
  const h = document.createElement('h2'); h.textContent = v.name; out.appendChild(h);
  const p = document.createElement('p'); p.textContent = v.desc; out.appendChild(p);
  const c = document.createElement('canvas');
  const dpr = Math.max(2, window.devicePixelRatio || 1);
  c.width = v.W * CW * dpr; c.height = v.H * CH * dpr;
  c.style.width = (v.W * CW) + 'px';
  out.appendChild(c);
  const ctx = c.getContext('2d'); ctx.scale(dpr, dpr);
  ctx.fillStyle = BG; ctx.fillRect(0, 0, v.W * CW, v.H * CH);
  ctx.font = '13px Menlo, "DejaVu Sans Mono", monospace'; ctx.textBaseline = 'middle';
  for (let y = 0; y < v.H; y++)
    for (let x = 0; x < v.W; x++)
      if (v.rows[y][x] !== ' ') cell(ctx, v.rows[y][x], v.pal[v.cols[y][x]], x, y);
}
</script>
</body>
</html>
"#;
