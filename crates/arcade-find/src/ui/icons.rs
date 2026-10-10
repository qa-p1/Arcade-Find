//! Vector icons: file kinds, actions, and the Arcade apps' glyphs (vendored
//! from Arcade Link `assets/glyphs`, v0.2.0). Rendered once per size and
//! color with resvg and cached.

use std::collections::HashMap;

use find_core::kind::Kind;
use resvg::tiny_skia::{Pixmap, Transform};

use super::model::{AppGlyph, Glyph};
use crate::theme::Rgba;

pub const FIND_GLYPH: &str = include_str!("../../../../assets/glyphs/arcade.find.svg");

const BOX: &str = include_str!("../../../../assets/glyphs/arcade.box.svg");
const LOOK: &str = include_str!("../../../../assets/glyphs/arcade.look.svg");
const WHEEL: &str = include_str!("../../../../assets/glyphs/arcade.wheel.svg");
const CLIPBOARD: &str = include_str!("../../../../assets/glyphs/arcade.clipboard.svg");
const LENS: &str = include_str!("../../../../assets/glyphs/arcade.lens.svg");
const TOOLS: &str = include_str!("../../../../assets/glyphs/arcade.tools.svg");
const SHELF: &str = include_str!("../../../../assets/glyphs/arcade.shelf.svg");
/// Apps the vendored Link assets have no glyph for yet: a neutral app tile.
const OTHER_APP: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><rect x="2.25" y="2.25" width="11.5" height="11.5" rx="3"/><path d="M5.5 8h5M8 5.5v5"/></svg>"##;

fn stroke16(body: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"##
    )
}

fn action_svg(g: Glyph) -> String {
    match g {
        Glyph::Preview => LOOK.to_string(),
        Glyph::Open => stroke16(
            r##"<path d="M9.5 2.25h4.25V6.5M13.75 2.25 7.5 8.5M12 9.5v3.25a1 1 0 0 1-1 1H3.25a1 1 0 0 1-1-1V5a1 1 0 0 1 1-1H6.5"/>"##,
        ),
        Glyph::Reveal => stroke16(
            r##"<path d="M1.75 4.25a1 1 0 0 1 1-1H6l1.5 1.5h5.75a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1H2.75a1 1 0 0 1-1-1z"/><path d="M6 9h4.5M8.75 7.25 10.5 9 8.75 10.75"/>"##,
        ),
        Glyph::Copy => stroke16(
            r##"<rect x="5.25" y="5.25" width="8.5" height="8.5" rx="1.25"/><path d="M10.75 5.25V3.25a1 1 0 0 0-1-1H3.25a1 1 0 0 0-1 1v6.5a1 1 0 0 0 1 1h2"/>"##,
        ),
        Glyph::CopyFile => stroke16(
            r##"<path d="M5.25 5.25h5.25l3.25 3.25v4.25a1 1 0 0 1-1 1h-7.5z"/><path d="M10.5 5.25V8.5h3.25M10.75 5.25V3.25a1 1 0 0 0-1-1H3.25a1 1 0 0 0-1 1v6.5a1 1 0 0 0 1 1h2"/>"##,
        ),
        Glyph::Rename => stroke16(r##"<path d="M10.25 2.75 13.25 5.75 5.75 13.25H2.75v-3z"/><path d="M8.75 4.25 11.75 7.25"/>"##),
        Glyph::Pin => stroke16(r##"<path d="M5.75 2.25h4.5M6.5 2.25v4l-2.25 2.5h7.5L9.5 6.25v-4M8 8.75v5"/>"##),
        Glyph::Trash => stroke16(r##"<path d="M2.75 4.25h10.5M6.25 4.25V2.75h3.5v1.5M4.25 4.25l.75 9h6l.75-9M6.75 6.75v4M9.25 6.75v4"/>"##),
        Glyph::Info => stroke16(r##"<circle cx="8" cy="8" r="6.25"/><path d="M8 7.25v4M8 4.75v.25"/>"##),
        Glyph::App(a) => match a {
            AppGlyph::Box => BOX,
            AppGlyph::Look => LOOK,
            AppGlyph::Wheel => WHEEL,
            AppGlyph::Clipboard => CLIPBOARD,
            AppGlyph::Lens => LENS,
            AppGlyph::Tools => TOOLS,
            AppGlyph::Shelf => SHELF,
            AppGlyph::Other => OTHER_APP,
        }
        .to_string(),
    }
}

/// The kind's color.
pub fn kind_color(k: Kind) -> Rgba {
    Rgba::hex(match k {
        Kind::Folder => 0x5B9BF8,
        Kind::Image => 0x34C38F,
        Kind::Video => 0xE85D75,
        Kind::Audio => 0xA66CFF,
        Kind::Pdf => 0xEF5350,
        Kind::Document => 0x3B82F6,
        Kind::Spreadsheet => 0x22A55B,
        Kind::Presentation => 0xF59E0B,
        Kind::Archive => 0xB08D5B,
        Kind::Code => 0x14B8A6,
        Kind::Text => 0x8A94A6,
        Kind::Font => 0xEC4899,
        Kind::Model => 0xF97316,
        Kind::Other => 0x8A94A6,
    })
}

/// A 24×24 file-kind icon: a tinted sheet (or folder) with a symbol.
fn kind_svg(k: Kind, color: Rgba, symlink: bool) -> String {
    let c = color.css();
    let link = if symlink {
        r##"<path d="M14.5 19.5h4.5v-4.5M19 19.5l-5.5-5.5" stroke="currentColor" stroke-width="1.6" fill="none" stroke-linecap="round"/>"##
    } else {
        ""
    };
    if k == Kind::Folder {
        return format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M2.5 6.5A2 2 0 0 1 4.5 4.5h4.4a2 2 0 0 1 1.4.6l1.4 1.4h7.8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-15a2 2 0 0 1-2-2z" fill="{c}" fill-opacity="0.9"/><path d="M2.5 9h19" stroke="white" stroke-opacity="0.35" stroke-width="1.2"/>{link}</svg>"##
        );
    }
    let symbol = match k {
        Kind::Image => r##"<circle cx="10" cy="11" r="1.6" fill="white"/><path d="M7 18l3.5-3.5 2 2 3-3.5 1.5 2V18z" fill="white"/>"##,
        Kind::Video => r##"<path d="M10 11v6l5-3z" fill="white"/>"##,
        Kind::Audio => {
            r##"<path d="M11 17.5a1.5 1.5 0 1 1-1.5-1.5H11v-6l4-1v6" stroke="white" stroke-width="1.4" fill="none" stroke-linejoin="round"/>"##
        }
        Kind::Pdf => r##"<path d="M8 17h8M8 14h8M8 11h5" stroke="white" stroke-width="1.4" stroke-linecap="round"/>"##,
        Kind::Document | Kind::Text => r##"<path d="M8 11h8M8 14h8M8 17h5" stroke="white" stroke-width="1.4" stroke-linecap="round"/>"##,
        Kind::Spreadsheet => r##"<path d="M8 11h8v7H8zM8 14.5h8M12 11v7" stroke="white" stroke-width="1.2" fill="none"/>"##,
        Kind::Presentation => r##"<path d="M9 18v-3M12 18v-6M15 18v-4.5" stroke="white" stroke-width="1.6" stroke-linecap="round"/>"##,
        Kind::Archive => {
            r##"<path d="M12 5v2M12 8.5v2M12 12v2" stroke="white" stroke-width="1.6"/><rect x="10.5" y="14.5" width="3" height="3" rx="0.6" fill="white"/>"##
        }
        Kind::Code => {
            r##"<path d="M10 11.5 7.5 14l2.5 2.5M14 11.5l2.5 2.5-2.5 2.5" stroke="white" stroke-width="1.5" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"##
        }
        Kind::Font => {
            r##"<path d="M9 18l3-7 3 7M10.2 15.5h3.6" stroke="white" stroke-width="1.4" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"##
        }
        Kind::Model => {
            r##"<path d="M12 10.5l3.5 2v4L12 18.5l-3.5-2v-4zM8.5 12.5 12 14.5l3.5-2M12 14.5v4" stroke="white" stroke-width="1.1" fill="none" stroke-linejoin="round"/>"##
        }
        _ => "",
    };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 2.5h8.2l4.8 4.8V20a1.5 1.5 0 0 1-1.5 1.5H6A1.5 1.5 0 0 1 4.5 20V4A1.5 1.5 0 0 1 6 2.5z" fill="{c}" fill-opacity="0.9"/><path d="M14.2 2.5v3.3a1.5 1.5 0 0 0 1.5 1.5H19" fill="white" fill-opacity="0.4"/>{symbol}{link}</svg>"##
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Kind(Kind, bool),
    Glyph(GlyphKey),
    Find,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey(u8, u8);

fn glyph_key(g: Glyph) -> GlyphKey {
    match g {
        Glyph::Preview => GlyphKey(0, 0),
        Glyph::Open => GlyphKey(1, 0),
        Glyph::Reveal => GlyphKey(2, 0),
        Glyph::Copy => GlyphKey(3, 0),
        Glyph::CopyFile => GlyphKey(4, 0),
        Glyph::Rename => GlyphKey(5, 0),
        Glyph::Pin => GlyphKey(6, 0),
        Glyph::Trash => GlyphKey(7, 0),
        Glyph::Info => GlyphKey(8, 0),
        Glyph::App(a) => GlyphKey(9, a as u8),
    }
}

#[derive(Default)]
pub struct Icons {
    cache: HashMap<(Key, u32, [u8; 4]), Option<Pixmap>>,
}

fn render_svg(svg: &str, px: u32, color: Rgba) -> Option<Pixmap> {
    let svg = svg.replace("currentColor", &color.css());
    let opt = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_str(&svg, &opt).ok()?;
    let mut pm = Pixmap::new(px.max(1), px.max(1))?;
    let size = tree.size();
    let s = px as f32 / size.width().max(size.height());
    resvg::render(&tree, Transform::from_scale(s, s), &mut pm.as_mut());
    Some(pm)
}

impl Icons {
    fn get(&mut self, key: Key, px: u32, color: Rgba, svg: impl FnOnce() -> String) -> Option<&Pixmap> {
        if self.cache.len() > 400 {
            self.cache.clear();
        }
        self.cache.entry((key, px, [color.r, color.g, color.b, color.a])).or_insert_with(|| render_svg(&svg(), px, color)).as_ref()
    }

    pub fn kind(&mut self, k: Kind, symlink: bool, px: u32, link_color: Rgba) -> Option<&Pixmap> {
        let color = kind_color(k);
        self.get(Key::Kind(k, symlink), px, link_color, || kind_svg(k, color, symlink))
    }

    pub fn glyph(&mut self, g: Glyph, px: u32, color: Rgba) -> Option<&Pixmap> {
        self.get(Key::Glyph(glyph_key(g)), px, color, || action_svg(g))
    }

    pub fn find(&mut self, px: u32, color: Rgba) -> Option<&Pixmap> {
        self.get(Key::Find, px, color, || FIND_GLYPH.to_string())
    }
}

/// The app icon (tray, window, packages): Find's glyph on its accent.
pub fn app_icon_svg() -> String {
    concat!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">"#,
        r#"<rect x="4" y="4" width="56" height="56" rx="14" fill="rgb(20,23,28)"/>"#,
        r#"<rect x="4" y="4" width="56" height="56" rx="14" fill="none" stroke="rgb(34,197,94)" stroke-opacity="0.35" stroke-width="2"/>"#,
        r#"<circle cx="28.5" cy="28.5" r="12.5" fill="none" stroke="rgb(34,197,94)" stroke-width="5"/>"#,
        r#"<path d="M38 38 48 48" stroke="rgb(34,197,94)" stroke-width="5.5" stroke-linecap="round"/></svg>"#
    )
    .to_string()
}

/// RGBA pixels of the app icon at `px` (straight alpha).
pub fn app_icon_rgba(px: u32) -> Vec<u8> {
    let Some(pm) = render_svg(&app_icon_svg(), px, Rgba::hex(0)) else { return vec![0; (px * px * 4) as usize] };
    let mut out = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        let c = p.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    out
}

/// PNG bytes of the app icon (for the tray and the manifest).
pub fn app_icon_png(px: u32) -> Vec<u8> {
    render_svg(&app_icon_svg(), px, Rgba::hex(0)).and_then(|p| p.encode_png().ok()).unwrap_or_default()
}

/// A Windows `.ico` holding PNG images (valid since Windows Vista).
pub fn app_icon_ico(sizes: &[u32]) -> Vec<u8> {
    let pngs: Vec<(u32, Vec<u8>)> = sizes.iter().map(|&s| (s, app_icon_png(s))).collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(pngs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pngs.len() as u32;
    for (s, png) in &pngs {
        let dim = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &pngs {
        out.extend_from_slice(png);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_renders() {
        let mut icons = Icons::default();
        for k in [
            Kind::Folder,
            Kind::Image,
            Kind::Video,
            Kind::Audio,
            Kind::Pdf,
            Kind::Document,
            Kind::Spreadsheet,
            Kind::Presentation,
            Kind::Archive,
            Kind::Text,
            Kind::Code,
            Kind::Font,
            Kind::Model,
            Kind::Other,
        ] {
            let p = icons.kind(k, k == Kind::Folder, 28, Rgba::hex(0xffffff)).expect("kind icon");
            assert!(p.pixels().iter().any(|px| px.alpha() > 0), "{k:?} is empty");
        }
        for g in
            [Glyph::Preview, Glyph::Open, Glyph::Reveal, Glyph::Copy, Glyph::CopyFile, Glyph::Rename, Glyph::Pin, Glyph::Trash, Glyph::Info]
                .into_iter()
                .chain(
                    [
                        AppGlyph::Box,
                        AppGlyph::Look,
                        AppGlyph::Wheel,
                        AppGlyph::Clipboard,
                        AppGlyph::Lens,
                        AppGlyph::Tools,
                        AppGlyph::Shelf,
                        AppGlyph::Other,
                    ]
                    .map(Glyph::App),
                )
        {
            let p = icons.glyph(g, 16, Rgba::hex(0x000000)).expect("glyph");
            assert!(p.pixels().iter().any(|px| px.alpha() > 0), "{g:?} is empty");
        }
        assert!(icons.find(18, Rgba::hex(0x888888)).is_some());
        assert_eq!(app_icon_rgba(32).len(), 32 * 32 * 4);
        assert!(app_icon_png(64).starts_with(&[0x89, b'P', b'N', b'G']));
    }
}
