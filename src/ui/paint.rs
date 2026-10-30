//! Drawing primitives: frames with titles, text rows, bars and braille graphs.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use crate::state::History;

pub const BACKGROUND: Color = Color::Rgb(0x0d, 0x11, 0x17);

/// What a piece of the screen means. Colour follows from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Text,
    Border,
    Title,
    Label,
    /// Within limits: a value that needs no attention, in the plain text colour.
    Fine,
    /// Status dot of something that works.
    Good,
    Warn,
    Bad,
    NoData,
    /// Identity colour of a bar or graph, from `hue`. Problems still turn it yellow or red.
    Hue(Color),
    /// Unfilled part of a bar.
    Empty,
    Bright,
}

impl Tone {
    pub fn color(self) -> Color {
        match self {
            Tone::Text => Color::Rgb(0xc9, 0xd1, 0xd9),
            Tone::Border => Color::Rgb(0x3d, 0x46, 0x56),
            Tone::Title => Color::Rgb(0xe6, 0xed, 0xf3),
            Tone::Label => Color::Rgb(0x7d, 0x87, 0x96),
            Tone::Fine => Color::Rgb(0xc9, 0xd1, 0xd9),
            Tone::Good => Color::Rgb(0x3f, 0xcf, 0x6e),
            Tone::Warn => Color::Rgb(0xe3, 0xc3, 0x41),
            Tone::Bad => Color::Rgb(0xff, 0x55, 0x55),
            Tone::NoData => Color::Rgb(0x5a, 0x61, 0x6c),
            Tone::Hue(c) => c,
            Tone::Empty => Color::Rgb(0x23, 0x28, 0x31),
            Tone::Bright => Color::Rgb(0xff, 0xff, 0xff),
        }
    }
}

/// Each panel has its own colour for bars and graphs, so the screen is not one colour
/// and a panel is recognisable from a distance. None of them is yellow, orange or red.
pub mod hue {
    use super::Tone::{self, Hue};
    use ratatui::style::Color::Rgb;

    pub const CPU: Tone = Hue(Rgb(0x39, 0xc5, 0xcf));
    pub const APACHE: Tone = Hue(Rgb(0x58, 0xa6, 0xff));
    pub const RAM: Tone = Hue(Rgb(0xa3, 0x71, 0xf7));
    pub const SWAP: Tone = Hue(Rgb(0xf7, 0x78, 0xba));
    pub const DISK: Tone = Hue(Rgb(0x79, 0xc0, 0xff));
    pub const DISK_WRITE: Tone = Hue(Rgb(0xf7, 0x78, 0xba));
    pub const NET_RX: Tone = Hue(Rgb(0x56, 0xd3, 0x64));
    pub const NET_TX: Tone = Hue(Rgb(0xd2, 0xa8, 0xff));
    pub const UPS_LOAD: Tone = Hue(Rgb(0x2d, 0xd4, 0xbf));
    pub const BATTERY: Tone = Hue(Rgb(0x3f, 0xb9, 0x50));
    pub const FAN: Tone = Hue(Rgb(0x39, 0xc5, 0xcf));
    pub const MARIADB: Tone = Hue(Rgb(0x2d, 0xd4, 0xbf));
    pub const POSTGRES: Tone = Hue(Rgb(0x79, 0xc0, 0xff));
    /// HTTP redirects, and other codes accepted by the configuration.
    pub const REDIRECT: Tone = Hue(Rgb(0x79, 0xc0, 0xff));
    pub const ACCEPTED: Tone = Hue(Rgb(0xd2, 0xa8, 0xff));

    /// A thermometer below the warning level: blue when cold, cyan, then green.
    pub fn temp(c: f64) -> Tone {
        match c {
            c if c < 40.0 => Hue(Rgb(0x58, 0xa6, 0xff)),
            c if c < 50.0 => Hue(Rgb(0x39, 0xc5, 0xcf)),
            _ => Hue(Rgb(0x56, 0xd3, 0x64)),
        }
    }
}

/// The colour of a bar: its own hue while the value is fine, yellow or red otherwise.
pub fn on(tone: Tone, hue: Tone) -> Tone {
    if tone == Tone::Fine { hue } else { tone }
}

/// Fine below `warn`, yellow below `crit`, red above.
pub fn lvl(v: f64, warn: f64, crit: f64) -> Tone {
    if v >= crit {
        Tone::Bad
    } else if v >= warn {
        Tone::Warn
    } else {
        Tone::Fine
    }
}

/// One piece of a row: text, or a bar that takes the width left over.
pub enum Item {
    Text(String, Tone),
    Bar(f64, Tone),
}

pub fn t(s: impl Into<String>, tone: Tone) -> Item {
    Item::Text(s.into(), tone)
}

pub fn width(items: &[Item]) -> usize {
    items
        .iter()
        .map(|i| match i {
            Item::Text(s, _) => s.chars().count(),
            Item::Bar(..) => 0,
        })
        .sum()
}

/// The first variant that fits in `w`, or the last one.
pub fn pick(w: usize, variants: &[Vec<Item>]) -> &[Item] {
    variants.iter().find(|v| width(v) <= w).or(variants.last()).map_or(&[], |v| v.as_slice())
}

/// Cuts text to `w` characters, ending with an ellipsis when cut.
pub fn fit(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        s.to_string()
    } else if w == 0 {
        String::new()
    } else {
        s.chars().take(w - 1).chain(std::iter::once('…')).collect()
    }
}

/// Spreads a whole history window over `points` values, divided by `scale`, so a graph
/// always spans the same time. Samples not collected yet (just after start) count as 0.
pub fn resample(hist: &History, points: usize, scale: f64) -> Vec<f64> {
    let (cap, values) = (hist.capacity(), hist.values());
    let missing = cap - values.len();
    (0..points)
        .map(|i| {
            let a = i * cap / points;
            let b = ((i + 1) * cap / points).max(a + 1);
            let got: Vec<f64> = (a..b).filter(|&j| j >= missing).map(|j| values[j - missing]).collect();
            if got.is_empty() { 0.0 } else { got.iter().sum::<f64>() / got.len() as f64 / scale }
        })
        .collect()
}

pub enum GraphTone {
    /// Bands by height: the hue, then yellow and red near the top.
    Level(Tone),
    Fixed(Tone),
}

/// The inside of a frame. Coordinates are relative to it and everything is clipped to it.
pub struct Panel<'a> {
    buf: &'a mut Buffer,
    frame: Rect,
    x: u16,
    y: u16,
    pub w: usize,
    pub h: usize,
    stale: bool,
    title_end: usize,
}

/// Draws a rounded frame with a title (and an age note for stale data) and returns its inside.
pub fn frame<'a>(buf: &'a mut Buffer, r: Rect, title: &str, note: Option<&str>) -> Panel<'a> {
    let border = Style::default().fg(Tone::Border.color());
    if r.width >= 2 && r.height >= 2 {
        let (right, bottom) = (r.right() - 1, r.bottom() - 1);
        for x in r.x..=right {
            buf[(x, r.y)].set_symbol("─").set_style(border);
            buf[(x, bottom)].set_symbol("─").set_style(border);
        }
        for y in r.y..=bottom {
            buf[(r.x, y)].set_symbol("│").set_style(border);
            buf[(right, y)].set_symbol("│").set_style(border);
        }
        buf[(r.x, r.y)].set_symbol("╭");
        buf[(right, r.y)].set_symbol("╮");
        buf[(r.x, bottom)].set_symbol("╰");
        buf[(right, bottom)].set_symbol("╯");
    }
    let room = (r.width as usize).saturating_sub(3);
    let tone = if note.is_some() { Tone::NoData } else { Tone::Title };
    let head = format!(" {title} ");
    buf.set_stringn(r.x + 2, r.y, &head, room, Style::default().fg(tone.color()));
    let mut end = 2 + head.chars().count();
    if let Some(note) = note {
        let note = format!("· {note} ");
        buf.set_stringn(r.x + end as u16, r.y, &note, room.saturating_sub(end), Style::default().fg(Tone::Label.color()));
        end += note.chars().count();
    }
    Panel {
        buf,
        frame: r,
        x: r.x + 2,
        y: r.y + 1,
        w: (r.width as usize).saturating_sub(4),
        h: (r.height as usize).saturating_sub(2),
        stale: note.is_some(),
        title_end: end,
    }
}

const LEFT_DOTS: [u32; 4] = [0x40, 0x04, 0x02, 0x01];
const RIGHT_DOTS: [u32; 4] = [0x80, 0x20, 0x10, 0x08];

impl Panel<'_> {
    fn style(&self, tone: Tone) -> Style {
        let tone = if self.stale && tone != Tone::Empty { Tone::NoData } else { tone };
        Style::default().fg(tone.color())
    }


    /// Writes text, clipped to the panel. Returns the column after it.
    pub fn put(&mut self, x: usize, y: usize, s: &str, tone: Tone) -> usize {
        if y >= self.h || x >= self.w {
            return x;
        }
        let room = self.w - x;
        let style = self.style(tone);
        self.buf.set_stringn(self.x + x as u16, self.y + y as u16, s, room, style);
        x + s.chars().count().min(room)
    }

    /// Writes a row of items starting at `x`. A bar fills what is left of `w`.
    pub fn row(&mut self, y: usize, x: usize, w: Option<usize>, items: &[Item]) -> usize {
        let w = w.unwrap_or(self.w.saturating_sub(x));
        let bar_w = w.saturating_sub(width(items)).max(3);
        let mut x = x;
        for it in items {
            match it {
                Item::Text(s, tone) => {
                    self.put(x, y, s, *tone);
                    x += s.chars().count();
                }
                Item::Bar(frac, tone) => {
                    self.bar(x, y, bar_w, *frac, *tone);
                    x += bar_w;
                }
            }
        }
        x
    }

    /// Writes a row aligned to the right edge.
    pub fn right(&mut self, y: usize, items: &[Item]) {
        let x = self.w.saturating_sub(width(items));
        self.row(y, x, None, items);
    }

    pub fn bar(&mut self, x: usize, y: usize, w: usize, frac: f64, tone: Tone) {
        let filled = (frac.clamp(0.0, 1.0) * w as f64).round() as usize;
        for i in 0..w {
            self.put(x + i, y, "▆", if i < filled { tone } else { Tone::Empty });
        }
    }

    /// Area graph of values in 0..1 drawn with braille, two samples per column, newest on the right.
    pub fn graph(&mut self, x: usize, y: usize, w: usize, h: usize, values: &[f64], tone: GraphTone) {
        if w == 0 || h == 0 {
            return;
        }
        let n = 2 * w;
        let tail = &values[values.len().saturating_sub(n)..];
        let vals: Vec<f64> = std::iter::repeat_n(0.0, n - tail.len()).chain(tail.iter().copied()).collect();
        let total = h * 4;
        let dots = |a: f64| {
            let a = a.clamp(0.0, 1.0);
            let d = (a * total as f64).round() as usize;
            if a > 0.003 && d == 0 { 1 } else { d.min(total) }
        };
        for col in 0..w {
            let (da, db) = (dots(vals[2 * col]), dots(vals[2 * col + 1]));
            for r in 0..h {
                let base = (h - 1 - r) * 4;
                let mut code = 0;
                for dot in LEFT_DOTS.iter().take(da.saturating_sub(base)) {
                    code |= dot;
                }
                for dot in RIGHT_DOTS.iter().take(db.saturating_sub(base)) {
                    code |= dot;
                }
                if code == 0 {
                    continue;
                }
                let tone = match tone {
                    GraphTone::Fixed(t) => t,
                    GraphTone::Level(hue) => {
                        let f = ((h - r) as f64 - 0.5) / h as f64;
                        on(lvl(f, 0.6, 0.85), hue)
                    }
                };
                let ch = char::from_u32(0x2800 + code).unwrap_or(' ');
                self.put(x + col, y + r, ch.encode_utf8(&mut [0; 4]), tone);
            }
        }
    }

    /// Small grey text on the right side of the top border, if there is room after the title.
    pub fn title_right(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let s = format!(" {s} ");
        let n = s.chars().count();
        let fw = self.frame.width as usize;
        if fw < n + 4 + self.title_end {
            return;
        }
        let style = self.style(Tone::Label);
        self.buf.set_string(self.frame.x + (fw - 2 - n) as u16, self.frame.y, s, style);
    }
}
