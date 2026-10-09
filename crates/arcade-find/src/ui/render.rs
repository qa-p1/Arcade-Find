//! Draws the overlay into a pixmap (software rendering; the overlay never
//! creates a GPU context).

use resvg::tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

use find_core::fmt;

use super::icons::Icons;
use super::model::{metrics as m, ActionItem, Mode, Overlay, Row};
use super::text::{Style, Text};
use crate::theme::{Palette, Rgba};

pub struct Renderer {
    pub text: Text,
    pub icons: Icons,
    /// Local UTC offset for dates.
    pub utc_offset: i64,
    /// Panel corner radius (0 where the window itself can't be transparent).
    pub radius: f32,
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(Color::from_rgba8(c.r, c.g, c.b, c.a));
    p.anti_alias = true;
    p
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<resvg::tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let k = 0.552_284_8 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn fill_round(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba) {
    if let Some(p) = rounded(x, y, w, h, r) {
        pm.fill_path(&p, &paint(c), FillRule::Winding, Transform::identity(), None);
    }
}

fn fill_rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
    if let Some(r) = Rect::from_xywh(x, y, w.max(0.0), h.max(0.0)) {
        pm.fill_rect(r, &paint(c), Transform::identity(), None);
    }
}

fn blit(pm: &mut Pixmap, icon: &Pixmap, x: f32, y: f32) {
    pm.draw_pixmap(x.round() as i32, y.round() as i32, icon.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
}

/// What the status area at the right of the bar shows.
fn bar_status(o: &Overlay) -> Option<(String, bool, bool)> {
    // (text, is_error, is_accent)
    if let Some(t) = &o.toast {
        return Some((t.text.clone(), t.kind == super::model::ToastKind::Error, t.kind != super::model::ToastKind::Error));
    }
    if let Some(s) = &o.status {
        return Some((s.clone(), false, false));
    }
    if matches!(o.mode, Mode::Results) && !o.rows.is_empty() && !o.info.empty_state {
        let mut s = fmt::count(o.info.matched as u64);
        if o.info.content {
            s = format!("{s} in contents");
        }
        return Some((s, false, false));
    }
    None
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer { text: Text::new(), icons: Icons::default(), utc_offset: find_core::local_offset_secs(), radius: m::RADIUS }
    }

    /// Renders the overlay at `scale` into a new pixmap of the window size.
    pub fn render(&mut self, o: &Overlay, p: &Palette, scale: f32, now: i64) -> Option<Pixmap> {
        let w = (m::WIDTH * scale).round();
        let h = (o.height() * scale).round();
        let mut pm = Pixmap::new(w as u32, h as u32)?;
        let s = scale;
        // Panel.
        fill_round(&mut pm, 0.0, 0.0, w, h, self.radius * s, p.panel);
        if let Some(path) = rounded(0.5 * s, 0.5 * s, w - s, h - s, (self.radius * s - 0.5 * s).max(0.0)) {
            let mut st = Stroke::default();
            st.width = s.max(1.0);
            pm.stroke_path(&path, &paint(p.border), &st, Transform::identity(), None);
        }
        self.bar(&mut pm, o, p, s);
        if o.shows_list() && o.visible_rows() > 0 {
            fill_rect(&mut pm, 0.0, (m::BAR * s).round() - s.max(1.0), w, s.max(1.0), p.separator);
            let top = (m::BAR + m::LIST_PAD) * s;
            match &o.mode {
                Mode::Actions { items, sel, scroll, .. } => self.actions(&mut pm, items, *sel, *scroll, o.visible_rows(), top, p, s),
                Mode::Details { row } => self.details(&mut pm, row, top, p, s, now),
                _ => self.rows(&mut pm, o, top, p, s, now),
            }
        }
        Some(pm)
    }

    fn bar(&mut self, pm: &mut Pixmap, o: &Overlay, p: &Palette, s: f32) {
        let w = m::WIDTH * s;
        let mid = m::BAR * s / 2.0;
        let gsz = (18.0 * s).round() as u32;
        if let Some(icon) = self.icons.find(gsz, if o.input.text.is_empty() { p.muted } else { p.accent }) {
            let icon = icon.clone();
            blit(pm, &icon, 20.0 * s, mid - gsz as f32 / 2.0);
        }
        let style = Style { size: 18.0 * s, weight: 400, color: p.text };
        let mut x = 52.0 * s;
        // Right side: status / toast.
        let small = Style { size: 12.5 * s, weight: 400, color: p.muted };
        let mut right_edge = w - 18.0 * s;
        if let Some((text, err, accent)) = bar_status(o) {
            let st = Style {
                color: if err {
                    p.danger
                } else if accent {
                    p.accent
                } else {
                    p.faint
                },
                ..small
            };
            let fitted = self.text.fit_end(&text, st, 260.0 * s).0;
            let tw = self.text.width(&fitted, st);
            let lh = self.text.line(&fitted, st, &[]).height;
            self.text.draw(pm, &fitted, st, &[], right_edge - tw, mid - lh / 2.0, (0, 0, pm.width() as i32, pm.height() as i32));
            right_edge -= tw + 14.0 * s;
        }
        if o.show_hidden {
            let st = Style { size: 11.5 * s, weight: 500, color: p.muted };
            let label = "Hidden";
            let tw = self.text.width(label, st);
            let cw = tw + 14.0 * s;
            let ch = 20.0 * s;
            fill_round(pm, right_edge - cw, mid - ch / 2.0, cw, ch, 6.0 * s, p.chip);
            let lh = self.text.line(label, st, &[]).height;
            self.text.draw(pm, label, st, &[], right_edge - cw + 7.0 * s, mid - lh / 2.0, (0, 0, pm.width() as i32, pm.height() as i32));
            right_edge -= cw + 10.0 * s;
        }
        // Actions mode: a chip naming the target, then the action filter.
        let (field, placeholder) = match &o.mode {
            Mode::Actions { filter, targets, .. } => {
                let label = if targets.len() == 1 { targets[0].name.clone() } else { format!("{} items", targets.len()) };
                let st = Style { size: 13.0 * s, weight: 500, color: p.text };
                let label = self.text.fit_end(&label, st, 220.0 * s).0;
                let tw = self.text.width(&label, st);
                let ch = 26.0 * s;
                fill_round(pm, x - 4.0 * s, mid - ch / 2.0, tw + 16.0 * s, ch, 7.0 * s, p.chip);
                let lh = self.text.line(&label, st, &[]).height;
                self.text.draw(pm, &label, st, &[], x + 4.0 * s, mid - lh / 2.0, (0, 0, pm.width() as i32, pm.height() as i32));
                x += tw + 24.0 * s;
                (filter, "Search actions")
            }
            _ => (&o.input, "Search files and folders"),
        };
        let clip = (x as i32, 0, (right_edge - 4.0 * s) as i32, (m::BAR * s) as i32);
        let avail = right_edge - x - 8.0 * s;
        if field.text.is_empty() {
            let ph = Style { color: p.faint, ..style };
            let lh = self.text.line(placeholder, ph, &[]).height;
            self.text.draw(pm, placeholder, ph, &[], x, mid - lh / 2.0, clip);
            fill_rect(pm, x, mid - 11.0 * s, (2.0 * s).max(1.0), 22.0 * s, p.accent);
            return;
        }
        let (lw, lh, caret, sel) = {
            let line = self.text.line(&field.text, style, &[]);
            let caret = Text::caret_x(line, field.cursor);
            let sel = field.selection().map(|(a, b)| (Text::caret_x(line, a), Text::caret_x(line, b)));
            (line.width, line.height, caret, sel)
        };
        // Scroll the field so the caret stays visible.
        let offset = if lw <= avail { 0.0 } else { (caret - avail + 4.0 * s).max(0.0).min(lw - avail + 4.0 * s) };
        let tx = x - offset;
        if let Some((a, b)) = sel {
            fill_round(pm, tx + a, mid - 13.0 * s, b - a, 26.0 * s, 3.0 * s, p.text_selection);
        }
        self.text.draw(pm, &field.text, style, &[], tx, mid - lh / 2.0, clip);
        if matches!(o.mode, Mode::Results | Mode::Actions { .. }) {
            fill_rect(pm, tx + caret, mid - 11.0 * s, (2.0 * s).max(1.0), 22.0 * s, p.accent);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn rows(&mut self, pm: &mut Pixmap, o: &Overlay, top: f32, p: &Palette, s: f32, now: i64) {
        let w = m::WIDTH * s;
        let rh = m::ROW * s;
        let vis = o.visible_rows();
        let full = (0, 0, pm.width() as i32, pm.height() as i32);
        if o.rows.is_empty() {
            let msg = if let Some(n) = &o.info.note {
                n.clone()
            } else if o.info.empty_state {
                "Nothing recent yet".to_string()
            } else if o.info.streaming {
                "Searching contents…".to_string()
            } else {
                "No matches".to_string()
            };
            let st = Style { size: 14.0 * s, weight: 400, color: p.muted };
            let msg = self.text.fit_end(&msg, st, w - 48.0 * s).0;
            let lh = self.text.line(&msg, st, &[]).height;
            self.text.draw(pm, &msg, st, &[], 24.0 * s, top + rh / 2.0 - lh / 2.0, full);
            return;
        }
        let multi = o.anchor.is_some_and(|a| a != o.sel);
        for (vi, i) in (o.scroll..(o.scroll + vis).min(o.rows.len())).enumerate() {
            let r = &o.rows[i];
            let y = top + vi as f32 * rh;
            let selected = o.is_selected(i);
            if selected {
                let c = if multi && i != o.sel { p.selection_multi } else { p.selection };
                fill_round(pm, 6.0 * s, y + 1.0 * s, w - 12.0 * s, rh - 2.0 * s, 9.0 * s, c);
            }
            self.row(pm, o, i, r, y, p, s, now);
        }
        // Scrollbar.
        if o.rows.len() > vis {
            let track = vis as f32 * rh;
            let thumb = (track * vis as f32 / o.rows.len() as f32).max(18.0 * s);
            let ty = top + (track - thumb) * o.scroll as f32 / (o.rows.len() - vis).max(1) as f32;
            fill_round(pm, w - 5.0 * s, ty + 2.0 * s, 3.0 * s, thumb - 4.0 * s, 1.5 * s, p.faint.alpha(110));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn row(&mut self, pm: &mut Pixmap, o: &Overlay, i: usize, r: &Row, y: f32, p: &Palette, s: f32, now: i64) {
        let w = m::WIDTH * s;
        let rh = m::ROW * s;
        let full = (0, 0, pm.width() as i32, pm.height() as i32);
        let isz = (28.0 * s).round() as u32;
        if let Some(icon) = self.icons.kind(r.kind, r.is_symlink, isz, p.panel.alpha(255).mix(p.text, 0.0)) {
            let icon = icon.clone();
            blit(pm, &icon, 18.0 * s, y + (rh - isz as f32) / 2.0);
        }
        let tx = 58.0 * s;
        let meta_w = 112.0 * s;
        let text_w = w - tx - meta_w - 22.0 * s;
        let name_st = Style { size: 14.5 * s, weight: 500, color: if r.missing { p.faint } else { p.text } };
        let sub_st = Style { size: 12.0 * s, weight: 400, color: p.muted };
        let confirm = matches!(&o.mode, Mode::ConfirmTrash { targets } if targets.iter().any(|t| t.path == r.path));
        // Name (or the rename field).
        if let Mode::Rename { row, input } = &o.mode {
            if *row == i {
                let fy = y + 7.0 * s;
                let fh = 24.0 * s;
                let fw = text_w + meta_w;
                fill_round(pm, tx - 6.0 * s, fy, fw, fh, 6.0 * s, p.panel.alpha(255));
                if let Some(path) = rounded(tx - 6.0 * s, fy, fw, fh, 6.0 * s) {
                    let mut st = Stroke::default();
                    st.width = (1.5 * s).max(1.0);
                    pm.stroke_path(&path, &paint(p.accent), &st, Transform::identity(), None);
                }
                let (lh, caret, sel) = {
                    let l = self.text.line(&input.text, name_st, &[]);
                    (l.height, Text::caret_x(l, input.cursor), input.selection().map(|(a, b)| (Text::caret_x(l, a), Text::caret_x(l, b))))
                };
                if let Some((a, b)) = sel {
                    fill_rect(pm, tx + a, fy + 4.0 * s, b - a, fh - 8.0 * s, p.text_selection);
                }
                let clip = (tx as i32, fy as i32, (tx + fw - 12.0 * s) as i32, (fy + fh) as i32);
                self.text.draw(pm, &input.text, name_st, &[], tx, fy + (fh - lh) / 2.0, clip);
                fill_rect(pm, tx + caret, fy + 4.0 * s, (1.5 * s).max(1.0), fh - 8.0 * s, p.accent);
                let parent = self.text.fit_middle(&r.parent, sub_st, text_w);
                self.text.draw(pm, &parent, sub_st, &[], tx, y + 32.0 * s, full);
                return;
            }
        }
        let (name, cut) = self.text.fit_end(&r.name, name_st, if confirm { text_w - 60.0 * s } else { text_w });
        let spans: Vec<(usize, usize, Rgba, u16)> =
            r.highlights.iter().filter(|(a, _)| *a < cut).map(|&(a, b)| (a, b.min(cut), p.highlight, 700)).collect();
        self.text.draw(pm, &name, name_st, &spans, tx, y + 8.0 * s, full);
        let parent = self.text.fit_middle(&r.parent, sub_st, text_w);
        self.text.draw(pm, &parent, sub_st, &[], tx, y + 28.0 * s, full);
        // Right column: date over size, or the trash confirmation.
        let right = w - 22.0 * s;
        if confirm {
            let n = match &o.mode {
                Mode::ConfirmTrash { targets } => targets.len(),
                _ => 1,
            };
            let msg = if n == 1 { "Move to Trash?  ⏎".to_string() } else { format!("Move {n} items to Trash?  ⏎") };
            let st = Style { size: 13.0 * s, weight: 600, color: p.danger };
            let tw = self.text.width(&msg, st);
            let lh = self.text.line(&msg, st, &[]).height;
            fill_round(pm, right - tw - 10.0 * s, y + (rh - 26.0 * s) / 2.0, tw + 20.0 * s, 26.0 * s, 7.0 * s, p.danger.alpha(30));
            self.text.draw(pm, &msg, st, &[], right - tw, y + (rh - lh) / 2.0, full);
            return;
        }
        let meta = Style { size: 12.0 * s, weight: 400, color: p.faint };
        let top_line = if r.missing {
            "Missing".to_string()
        } else if let Some(sec) = r.section.filter(|_| r.pinned) {
            sec.to_string()
        } else {
            fmt::date(r.mtime, now, self.utc_offset)
        };
        let bottom = if r.is_dir {
            if r.is_symlink {
                "Link".to_string()
            } else {
                "Folder".to_string()
            }
        } else {
            r.size.map(fmt::size).unwrap_or_default()
        };
        let tw = self.text.width(&top_line, meta);
        self.text.draw(pm, &top_line, meta, &[], right - tw, y + 9.0 * s, full);
        let bw = self.text.width(&bottom, meta);
        self.text.draw(pm, &bottom, meta, &[], right - bw, y + 28.0 * s, full);
        if r.pinned {
            let gs = (12.0 * s).round() as u32;
            if let Some(icon) = self.icons.glyph(super::model::Glyph::Pin, gs, p.accent) {
                let icon = icon.clone();
                blit(pm, &icon, right - tw - gs as f32 - 6.0 * s, y + 10.0 * s);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn actions(&mut self, pm: &mut Pixmap, items: &[ActionItem], sel: usize, scroll: usize, vis: usize, top: f32, p: &Palette, s: f32) {
        let w = m::WIDTH * s;
        let rh = m::ROW * s;
        let full = (0, 0, pm.width() as i32, pm.height() as i32);
        if items.is_empty() {
            let st = Style { size: 14.0 * s, weight: 400, color: p.muted };
            let lh = self.text.line("No matching actions", st, &[]).height;
            self.text.draw(pm, "No matching actions", st, &[], 24.0 * s, top + rh / 2.0 - lh / 2.0, full);
            return;
        }
        for (vi, i) in (scroll..(scroll + vis).min(items.len())).enumerate() {
            let a = &items[i];
            let y = top + vi as f32 * rh;
            if i == sel {
                fill_round(pm, 6.0 * s, y + 1.0 * s, w - 12.0 * s, rh - 2.0 * s, 9.0 * s, p.selection);
            }
            let color = if a.disabled {
                p.faint
            } else if a.danger {
                p.danger
            } else {
                p.text
            };
            let gs = (18.0 * s).round() as u32;
            if let Some(icon) = self.icons.glyph(
                a.glyph,
                gs,
                if a.disabled {
                    p.faint
                } else if a.danger {
                    p.danger
                } else {
                    p.muted
                },
            ) {
                let icon = icon.clone();
                blit(pm, &icon, 23.0 * s, y + (rh - gs as f32) / 2.0);
            }
            let title = if a.outbound { format!("{} ↗", a.title) } else { a.title.clone() };
            let st = Style { size: 14.5 * s, weight: 500, color };
            let hint_st = Style { size: 12.0 * s, weight: 400, color: p.faint };
            let hint_w = a.hint.as_ref().map(|h| self.text.width(h, hint_st) + 16.0 * s).unwrap_or(0.0);
            let tx = 58.0 * s;
            let max = w - tx - hint_w - 24.0 * s;
            match &a.detail {
                Some(d) => {
                    let t = self.text.fit_end(&title, st, max).0;
                    self.text.draw(pm, &t, st, &[], tx, y + 8.0 * s, full);
                    let ds = Style { size: 12.0 * s, weight: 400, color: p.muted };
                    let d = self.text.fit_end(d, ds, max).0;
                    self.text.draw(pm, &d, ds, &[], tx, y + 28.0 * s, full);
                }
                None => {
                    let t = self.text.fit_end(&title, st, max).0;
                    let lh = self.text.line(&t, st, &[]).height;
                    self.text.draw(pm, &t, st, &[], tx, y + (rh - lh) / 2.0, full);
                }
            }
            if let Some(h) = &a.hint {
                let hw = self.text.width(h, hint_st);
                let lh = self.text.line(h, hint_st, &[]).height;
                self.text.draw(pm, h, hint_st, &[], w - 22.0 * s - hw, y + (rh - lh) / 2.0, full);
            }
        }
        if items.len() > vis {
            let track = vis as f32 * rh;
            let thumb = (track * vis as f32 / items.len() as f32).max(18.0 * s);
            let ty = top + (track - thumb) * scroll as f32 / (items.len() - vis).max(1) as f32;
            fill_round(pm, w - 5.0 * s, ty + 2.0 * s, 3.0 * s, thumb - 4.0 * s, 1.5 * s, p.faint.alpha(110));
        }
    }

    fn details(&mut self, pm: &mut Pixmap, r: &Row, top: f32, p: &Palette, s: f32, now: i64) {
        let w = m::WIDTH * s;
        let full = (0, 0, pm.width() as i32, pm.height() as i32);
        let isz = (48.0 * s).round() as u32;
        if let Some(icon) = self.icons.kind(r.kind, r.is_symlink, isz, p.panel) {
            let icon = icon.clone();
            blit(pm, &icon, 22.0 * s, top + 12.0 * s);
        }
        let tx = 86.0 * s;
        let maxw = w - tx - 24.0 * s;
        let name_st = Style { size: 17.0 * s, weight: 600, color: p.text };
        let name = self.text.fit_end(&r.name, name_st, maxw).0;
        self.text.draw(pm, &name, name_st, &[], tx, top + 12.0 * s, full);
        let lab = Style { size: 12.5 * s, weight: 500, color: p.faint };
        let val = Style { size: 13.0 * s, weight: 400, color: p.text };
        let kind = if r.is_symlink { format!("{} (link)", r.kind.label()) } else { r.kind.label().to_string() };
        let size = if r.is_dir {
            "—".to_string()
        } else {
            r.size.map(|b| format!("{} ({} bytes)", fmt::size(b), fmt::count(b))).unwrap_or_default()
        };
        let modified = if r.mtime > 0 {
            format!("{} · {}", fmt::datetime(r.mtime, self.utc_offset), fmt::date(r.mtime, now, self.utc_offset))
        } else {
            String::new()
        };
        let lines = [("Kind", kind), ("Size", size), ("Modified", modified), ("Where", r.parent.clone())];
        let mut y = top + 44.0 * s;
        for (l, v) in lines {
            self.text.draw(pm, l, lab, &[], tx, y, full);
            let v = self.text.fit_middle(&v, val, maxw - 80.0 * s);
            self.text.draw(pm, &v, val, &[], tx + 80.0 * s, y, full);
            y += 36.0 * s;
        }
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::model::{ResultsInfo, Row};
    use find_core::kind::Kind;
    use std::path::PathBuf;

    fn row(name: &str, hl: Vec<(usize, usize)>) -> Row {
        Row {
            path: PathBuf::from(format!("/home/u/Documents/{name}")),
            name: name.into(),
            parent: "~/Documents/Work/Reports/2026".into(),
            size: Some(1_234_567),
            mtime: 1_790_000_000,
            is_dir: name.ends_with('/'),
            is_symlink: false,
            kind: Kind::of(name, false),
            highlights: hl,
            pinned: false,
            section: None,
            missing: false,
        }
    }

    #[test]
    fn renders_every_state() {
        let mut r = Renderer::new();
        let mut o = Overlay::default();
        o.show(Some("report"), None);
        let rows = vec![
            row("annual-report.pdf", vec![(7, 13)]),
            row("report.docx", vec![(0, 6)]),
            row("a very long file name that will certainly need to be truncated somewhere.txt", vec![]),
        ];
        o.set_results(o.seq, rows, ResultsInfo { matched: 3, ..Default::default() });
        for pal in [Palette::dark(true), Palette::light(false)] {
            for scale in [1.0, 1.5, 2.0] {
                let pm = r.render(&o, &pal, scale, 1_791_000_000).unwrap();
                assert_eq!(pm.width(), (680.0 * scale) as u32);
                assert_eq!(pm.height() as f32, (o.height() * scale).round());
            }
        }
        // Bar only at rest.
        let mut rest = Overlay::default();
        rest.show(None, None);
        assert_eq!(r.render(&rest, &Palette::dark(true), 1.0, 0).unwrap().height(), 58);
    }
}
