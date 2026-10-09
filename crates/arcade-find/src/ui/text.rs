//! Text shaping (cosmic-text) and glyph blending into a tiny-skia pixmap.

use std::collections::HashMap;

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight, Wrap};
use resvg::tiny_skia::Pixmap;

use crate::theme::Rgba;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub size: f32,
    pub weight: u16,
    pub color: Rgba,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    text: String,
    size: u32,
    weight: u16,
    color: [u8; 4],
    spans: Vec<(usize, usize, [u8; 4], u16)>,
}

/// A shaped single line.
pub struct Line {
    buffer: Buffer,
    pub width: f32,
    pub ascent: f32,
    pub height: f32,
    /// (byte start, x left, x right) per glyph, for carets and cuts.
    pub glyphs: Vec<(usize, usize, f32, f32)>,
}

pub struct Text {
    pub fonts: FontSystem,
    cache: SwashCache,
    lines: HashMap<Key, Line>,
}

fn color(c: Rgba) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a)
}

impl Text {
    pub fn new() -> Text {
        Text { fonts: FontSystem::new(), cache: SwashCache::new(), lines: HashMap::new() }
    }

    fn shape(&mut self, key: &Key, style: Style) -> Line {
        let metrics = Metrics::new(style.size, (style.size * 1.3).ceil());
        let mut buffer = Buffer::new(&mut self.fonts, metrics);
        buffer.set_wrap(&mut self.fonts, Wrap::None);
        buffer.set_size(&mut self.fonts, None, None);
        let base = Attrs::new().family(Family::SansSerif).weight(Weight(style.weight)).color(color(style.color));
        if key.spans.is_empty() {
            buffer.set_text(&mut self.fonts, &key.text, &base, Shaping::Advanced, None);
        } else {
            let mut spans: Vec<(&str, Attrs)> = Vec::new();
            let mut at = 0;
            for &(s, e, c, w) in &key.spans {
                if s > at {
                    spans.push((&key.text[at..s], base.clone()));
                }
                spans.push((&key.text[s..e], base.clone().color(Color::rgba(c[0], c[1], c[2], c[3])).weight(Weight(w))));
                at = e;
            }
            if at < key.text.len() {
                spans.push((&key.text[at..], base.clone()));
            }
            buffer.set_rich_text(&mut self.fonts, spans, &base, Shaping::Advanced, None);
        }
        buffer.shape_until_scroll(&mut self.fonts, false);
        let mut width = 0f32;
        let mut ascent = style.size * 0.95;
        let mut glyphs = Vec::new();
        for run in buffer.layout_runs() {
            width = width.max(run.line_w);
            ascent = run.line_y - run.line_top;
            for g in run.glyphs {
                glyphs.push((g.start, g.end, g.x, g.x + g.w));
            }
        }
        Line { buffer, width, ascent, height: metrics.line_height, glyphs }
    }

    /// Shapes `text` (cached). `spans` color/bold byte ranges.
    pub fn line(&mut self, text: &str, style: Style, spans: &[(usize, usize, Rgba, u16)]) -> &Line {
        if self.lines.len() > 600 {
            self.lines.clear();
        }
        let key = Key {
            text: text.to_string(),
            size: (style.size * 10.0) as u32,
            weight: style.weight,
            color: [style.color.r, style.color.g, style.color.b, style.color.a],
            spans: spans.iter().filter(|s| s.0 < s.1 && s.1 <= text.len() && text.is_char_boundary(s.0) && text.is_char_boundary(s.1)).map(|&(s, e, c, w)| (s, e, [c.r, c.g, c.b, c.a], w)).collect(),
        };
        if !self.lines.contains_key(&key) {
            let l = self.shape(&key, style);
            self.lines.insert(key.clone(), l);
        }
        &self.lines[&key]
    }

    pub fn width(&mut self, text: &str, style: Style) -> f32 {
        self.line(text, style, &[]).width
    }

    /// `text` shortened with "…" at the end to fit `max` (returns the cut byte length).
    pub fn fit_end(&mut self, text: &str, style: Style, max: f32) -> (String, usize) {
        let ell = self.width("…", style);
        let l = self.line(text, style, &[]);
        if l.width <= max {
            return (text.to_string(), text.len());
        }
        let mut cut = 0;
        for &(start, end, _x0, x1) in &l.glyphs {
            if x1 + ell > max {
                break;
            }
            cut = cut.max(end).max(start);
        }
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        (format!("{}…", &text[..cut]), cut)
    }

    /// `text` shortened in the middle ("~/Projects/…/src") to fit `max`.
    pub fn fit_middle(&mut self, text: &str, style: Style, max: f32) -> String {
        let ell = self.width("…", style);
        let l = self.line(text, style, &[]);
        if l.width <= max {
            return text.to_string();
        }
        let total = l.width;
        let head_budget = (max - ell) * 0.35;
        let tail_budget = max - ell - head_budget;
        let mut head = 0;
        let mut tail = text.len();
        for &(start, end, _x0, x1) in &l.glyphs {
            if x1 <= head_budget {
                head = head.max(end);
            }
            if total - (_x0) <= tail_budget {
                tail = tail.min(start);
            }
        }
        while head > 0 && !text.is_char_boundary(head) {
            head -= 1;
        }
        while tail < text.len() && !text.is_char_boundary(tail) {
            tail += 1;
        }
        if tail <= head {
            return self.fit_end(text, style, max).0;
        }
        format!("{}…{}", &text[..head], &text[tail..])
    }

    /// The x offset of byte `i` in a shaped line.
    pub fn caret_x(line: &Line, i: usize) -> f32 {
        let mut x = 0.0f32;
        for &(start, end, x0, x1) in &line.glyphs {
            if i <= start {
                return x0.min(x.max(x0));
            }
            if i < end {
                // Inside a cluster (ligature): interpolate.
                let t = (i - start) as f32 / (end - start).max(1) as f32;
                return x0 + (x1 - x0) * t;
            }
            x = x1;
        }
        x
    }

    /// Draws a shaped line with its top-left at (x, y), clipped to `clip` (x0, y0, x1, y1).
    pub fn draw(&mut self, pm: &mut Pixmap, text: &str, style: Style, spans: &[(usize, usize, Rgba, u16)], x: f32, y: f32, clip: (i32, i32, i32, i32)) -> f32 {
        let key_line_width;
        // Borrow dance: shape first, then draw with the font system.
        {
            let l = self.line(text, style, spans);
            key_line_width = l.width;
        }
        let key = Key {
            text: text.to_string(),
            size: (style.size * 10.0) as u32,
            weight: style.weight,
            color: [style.color.r, style.color.g, style.color.b, style.color.a],
            spans: spans.iter().filter(|s| s.0 < s.1 && s.1 <= text.len() && text.is_char_boundary(s.0) && text.is_char_boundary(s.1)).map(|&(s, e, c, w)| (s, e, [c.r, c.g, c.b, c.a], w)).collect(),
        };
        let Some(line) = self.lines.get(&key) else { return key_line_width };
        let (w, h) = (pm.width() as i32, pm.height() as i32);
        let (cx0, cy0, cx1, cy1) = (clip.0.max(0), clip.1.max(0), clip.2.min(w), clip.3.min(h));
        let data = pm.data_mut();
        for run in line.buffer.layout_runs() {
            for g in run.glyphs {
                let pg = g.physical((x, y), 1.0);
                let c = g.color_opt.unwrap_or(color(style.color));
                let Some(img) = self.cache.get_image(&mut self.fonts, pg.cache_key) else { continue };
                let gx = pg.x + img.placement.left;
                let gy = run.line_y.round() as i32 + pg.y - img.placement.top;
                let (iw, ih) = (img.placement.width as i32, img.placement.height as i32);
                for row in 0..ih {
                    let py = gy + row;
                    if py < cy0 || py >= cy1 {
                        continue;
                    }
                    for col in 0..iw {
                        let px = gx + col;
                        if px < cx0 || px >= cx1 {
                            continue;
                        }
                        let i = (row * iw + col) as usize;
                        let (sr, sg, sb, sa) = match img.content {
                            SwashContent::Mask => {
                                let a = img.data[i] as u32 * c.a() as u32 / 255;
                                (c.r() as u32, c.g() as u32, c.b() as u32, a)
                            }
                            SwashContent::Color => {
                                let o = i * 4;
                                (img.data[o] as u32, img.data[o + 1] as u32, img.data[o + 2] as u32, img.data[o + 3] as u32)
                            }
                            SwashContent::SubpixelMask => {
                                let o = i * 4;
                                let a = (img.data[o] as u32 + img.data[o + 1] as u32 + img.data[o + 2] as u32) / 3 * c.a() as u32 / 255;
                                (c.r() as u32, c.g() as u32, c.b() as u32, a)
                            }
                        };
                        if sa == 0 {
                            continue;
                        }
                        let o = ((py * w + px) * 4) as usize;
                        let inv = 255 - sa;
                        data[o] = ((sr * sa + data[o] as u32 * inv) / 255) as u8;
                        data[o + 1] = ((sg * sa + data[o + 1] as u32 * inv) / 255) as u8;
                        data[o + 2] = ((sb * sa + data[o + 2] as u32 * inv) / 255) as u8;
                        data[o + 3] = (sa + data[o + 3] as u32 * inv / 255).min(255) as u8;
                    }
                }
            }
        }
        key_line_width
    }
}

impl Default for Text {
    fn default() -> Self {
        Text::new()
    }
}
