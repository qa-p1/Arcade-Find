//! Draws the overlay into a pixmap (software rendering; the overlay never
//! creates a GPU context). Template B, launcher variant: every size, radius
//! and type style comes from the Arcade design kit.

use arcade_link::design::{icon as kit_icon, radius, size, text as kit, PathOp, TypeStyle, SPACE};
use resvg::tiny_skia::{Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

use super::icons::Icons;
use super::model::{metrics as m, ActionItem, Entry, Mode, Overlay, Row, ToastKind};
use super::text::{Style, Text};
use crate::theme::{Palette, Rgba};

pub struct Renderer {
    pub text: Text,
    pub icons: Icons,
    /// Local UTC offset for dates.
    pub utc_offset: i64,
    /// Panel corner radius (0 where the window itself can't be transparent).
    pub radius: f32,
    /// Transparent margin for the panel's shadow (0 where the window is opaque).
    pub shadow: f32,
    shadow_cache: Option<((u32, u32, bool), Pixmap)>,
}

/// A type-scale entry at `s`, in `color` (`weight` overrides the token's).
fn style(t: TypeStyle, weight: u16, color: Rgba, s: f32) -> Style {
    Style { size: t.size * s, weight, color }
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

/// A 1 px (at `s`) outline inside the rounded rectangle.
fn stroke_round(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba, s: f32) {
    let half = s.max(1.0) / 2.0;
    if let Some(path) = rounded(x + half, y + half, w - 2.0 * half, h - 2.0 * half, (r - half).max(0.0)) {
        let st = Stroke { width: s.max(1.0), ..Stroke::default() };
        pm.stroke_path(&path, &paint(c), &st, Transform::identity(), None);
    }
}

/// Fills a rectangle snapped to whole pixels: carets, separators and
/// selections stay crisp, and tiny-skia's anti-aliased rect path never sees a
/// fractional sliver narrower than a pixel (it debug-asserts on those).
fn fill_rect(pm: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let (l, t) = (x.round(), y.round());
    let (r, b) = ((x + w).round().max(l + 1.0), (y + h).round().max(t + 1.0));
    if let Some(r) = Rect::from_ltrb(l, t, r, b) {
        pm.fill_rect(r, &paint(c), Transform::identity(), None);
    }
}

fn blit(pm: &mut Pixmap, icon: &Pixmap, x: f32, y: f32) {
    pm.draw_pixmap(x.round() as i32, y.round() as i32, icon.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
}

/// Draws a kit icon (24 grid, 1.5 stroke) centred on (cx, cy) at `px` size.
fn icon(pm: &mut Pixmap, name: &str, cx: f32, cy: f32, px: f32, color: Rgba, s: f32) {
    let Some(i) = kit_icon(name) else { return };
    let (ox, oy) = (cx - px / 2.0, cy - px / 2.0);
    let mut pb = PathBuilder::new();
    for op in i.path(px / 24.0) {
        match op {
            PathOp::Move(x, y) => pb.move_to(ox + x, oy + y),
            PathOp::Line(x, y) => pb.line_to(ox + x, oy + y),
            PathOp::Cubic(a, b, c, d, x, y) => pb.cubic_to(ox + a, oy + b, ox + c, oy + d, ox + x, oy + y),
            PathOp::Close => pb.close(),
        }
    }
    let Some(path) = pb.finish() else { return };
    if i.filled {
        pm.fill_path(&path, &paint(color), FillRule::Winding, Transform::identity(), None);
    } else {
        let st = Stroke { width: size::ICON_STROKE * s, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
        pm.stroke_path(&path, &paint(color), &st, Transform::identity(), None);
    }
}

/// A key's label on a chip: platform names for modifiers, symbols for keys.
pub fn key_label(key: &str) -> String {
    let mac = cfg!(target_os = "macos");
    match key {
        "Enter" => "↵".into(),
        "Ctrl" if mac => "⌘".into(),
        "Shift" if mac => "⇧".into(),
        "Alt" if mac => "⌥".into(),
        other => other.into(),
    }
}

/// What the footer hint bar lists for the overlay's state: (keys, label).
pub fn hints(o: &Overlay, previews: bool) -> Vec<(&'static [&'static str], &'static str)> {
    let primary = if previews { "Quick Look" } else { "Open" };
    match &o.mode {
        Mode::Actions { .. } => vec![(&["Enter"], "Run"), (&["Esc"], "Back")],
        Mode::Rename { .. } => vec![(&["Enter"], "Rename"), (&["Esc"], "Cancel")],
        Mode::ConfirmTrash { .. } => vec![(&["Enter"], "Move to Trash"), (&["Esc"], "Keep")],
        Mode::Details { .. } => vec![(&["Enter"], primary), (&["Esc"], "Back")],
        Mode::Results if o.rows.is_empty() && o.offers_content_search() => vec![(&["Enter"], "Search inside files"), (&["Esc"], "Close")],
        Mode::Results if o.rows.is_empty() => vec![(&["Esc"], "Close")],
        Mode::Results => vec![(&["Enter"], primary), (&["Ctrl", "Enter"], "Show in folder"), (&["Tab"], "Actions"), (&["Esc"], "Close")],
    }
}

/// The footer's trailing text: (text, is an error).
fn footer_status(o: &Overlay) -> Option<(String, bool)> {
    use arcade_link::design::format::format_count;
    if o.index.error.is_some() && !o.rows.is_empty() {
        return Some(("Index problem: see Settings".into(), true));
    }
    if let Some(n) = o.index.indexing.filter(|_| !o.rows.is_empty()) {
        return Some((format!("Indexing… {} files", format_count(n as i64)), false));
    }
    if !matches!(o.mode, Mode::Results) || o.rows.is_empty() || o.info.empty_state {
        return None;
    }
    if o.info.streaming {
        return Some(("Searching inside files…".into(), false));
    }
    if let Some(note) = &o.info.note {
        return Some((note.clone(), false));
    }
    let n = format_count(o.info.matched as i64);
    let what = if o.info.matched == 1 { "result" } else { "results" };
    Some((if o.info.content { format!("{n} {what} in contents") } else { format!("{n} {what}") }, false))
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer { text: Text::new(), icons: Icons::default(), utc_offset: find_core::local_offset_secs(), radius: m::RADIUS, shadow: 0.0, shadow_cache: None }
    }

    /// "Just now", "5 min ago", "Yesterday 14:05", "12 Oct" (the kit helper).
    fn relative(&self, t: i64, now: i64) -> String {
        if t <= 0 {
            return String::new();
        }
        arcade_link::design::format::format_relative(t * 1000, now * 1000, (self.utc_offset / 60) as i32)
    }

    /// Renders the overlay at `scale` into a new pixmap of the window size
    /// (the panel plus any shadow margin). `previews`: Enter opens Quick Look.
    pub fn render(&mut self, o: &Overlay, p: &Palette, scale: f32, now: i64, previews: bool) -> Option<Pixmap> {
        let panel = self.panel(o, p, scale, now, previews)?;
        if self.shadow <= 0.0 {
            return Some(panel);
        }
        let margin = (self.shadow * scale).round();
        let (w, h) = (panel.width() + 2 * margin as u32, panel.height() + 2 * margin as u32);
        let key = (w, h, p.dark);
        if self.shadow_cache.as_ref().is_none_or(|(k, _)| *k != key) {
            self.shadow_cache = Some((key, shadow(w, h, margin, self.radius * scale, scale, p.dark)?));
        }
        let mut pm = self.shadow_cache.as_ref()?.1.clone();
        pm.draw_pixmap(margin as i32, margin as i32, panel.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        Some(pm)
    }

    fn tooltip_layout(&mut self, o: &Overlay, p: &Palette, s: f32) -> Option<(Rect, Vec<String>)> {
        let row = o.tooltip_row()?;
        let st = style(kit::CAPTION, kit::CAPTION.weight, p.text, s);
        let pad = SPACE[3] * s;
        let available = (m::WIDTH - SPACE[8] * 2.0) * s;
        let lines = self.text.wrap_lines(&row.name, st, available - pad * 2.0);
        let width = lines.iter().map(|line| self.text.width(line, st)).fold(0.0, f32::max) + pad * 2.0;
        let height = lines.len() as f32 * kit::CAPTION.line * s + SPACE[2] * 2.0 * s;
        let (index, _) = o.hovered_row?;
        let row_y = o.layout().into_iter().find_map(|(y, entry)| matches!(entry, Entry::Row(i) if i == index).then_some(y))?;
        let y = ((m::LIST_TOP + row_y + m::ROW + SPACE[1]) * s).min((o.height() - SPACE[1]) * s - height).max(SPACE[1] * s);
        Some((Rect::from_xywh(SPACE[8] * s, y, width, height)?, lines))
    }

    fn panel(&mut self, o: &Overlay, p: &Palette, s: f32, now: i64, previews: bool) -> Option<Pixmap> {
        let w = (m::WIDTH * s).round();
        let h = (o.height() * s).round();
        let mut pm = Pixmap::new(w as u32, h as u32)?;
        fill_round(&mut pm, 0.0, 0.0, w, h, self.radius * s, p.panel);
        stroke_round(&mut pm, 0.0, 0.0, w, h, self.radius * s, p.border, s);
        self.bar(&mut pm, o, p, s);
        let list = o.list_height();
        if list > 0.0 {
            fill_rect(&mut pm, 0.0, (m::BAR * s).round() - s.max(1.0), w, s.max(1.0), p.separator);
            let top = m::LIST_TOP * s;
            match &o.mode {
                Mode::Actions { items, sel, scroll, filter, .. } => self.actions(&mut pm, o, items, *sel, *scroll, &filter.text, top, p, s),
                Mode::Details { row } => self.details(&mut pm, row, top, p, s, now),
                _ => self.rows(&mut pm, o, top, p, s, now),
            }
        }
        let footer = o.footer_height();
        if footer > 0.0 {
            let y = (o.height() - footer) * s;
            fill_rect(&mut pm, 0.0, y.round(), w, s.max(1.0), p.separator);
            match &o.toast {
                Some(t) => self.toast(&mut pm, &t.text, t.kind, y, p, s),
                None => self.footer(&mut pm, o, previews, y, p, s),
            }
        }
        if let Some((rect, lines)) = self.tooltip_layout(o, p, s) {
            fill_round(&mut pm, rect.x(), rect.y(), rect.width(), rect.height(), radius::SM * s, p.panel);
            stroke_round(&mut pm, rect.x(), rect.y(), rect.width(), rect.height(), radius::SM * s, p.border, s);
            let st = style(kit::CAPTION, kit::CAPTION.weight, p.text, s);
            let clip = (rect.left() as i32, rect.top() as i32, rect.right() as i32, rect.bottom() as i32);
            for (index, line) in lines.iter().enumerate() {
                self.text.draw(&mut pm, line, st, &[], rect.left() + SPACE[3] * s, rect.top() + SPACE[2] * s + index as f32 * kit::CAPTION.line * s, clip);
            }
        }
        Some(pm)
    }

    /// The image that follows the pointer while result rows are dragged:
    /// the first item's icon and name, and "+N" for the rest.
    pub fn drag_image(&mut self, rows: &[Row], p: &Palette, s: f32) -> Option<Pixmap> {
        let first = rows.first()?;
        let h = size::ROW_OVERLAY * s;
        let isz = (size::TILE_SM * s).round() as u32;
        let name_st = style(kit::HEADING, 500, p.text, s);
        let name = self.text.fit_end(&first.name, name_st, 220.0 * s).0;
        let name_w = self.text.width(&name, name_st);
        let more = (rows.len() > 1).then(|| format!("+{}", rows.len() - 1));
        let badge_st = style(kit::CAPTION, 600, p.accent_text, s);
        let badge_w = more.as_ref().map(|t| self.text.width(t, badge_st) + 14.0 * s).unwrap_or(0.0);
        let pad = 6.0 * s;
        let w = (pad + isz as f32 + 10.0 * s + name_w + if more.is_some() { 10.0 * s + badge_w } else { 0.0 } + 14.0 * s).ceil();
        let mut pm = Pixmap::new(w as u32, h.ceil() as u32)?;
        fill_round(&mut pm, 0.0, 0.0, w, h, radius::MD * s, p.panel);
        stroke_round(&mut pm, 0.0, 0.0, w, h, radius::MD * s, p.border, s);
        if let Some(icon) = self.icons.kind(first.kind, first.is_symlink, isz, p) {
            let icon = icon.clone();
            blit(&mut pm, &icon, pad, (h - isz as f32) / 2.0);
        }
        let full = (0, 0, pm.width() as i32, pm.height() as i32);
        let tx = pad + isz as f32 + 10.0 * s;
        let lh = self.text.line(&name, name_st, &[]).height;
        self.text.draw(&mut pm, &name, name_st, &[], tx, (h - lh) / 2.0, full);
        if let Some(t) = more {
            let bx = tx + name_w + 10.0 * s;
            let bh = 20.0 * s;
            fill_round(&mut pm, bx, (h - bh) / 2.0, badge_w, bh, bh / 2.0, p.accent);
            let blh = self.text.line(&t, badge_st, &[]).height;
            self.text.draw(&mut pm, &t, badge_st, &[], bx + 7.0 * s, (h - blh) / 2.0, full);
        }
        Some(pm)
    }

    /// A kit key chip (22 px, at least as wide, 2 px bottom edge) with its
    /// left edge at `x`, centred on `cy`. Returns its width.
    fn key_chip(&mut self, pm: &mut Pixmap, key: &str, x: f32, cy: f32, p: &Palette, s: f32) -> f32 {
        let label = key_label(key);
        let st = style(kit::CAPTION, 500, p.text, s);
        let tw = self.text.width(&label, st);
        let (cw, ch) = ((tw + 12.0 * s).max(size::KEY_CHIP * s), size::KEY_CHIP * s);
        let y = cy - ch / 2.0;
        fill_round(pm, x, y, cw, ch, radius::XS * s, p.chip);
        stroke_round(pm, x, y, cw, ch, radius::XS * s, p.chip_edge, s);
        fill_rect(pm, x + radius::XS * s, y + ch - 2.0 * s, cw - 2.0 * radius::XS * s, s.max(1.0), p.chip_edge);
        let lh = self.text.line(&label, st, &[]).height;
        let full = (0, 0, pm.width() as i32, pm.height() as i32);
        self.text.draw(pm, &label, st, &[], x + (cw - tw) / 2.0, y + (ch - lh) / 2.0 - 0.5 * s, full);
        cw
    }

    /// Key chips for `keys`, 4 px apart. Returns the total width.
    fn key_chips(&mut self, pm: &mut Pixmap, keys: &[&str], x: f32, cy: f32, p: &Palette, s: f32) -> f32 {
        let mut at = x;
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                at += 4.0 * s;
            }
            at += self.key_chip(pm, k, at, cy, p, s);
        }
        at - x
    }

    /// Width of [`Renderer::key_chips`] without drawing.
    fn key_chips_width(&mut self, keys: &[&str], p: &Palette, s: f32) -> f32 {
        let st = style(kit::CAPTION, 500, p.text, s);
        keys.iter().map(|k| (self.text.width(&key_label(k), st) + 12.0 * s).max(size::KEY_CHIP * s)).sum::<f32>()
            + keys.len().saturating_sub(1) as f32 * 4.0 * s
    }

    /// A kit badge (20 px pill, caption) ending at `right`. Returns its width.
    fn badge(&mut self, pm: &mut Pixmap, label: &str, accent: bool, right: f32, cy: f32, p: &Palette, s: f32) -> f32 {
        let (fg, bg) = if accent { (p.highlight, p.selection) } else { (p.muted, p.chip) };
        let st = style(kit::CAPTION, 500, fg, s);
        let tw = self.text.width(label, st);
        let (bw, bh) = (tw + 14.0 * s, 20.0 * s);
        let x = right - bw;
        fill_round(pm, x, cy - bh / 2.0, bw, bh, bh / 2.0, bg);
        if !accent {
            stroke_round(pm, x, cy - bh / 2.0, bw, bh, bh / 2.0, p.border, s);
        }
        let lh = self.text.line(label, st, &[]).height;
        self.text.draw(pm, label, st, &[], x + 7.0 * s, cy - lh / 2.0, (0, 0, pm.width() as i32, pm.height() as i32));
        bw
    }

    fn bar(&mut self, pm: &mut Pixmap, o: &Overlay, p: &Palette, s: f32) {
        let w = m::WIDTH * s;
        let mid = m::BAR * s / 2.0;
        icon(pm, "search", 20.0 * s + size::ICON_TILE * s / 2.0, mid, size::ICON_TILE * s, p.muted, s);
        let field_st = style(kit::LAUNCHER, 400, p.text, s);
        let mut x = 52.0 * s;
        // Trailing badges: the search mode, then hidden files.
        let mut right_edge = w - 16.0 * s;
        if o.info.content && matches!(o.mode, Mode::Results) && !o.input.text.is_empty() {
            right_edge -= self.badge(pm, "Content", true, right_edge, mid, p, s) + 8.0 * s;
        }
        if o.show_hidden {
            right_edge -= self.badge(pm, "Hidden", false, right_edge, mid, p, s) + 8.0 * s;
        }
        // Actions mode: a chip naming the target, then the action filter.
        let (field, placeholder) = match &o.mode {
            Mode::Actions { filter, targets, .. } => {
                let label = if targets.len() == 1 { targets[0].name.clone() } else { format!("{} items", targets.len()) };
                let st = style(kit::BODY, 500, p.text, s);
                let label = self.text.fit_end(&label, st, 220.0 * s).0;
                let tw = self.text.width(&label, st);
                let ch = size::CONTROL_SM * s;
                fill_round(pm, x - 4.0 * s, mid - ch / 2.0, tw + 20.0 * s, ch, radius::SM * s, p.chip);
                let lh = self.text.line(&label, st, &[]).height;
                self.text.draw(pm, &label, st, &[], x + 6.0 * s, mid - lh / 2.0, (0, 0, pm.width() as i32, pm.height() as i32));
                x += tw + 28.0 * s;
                (filter, "Search actions")
            }
            _ => (&o.input, "Search files and folders"),
        };
        let clip = (x as i32, 0, (right_edge - 4.0 * s) as i32, (m::BAR * s) as i32);
        let avail = right_edge - x - 8.0 * s;
        let caret_h = 22.0 * s;
        let preedit = if matches!(o.mode, Mode::Results | Mode::Actions { .. }) { o.preedit.as_str() } else { "" };
        if field.text.is_empty() && preedit.is_empty() {
            let ph = Style { color: p.muted, ..field_st };
            let lh = self.text.line(placeholder, ph, &[]).height;
            self.text.draw(pm, placeholder, ph, &[], x, mid - lh / 2.0, clip);
            fill_rect(pm, x, mid - caret_h / 2.0, (2.0 * s).max(1.0), caret_h, p.accent);
            return;
        }
        // Composed (IME) text is shown at the caret, underlined, until committed.
        let cursor = field.cursor.min(field.text.len());
        let shown = format!("{}{}{}", &field.text[..cursor], preedit, &field.text[cursor..]);
        let (lw, lh, caret, sel, pre) = {
            let line = self.text.line(&shown, field_st, &[]);
            let caret = Text::caret_x(line, cursor + preedit.len());
            let sel = if preedit.is_empty() { field.selection().map(|(a, b)| (Text::caret_x(line, a), Text::caret_x(line, b))) } else { None };
            let pre = (!preedit.is_empty()).then(|| (Text::caret_x(line, cursor), caret));
            (line.width, line.height, caret, sel, pre)
        };
        // Scroll the field so the caret stays visible.
        let offset = if lw <= avail { 0.0 } else { (caret - avail + 4.0 * s).max(0.0).min(lw - avail + 4.0 * s) };
        let tx = x - offset;
        if let Some((a, b)) = sel {
            fill_round(pm, tx + a, mid - 13.0 * s, b - a, 26.0 * s, 3.0 * s, p.text_selection);
        }
        self.text.draw(pm, &shown, field_st, &[], tx, mid - lh / 2.0, clip);
        if let Some((a, b)) = pre {
            fill_rect(pm, tx + a, mid + lh / 2.0, b - a, s.max(1.0), p.text);
        }
        if matches!(o.mode, Mode::Results | Mode::Actions { .. }) {
            fill_rect(pm, tx + caret, mid - caret_h / 2.0, (2.0 * s).max(1.0), caret_h, p.accent);
        }
    }

    /// The single-line state shown when the list has no rows.
    fn empty(&mut self, pm: &mut Pixmap, o: &Overlay, top: f32, p: &Palette, s: f32) {
        use arcade_link::design::format::format_count;
        let w = m::WIDTH * s;
        let rh = m::ROW * s;
        let full = (0, 0, pm.width() as i32, pm.height() as i32);
        let x = 20.0 * s;
        let line = style(kit::HEADING, 400, p.muted, s);
        if let Some(err) = &o.index.error {
            // A danger notice with the way to Settings.
            let (nx, ny, nw, nh) = (8.0 * s, top + 6.0 * s, w - 16.0 * s, rh - 12.0 * s);
            fill_round(pm, nx, ny, nw, nh, radius::MD * s, p.danger_soft);
            stroke_round(pm, nx, ny, nw, nh, radius::MD * s, p.border.mix(p.danger, 0.35), s);
            icon(pm, "alert", nx + 14.0 * s + size::ICON * s / 2.0, ny + nh / 2.0, size::ICON * s, p.danger, s);
            let label = "Open settings";
            let st = style(kit::CONTROL, 500, p.text, s);
            let lw = self.text.width(label, st);
            let cw = self.key_chips_width(&["Ctrl", ","], p, s);
            let right = nx + nw - 14.0 * s;
            self.key_chips(pm, &["Ctrl", ","], right - lw - 6.0 * s - cw, ny + nh / 2.0, p, s);
            let lh = self.text.line(label, st, &[]).height;
            self.text.draw(pm, label, st, &[], right - lw, ny + (nh - lh) / 2.0, full);
            let tx = nx + 14.0 * s + size::ICON * s + 10.0 * s;
            let msg = self.text.fit_end(err, st, right - lw - cw - 20.0 * s - tx).0;
            self.text.draw(pm, &msg, st, &[], tx, ny + (nh - lh) / 2.0, full);
            return;
        }
        if o.index.loading || o.index.indexing.is_some() {
            let msg = match o.index.indexing {
                Some(n) => format!("Indexing… {} files", format_count(n as i64)),
                None => "Loading the index…".to_string(),
            };
            self.text.draw(pm, &msg, line, &[], x, top + 10.0 * s, full);
            // A progress line: the scan has no known end, so a segment moves
            // along the track as progress arrives.
            let (ty, tw) = (top + 38.0 * s, w - 2.0 * x);
            fill_round(pm, x, ty, tw, 2.0 * s, s, p.border);
            let at = (o.index.indexing.unwrap_or(0) as f32 / 2000.0).fract();
            fill_round(pm, x + at * tw * 0.7, ty, tw * 0.3, 2.0 * s, s, p.accent);
            return;
        }
        let query = o.input.text.trim();
        let msg = if let Some(n) = &o.info.note {
            n.clone()
        } else if o.info.empty_state || query.is_empty() {
            "Type to search your files".to_string()
        } else if o.info.streaming {
            "Searching inside files…".to_string()
        } else {
            format!("No matches for \u{201c}{query}\u{201d}")
        };
        let msg = self.text.fit_end(&msg, line, w - 2.0 * x).0;
        let lh = self.text.line(&msg, line, &[]).height;
        if o.offers_content_search() {
            self.text.draw(pm, &msg, line, &[], x, top + 6.0 * s, full);
            let cy = top + 38.0 * s;
            let cw = self.key_chip(pm, "/", x, cy, p, s);
            let st = style(kit::SMALL, 400, p.muted, s);
            let hl = self.text.line("Search inside files", st, &[]).height;
            self.text.draw(pm, "Search inside files", st, &[], x + cw + 6.0 * s, cy - hl / 2.0, full);
        } else {
            self.text.draw(pm, &msg, line, &[], x, top + (rh - lh) / 2.0, full);
        }
    }

    fn section(&mut self, pm: &mut Pixmap, label: &str, y: f32, p: &Palette, s: f32) {
        let st = style(kit::CAPTION, 500, p.muted, s);
        let lh = self.text.line(label, st, &[]).height;
        let full = (0, 0, pm.width() as i32, pm.height() as i32);
        self.text.draw(pm, label, st, &[], 20.0 * s, y + m::SECTION * s - lh - 4.0 * s, full);
    }

    fn scrollbar(&mut self, pm: &mut Pixmap, len: usize, vis: usize, scroll: usize, top: f32, height: f32, p: &Palette, s: f32) {
        if len <= vis {
            return;
        }
        let w = m::WIDTH * s;
        let thumb = (height * vis as f32 / len as f32).max(18.0 * s);
        let ty = top + (height - thumb) * scroll as f32 / (len - vis).max(1) as f32;
        fill_round(pm, w - 6.0 * s, ty + 2.0 * s, 3.0 * s, thumb - 4.0 * s, 1.5 * s, p.chip_edge);
    }

    /// The selected-row fill (inset, `radius-sm`) with the 2 px leading bar.
    fn selection(&mut self, pm: &mut Pixmap, y: f32, fill: Rgba, bar: bool, p: &Palette, s: f32) {
        let w = m::WIDTH * s;
        let rh = m::ROW * s;
        fill_round(pm, 6.0 * s, y + s, w - 12.0 * s, rh - 2.0 * s, radius::SM * s, fill);
        if bar {
            fill_round(pm, 6.0 * s, y + 12.0 * s, size::SELECTION_BAR * s, rh - 24.0 * s, s, p.accent);
        }
    }

    fn rows(&mut self, pm: &mut Pixmap, o: &Overlay, top: f32, p: &Palette, s: f32, now: i64) {
        if o.rows.is_empty() {
            self.empty(pm, o, top, p, s);
            return;
        }
        let multi = o.anchor.is_some_and(|a| a != o.sel);
        for (y, entry) in o.layout() {
            let y = top + y * s;
            match entry {
                Entry::Section(label) => self.section(pm, label, y, p, s),
                Entry::Row(i) => {
                    if o.is_selected(i) {
                        let fill = if multi && i != o.sel { p.selection_multi } else { p.selection };
                        self.selection(pm, y, fill, i == o.sel, p, s);
                    }
                    self.row(pm, o, i, &o.rows[i], y, multi && o.is_selected(i), p, s, now);
                }
            }
        }
        self.scrollbar(pm, o.rows.len(), o.visible_rows(), o.scroll, top, o.list_height() * s, p, s);
    }

    #[allow(clippy::too_many_arguments)]
    fn row(&mut self, pm: &mut Pixmap, o: &Overlay, i: usize, r: &Row, y: f32, checked: bool, p: &Palette, s: f32, now: i64) {
        use arcade_link::design::format::format_size;
        let w = m::WIDTH * s;
        let rh = m::ROW * s;
        let full = (0, 0, pm.width() as i32, pm.height() as i32);
        let isz = (size::TILE_SM * s).round() as u32;
        let (ix, iy) = (18.0 * s, y + (rh - isz as f32) / 2.0);
        if let Some(icon) = self.icons.kind(r.kind, r.is_symlink, isz, p) {
            let icon = icon.clone();
            blit(pm, &icon, ix, iy);
        }
        if checked {
            // Part of a multi-selection: a check badge on the icon's corner.
            let d = 14.0 * s;
            let (cx, cy) = (ix + isz as f32 - d / 2.0 + 2.0 * s, iy + isz as f32 - d / 2.0 + 2.0 * s);
            fill_round(pm, cx - d / 2.0 - s, cy - d / 2.0 - s, d + 2.0 * s, d + 2.0 * s, d, p.panel);
            fill_round(pm, cx - d / 2.0, cy - d / 2.0, d, d, d / 2.0, p.accent);
            icon(pm, "check", cx, cy, 10.0 * s, p.accent_text, s);
        }
        let tx = 58.0 * s;
        let meta_w = 112.0 * s;
        let text_w = w - tx - meta_w - 22.0 * s;
        let name_st = style(kit::HEADING, 500, if r.missing { p.muted } else { p.text }, s);
        let sub_st = style(kit::SMALL, 400, p.muted, s);
        let confirm = matches!(&o.mode, Mode::ConfirmTrash { targets } if targets.iter().any(|t| t.path == r.path));
        // Name (or the rename field).
        if let Mode::Rename { row, input } = &o.mode {
            if *row == i {
                let fh = size::CONTROL_SM * s;
                let fy = y + 4.0 * s;
                let fw = text_w + meta_w;
                fill_round(pm, tx - 8.0 * s, fy, fw, fh, radius::SM * s, p.panel);
                stroke_round(pm, tx - 8.0 * s, fy, fw, fh, radius::SM * s, p.accent, s);
                let shown = format!("{}{}{}", &input.text[..input.cursor.min(input.text.len())], o.preedit, &input.text[input.cursor.min(input.text.len())..]);
                let (lh, caret, sel) = {
                    let l = self.text.line(&shown, name_st, &[]);
                    (l.height, Text::caret_x(l, input.cursor + o.preedit.len()), input.selection().map(|(a, b)| (Text::caret_x(l, a), Text::caret_x(l, b))))
                };
                if let Some((a, b)) = sel.filter(|_| o.preedit.is_empty()) {
                    fill_rect(pm, tx + a, fy + 4.0 * s, b - a, fh - 8.0 * s, p.text_selection);
                }
                let clip = (tx as i32, fy as i32, (tx + fw - 16.0 * s) as i32, (fy + fh) as i32);
                self.text.draw(pm, &shown, name_st, &[], tx, fy + (fh - lh) / 2.0, clip);
                fill_rect(pm, tx + caret, fy + 5.0 * s, (1.5 * s).max(1.0), fh - 10.0 * s, p.accent);
                let parent = self.text.fit_middle(&r.parent, sub_st, text_w);
                self.text.draw(pm, &parent, sub_st, &[], tx, y + 33.0 * s, full);
                return;
            }
        }
        let (name, cut) = self.text.fit_end(&r.name, name_st, if confirm { text_w - 60.0 * s } else { text_w - 4.0 * s });
        let spans: Vec<(usize, usize, Rgba, u16)> = r.highlights.iter().filter(|(a, _)| *a < cut).map(|&(a, b)| (a, b.min(cut), p.highlight, 600)).collect();
        let name_clip = (0, 0, (tx + text_w) as i32, pm.height() as i32);
        self.text.draw(pm, &name, name_st, &spans, tx, y + 7.0 * s, name_clip);
        let parent = self.text.fit_middle(&r.parent, sub_st, text_w);
        self.text.draw(pm, &parent, sub_st, &[], tx, y + 28.0 * s, full);
        let right = w - 20.0 * s;
        if confirm {
            let n = match &o.mode {
                Mode::ConfirmTrash { targets } => targets.len(),
                _ => 1,
            };
            let msg = if n == 1 { "Move to Trash?".to_string() } else { format!("Move {n} items to Trash?") };
            let st = style(kit::BODY, 500, p.danger, s);
            let tw = self.text.width(&msg, st);
            let cw = self.key_chips_width(&["Enter"], p, s);
            let ph = size::CONTROL_SM * s;
            let pw = tw + cw + 26.0 * s;
            fill_round(pm, right - pw, y + (rh - ph) / 2.0, pw, ph, radius::SM * s, p.danger_soft);
            let lh = self.text.line(&msg, st, &[]).height;
            self.text.draw(pm, &msg, st, &[], right - pw + 10.0 * s, y + (rh - lh) / 2.0, full);
            self.key_chip(pm, "Enter", right - cw - 6.0 * s, y + rh / 2.0, p, s);
            return;
        }
        // Right column: date over size.
        let meta = style(kit::SMALL, 400, p.muted, s);
        let top_line = if r.missing { "Missing".to_string() } else { self.relative(r.mtime, now) };
        let bottom = if r.is_dir {
            if r.is_symlink {
                "Link".to_string()
            } else {
                "Folder".to_string()
            }
        } else {
            r.size.map(format_size).unwrap_or_default()
        };
        let tw = self.text.width(&top_line, meta);
        self.text.draw(pm, &top_line, meta, &[], right - tw, y + 8.0 * s, full);
        let bw = self.text.width(&bottom, meta);
        self.text.draw(pm, &bottom, meta, &[], right - bw, y + 28.0 * s, full);
        if r.pinned {
            let gs = size::ICON_SMALL * s;
            icon(pm, "pin", right - tw - gs / 2.0 - 6.0 * s, y + 8.0 * s + 8.0 * s, gs, p.highlight, s);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn actions(&mut self, pm: &mut Pixmap, o: &Overlay, items: &[ActionItem], sel: usize, scroll: usize, filter: &str, top: f32, p: &Palette, s: f32) {
        let w = m::WIDTH * s;
        let rh = m::ROW * s;
        let full = (0, 0, pm.width() as i32, pm.height() as i32);
        if items.is_empty() {
            let st = style(kit::HEADING, 400, p.muted, s);
            let msg = format!("Nothing matches \u{201c}{}\u{201d}", filter.trim());
            let msg = self.text.fit_end(&msg, st, w - 40.0 * s).0;
            let lh = self.text.line(&msg, st, &[]).height;
            self.text.draw(pm, &msg, st, &[], 20.0 * s, top + (rh - lh) / 2.0, full);
            return;
        }
        for (y, entry) in o.layout() {
            let Entry::Row(i) = entry else { continue };
            let a = &items[i];
            let y = top + y * s;
            if i == sel {
                self.selection(pm, y, p.selection, true, p, s);
            }
            let color = if a.disabled {
                p.muted
            } else if a.danger {
                p.danger
            } else {
                p.text
            };
            let gs = size::ICON * s;
            let gx = 24.0 * s;
            match a.glyph {
                super::model::Glyph::App(_) => {
                    if let Some(img) = self.icons.glyph(a.glyph, gs.round() as u32, p.muted) {
                        let img = img.clone();
                        blit(pm, &img, gx, y + (rh - gs) / 2.0);
                    }
                }
                g => icon(
                    pm,
                    super::icons::kit_name(g),
                    gx + gs / 2.0,
                    y + rh / 2.0,
                    gs,
                    if a.disabled {
                        p.muted
                    } else if a.danger {
                        p.danger
                    } else {
                        p.muted
                    },
                    s,
                ),
            }
            let title = if a.outbound { format!("{} ↗", a.title) } else { a.title.clone() };
            let st = style(kit::HEADING, 500, color, s);
            let keys: Vec<&str> = a.hint.as_deref().map(|h| h.split('+').collect()).unwrap_or_default();
            let hint_w = if keys.is_empty() { 0.0 } else { self.key_chips_width(&keys, p, s) + 16.0 * s };
            let tx = 58.0 * s;
            let max = w - tx - hint_w - 24.0 * s;
            match &a.detail {
                Some(d) => {
                    let t = self.text.fit_end(&title, st, max).0;
                    self.text.draw(pm, &t, st, &[], tx, y + 7.0 * s, full);
                    let ds = style(kit::SMALL, 400, p.muted, s);
                    let d = self.text.fit_end(d, ds, max).0;
                    self.text.draw(pm, &d, ds, &[], tx, y + 28.0 * s, full);
                }
                None => {
                    let t = self.text.fit_end(&title, st, max).0;
                    let lh = self.text.line(&t, st, &[]).height;
                    self.text.draw(pm, &t, st, &[], tx, y + (rh - lh) / 2.0, full);
                }
            }
            if !keys.is_empty() {
                let kw = self.key_chips_width(&keys, p, s);
                self.key_chips(pm, &keys, w - 20.0 * s - kw, y + rh / 2.0, p, s);
            }
        }
        self.scrollbar(pm, items.len(), o.visible_rows(), scroll, top, o.list_height() * s, p, s);
    }

    fn details(&mut self, pm: &mut Pixmap, r: &Row, top: f32, p: &Palette, s: f32, now: i64) {
        use arcade_link::design::format::{format_count, format_size};
        let w = m::WIDTH * s;
        let full = (0, 0, pm.width() as i32, pm.height() as i32);
        let isz = (size::TILE_LG * s).round() as u32;
        if let Some(icon) = self.icons.kind(r.kind, r.is_symlink, isz, p) {
            let icon = icon.clone();
            blit(pm, &icon, 20.0 * s, top + 12.0 * s);
        }
        let tx = 88.0 * s;
        let maxw = w - tx - 24.0 * s;
        let name_st = style(kit::TITLE_SM, 600, p.text, s);
        let name = self.text.fit_end(&r.name, name_st, maxw).0;
        self.text.draw(pm, &name, name_st, &[], tx, top + 12.0 * s, full);
        let lab = style(kit::SMALL, 400, p.muted, s);
        let val = style(kit::BODY, 400, p.text, s);
        let kind = if r.is_symlink { format!("{} (link)", r.kind.label()) } else { r.kind.label().to_string() };
        let size = if r.is_dir {
            "—".to_string()
        } else {
            r.size.map(|b| format!("{} ({} bytes)", format_size(b), format_count(b.min(i64::MAX as u64) as i64))).unwrap_or_default()
        };
        let modified =
            if r.mtime > 0 { format!("{} · {}", find_core::fmt::datetime(r.mtime, self.utc_offset), self.relative(r.mtime, now)) } else { String::new() };
        let lines = [("Kind", kind), ("Size", size), ("Modified", modified), ("Where", r.parent.clone())];
        let mut y = top + 46.0 * s;
        for (l, v) in lines {
            self.text.draw(pm, l, lab, &[], tx, y + 1.0 * s, full);
            let v = self.text.fit_middle(&v, val, maxw - 80.0 * s);
            self.text.draw(pm, &v, val, &[], tx + 80.0 * s, y, full);
            y += 36.0 * s;
        }
    }

    fn footer(&mut self, pm: &mut Pixmap, o: &Overlay, previews: bool, y: f32, p: &Palette, s: f32) {
        let w = m::WIDTH * s;
        let cy = y + m::FOOTER * s / 2.0;
        let full = (0, 0, pm.width() as i32, pm.height() as i32);
        let label_st = style(kit::CAPTION, 500, p.muted, s);
        let mut x = 14.0 * s;
        let mut right = w - 16.0 * s;
        if let Some((text, error)) = footer_status(o) {
            let st = Style { color: if error { p.danger } else { p.muted }, ..label_st };
            let text = self.text.fit_end(&text, st, 220.0 * s).0;
            let tw = self.text.width(&text, st);
            let lh = self.text.line(&text, st, &[]).height;
            self.text.draw(pm, &text, st, &[], right - tw, cy - lh / 2.0, full);
            right -= tw + 16.0 * s;
        }
        for (keys, label) in hints(o, previews) {
            let lw = self.text.width(label, label_st);
            if x + self.key_chips_width(keys, p, s) + 6.0 * s + lw > right {
                break;
            }
            x += self.key_chips(pm, keys, x, cy, p, s) + 6.0 * s;
            let lh = self.text.line(label, label_st, &[]).height;
            self.text.draw(pm, label, label_st, &[], x, cy - lh / 2.0, full);
            x += lw + 16.0 * s;
        }
    }

    /// The kit toast, at the bottom of the panel in the footer's place.
    fn toast(&mut self, pm: &mut Pixmap, text: &str, kind: ToastKind, y: f32, p: &Palette, s: f32) {
        let w = m::WIDTH * s;
        let (tx, ty, tw, th) = (8.0 * s, y + 6.0 * s, w - 16.0 * s, (m::TOAST - 12.0) * s);
        let (glyph, fg, fill, edge) = match kind {
            ToastKind::Error => ("alert", p.danger, p.danger_soft, p.border.mix(p.danger, 0.35)),
            ToastKind::Copied => ("check", p.ok, p.panel, p.border),
            ToastKind::Info => ("info", p.highlight, p.panel, p.border),
        };
        fill_round(pm, tx, ty, tw, th, radius::MD * s, fill);
        stroke_round(pm, tx, ty, tw, th, radius::MD * s, edge, s);
        icon(pm, glyph, tx + 12.0 * s + size::ICON * s / 2.0, ty + th / 2.0, size::ICON * s, fg, s);
        let st = style(kit::CONTROL, 500, p.text, s);
        let x = tx + 12.0 * s + size::ICON * s + 10.0 * s;
        let msg = self.text.fit_end(text, st, tx + tw - 12.0 * s - x).0;
        let lh = self.text.line(&msg, st, &[]).height;
        self.text.draw(pm, &msg, st, &[], x, ty + (th - lh) / 2.0, (0, 0, pm.width() as i32, pm.height() as i32));
    }
}

/// The kit's `shadow-pop` behind a `w`×`h` window whose panel is inset by
/// `margin`, fitted so it fades out completely inside the margin.
fn shadow(w: u32, h: u32, margin: f32, radius: f32, s: f32, dark: bool) -> Option<Pixmap> {
    let mut pm = Pixmap::new(w, h)?;
    let theme = if dark { arcade_link::design::Theme::Dark } else { arcade_link::design::Theme::Light };
    let pop = arcade_link::design::egui::visuals(theme, crate::link::ME).popup_shadow;
    let [r, g, b, alpha] = pop.color.to_srgba_unmultiplied();
    let colour = Rgba { r, g, b, a: alpha };
    let offset = f32::from(pop.offset[1]);
    let offset = (offset * s * 0.5).min(margin / 3.0);
    // One pixel short of the window's edge, so the edge stays clear.
    let reach = margin - offset - s.max(1.0);
    let (pw, ph) = (w as f32 - 2.0 * margin, h as f32 - 2.0 * margin);
    // Nested rings approximate a blur whose strength falls as (1 - d/reach)².
    const STEPS: usize = 14;
    let strength = |d: f32| (1.0 - d / reach).clamp(0.0, 1.0).powi(2);
    for k in (1..=STEPS).rev() {
        let d = reach * k as f32 / STEPS as f32;
        let a = (strength(d - reach / STEPS as f32) - strength(d)) * alpha as f32;
        if a < 0.5 {
            continue;
        }
        fill_round(&mut pm, margin - d, margin + offset - d, pw + 2.0 * d, ph + 2.0 * d, radius + d, colour.alpha(a.round().clamp(1.0, 255.0) as u8));
    }
    Some(pm)
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer::new()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn thin_fractional_rects_are_snapped() {
        let mut pm = Pixmap::new(64, 32).unwrap();
        let c = Rgba::hex(0xff0000);
        for i in 0..40 {
            let x = 3.0 + i as f32 * 0.37;
            super::fill_rect(&mut pm, x, 2.3, 1.5, 20.6, c);
            super::fill_rect(&mut pm, x, 4.1, 2.0 * 1.25, 0.6, c);
        }
    }

    use super::*;
    use crate::ui::model::{ResultsInfo, Row};
    use find_core::kind::Kind;
    use std::path::PathBuf;

    #[test]
    fn long_unicode_name_tooltips_keep_all_text_inside_the_small_overlay() {
        for palette in [Palette::light(), Palette::dark()] {
            for scale in [1.0, 1.25] {
                let mut renderer = Renderer::new();
                let mut overlay = Overlay::default();
                overlay.show(Some("long"), None);
                let name = "café-日本-wide-name-".repeat(10) + ".txt";
                overlay.set_results(overlay.seq, vec![row(&name, vec![])], ResultsInfo::default());
                overlay.hover(Some(0));
                overlay.tick(overlay.feedback_deadline().unwrap());
                let (rect, lines) = renderer.tooltip_layout(&overlay, &palette, scale).unwrap();
                assert_eq!(lines.concat(), name, "a tooltip must expose the complete name");
                assert!(lines.len() > 1, "the long name wraps instead of clipping");
                assert!(rect.left() >= 0.0 && rect.right() <= m::WIDTH * scale);
                assert!(rect.top() >= 0.0 && rect.bottom() <= overlay.height() * scale);
                let st = style(kit::CAPTION, kit::CAPTION.weight, palette.text, scale);
                for line in lines {
                    assert!(renderer.text.width(&line, st) <= rect.width() - SPACE[3] * 2.0 * scale + 0.1);
                }
            }
        }
    }

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
        for pal in [Palette::dark(), Palette::light()] {
            for scale in [1.0, 1.5, 2.0] {
                let pm = r.render(&o, &pal, scale, 1_791_000_000, true).unwrap();
                assert_eq!(pm.width(), (680.0 * scale) as u32);
                assert_eq!(pm.height() as f32, (o.height() * scale).round());
            }
        }
        // Bar only at rest.
        let mut rest = Overlay::default();
        rest.show(None, None);
        assert_eq!(r.render(&rest, &Palette::dark(), 1.0, 0, false).unwrap().height(), 56);
    }

    #[test]
    fn the_shadow_margin_surrounds_the_panel_and_fades_out() {
        let mut r = Renderer::new();
        r.shadow = m::SHADOW;
        let mut o = Overlay::default();
        o.show(None, None);
        for pal in [Palette::dark(), Palette::light()] {
            let pm = r.render(&o, &pal, 1.0, 0, false).unwrap();
            assert_eq!((pm.width(), pm.height()), (680 + 80, 56 + 80));
            // Fully transparent at the window's edges, opaque panel inside.
            for (x, y) in [(0, 0), (pm.width() - 1, pm.height() - 1), (pm.width() / 2, pm.height() - 1), (0, pm.height() / 2)] {
                assert_eq!(pm.pixel(x, y).unwrap().alpha(), 0, "edge ({x}, {y}), dark {}", pal.dark);
            }
            assert_eq!(pm.pixel(380, 68).unwrap().alpha(), 255);
            // Some shadow just below the panel.
            assert!(pm.pixel(380, 40 + 56 + 4).unwrap().alpha() > 0);
        }
    }

    #[test]
    fn footer_hints_use_the_overlay_bindings() {
        let mut o = Overlay::default();
        o.show(Some("report"), None);
        o.set_results(o.seq, vec![row("report.pdf", vec![])], ResultsInfo { matched: 1, ..Default::default() });
        let labels: Vec<&str> = hints(&o, true).into_iter().map(|(_, l)| l).collect();
        assert_eq!(labels, ["Quick Look", "Show in folder", "Actions", "Close"]);
        assert_eq!(hints(&o, false)[0], (&["Enter"][..], "Open"));
        assert_eq!(hints(&o, false)[1].0, ["Ctrl", "Enter"]);
        assert_eq!(key_label("Enter"), "↵");
    }
}
