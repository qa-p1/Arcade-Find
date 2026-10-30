//! Raster icons: file kinds (Find's content palette in `theme.rs`) and the
//! Arcade apps' glyphs and launcher icon (synced from Arcade Link
//! `design/` by `scripts/sync-design`). Rendered once per size and colour
//! with resvg and cached. Action glyphs are the kit's outline icons, drawn
//! as paths by the renderer ([`kit_name`]).

use std::collections::HashMap;

use find_core::kind::Kind;
use resvg::tiny_skia::{Pixmap, Transform};

use super::model::{AppGlyph, Glyph};
use crate::theme::{Palette, Rgba};

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

/// The kit icon an action glyph uses (app glyphs are SVGs, see [`Icons::glyph`]).
pub fn kit_name(g: Glyph) -> &'static str {
    match g {
        Glyph::Preview => "eye",
        Glyph::Open => "external",
        Glyph::Reveal => "showInFolder",
        Glyph::Copy => "copy",
        Glyph::CopyFile => "file",
        Glyph::Rename => "edit",
        Glyph::Pin => "pin",
        Glyph::Trash => "trash",
        Glyph::Info => "info",
        Glyph::App(_) => "apps",
    }
}

fn app_svg(a: AppGlyph) -> &'static str {
    match a {
        AppGlyph::Box => BOX,
        AppGlyph::Look => LOOK,
        AppGlyph::Wheel => WHEEL,
        AppGlyph::Clipboard => CLIPBOARD,
        AppGlyph::Lens => LENS,
        AppGlyph::Tools => TOOLS,
        AppGlyph::Shelf => SHELF,
        AppGlyph::Other => OTHER_APP,
    }
}

/// A 24×24 file-kind icon: a tinted sheet (or folder) with a symbol.
fn kind_svg(k: Kind, fill: Rgba, symbol: Rgba, symlink: bool) -> String {
    let c = fill.css();
    let white = symbol.css();
    let link = if symlink {
        r##"<path d="M14.5 19.5h4.5v-4.5M19 19.5l-5.5-5.5" stroke="currentColor" stroke-width="1.6" fill="none" stroke-linecap="round"/>"##
    } else {
        ""
    };
    if k == Kind::Folder {
        return format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M2.5 6.5A2 2 0 0 1 4.5 4.5h4.4a2 2 0 0 1 1.4.6l1.4 1.4h7.8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-15a2 2 0 0 1-2-2z" fill="{c}"/><path d="M2.5 9h19" stroke="{white}" stroke-opacity="0.35" stroke-width="1.2"/>{link}</svg>"##
        );
    }
    let symbol = match k {
        Kind::Image => format!(r##"<circle cx="10" cy="11" r="1.6" fill="{white}"/><path d="M7 18l3.5-3.5 2 2 3-3.5 1.5 2V18z" fill="{white}"/>"##),
        Kind::Video => format!(r##"<path d="M10 11v6l5-3z" fill="{white}"/>"##),
        Kind::Audio => {
            format!(r##"<path d="M11 17.5a1.5 1.5 0 1 1-1.5-1.5H11v-6l4-1v6" stroke="{white}" stroke-width="1.4" fill="none" stroke-linejoin="round"/>"##)
        }
        Kind::Pdf => format!(r##"<path d="M8 17h8M8 14h8M8 11h5" stroke="{white}" stroke-width="1.4" stroke-linecap="round"/>"##),
        Kind::Document | Kind::Text => format!(r##"<path d="M8 11h8M8 14h8M8 17h5" stroke="{white}" stroke-width="1.4" stroke-linecap="round"/>"##),
        Kind::Spreadsheet => format!(r##"<path d="M8 11h8v7H8zM8 14.5h8M12 11v7" stroke="{white}" stroke-width="1.2" fill="none"/>"##),
        Kind::Presentation => format!(r##"<path d="M9 18v-3M12 18v-6M15 18v-4.5" stroke="{white}" stroke-width="1.6" stroke-linecap="round"/>"##),
        Kind::Archive => {
            format!(
                r##"<path d="M12 5v2M12 8.5v2M12 12v2" stroke="{white}" stroke-width="1.6"/><rect x="10.5" y="14.5" width="3" height="3" rx="0.6" fill="{white}"/>"##
            )
        }
        Kind::Code => {
            format!(
                r##"<path d="M10 11.5 7.5 14l2.5 2.5M14 11.5l2.5 2.5-2.5 2.5" stroke="{white}" stroke-width="1.5" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"##
            )
        }
        Kind::Font => {
            format!(
                r##"<path d="M9 18l3-7 3 7M10.2 15.5h3.6" stroke="{white}" stroke-width="1.4" fill="none" stroke-linecap="round" stroke-linejoin="round"/>"##
            )
        }
        Kind::Model => {
            format!(
                r##"<path d="M12 10.5l3.5 2v4L12 18.5l-3.5-2v-4zM8.5 12.5 12 14.5l3.5-2M12 14.5v4" stroke="{white}" stroke-width="1.1" fill="none" stroke-linejoin="round"/>"##
            )
        }
        _ => String::new(),
    };
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 2.5h8.2l4.8 4.8V20a1.5 1.5 0 0 1-1.5 1.5H6A1.5 1.5 0 0 1 4.5 20V4A1.5 1.5 0 0 1 6 2.5z" fill="{c}"/><path d="M14.2 2.5v3.3a1.5 1.5 0 0 0 1.5 1.5H19" fill="{white}" fill-opacity="0.4"/>{symbol}{link}</svg>"##
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Kind(Kind, bool, bool),
    Glyph(GlyphKey),
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

    /// A file-kind icon in `p`'s content palette (the symlink arrow in the
    /// symbol colour).
    pub fn kind(&mut self, k: Kind, symlink: bool, px: u32, p: &Palette) -> Option<&Pixmap> {
        let (fill, symbol) = (p.kind(k), p.kind_symbol);
        self.get(Key::Kind(k, symlink, p.dark), px, symbol, || kind_svg(k, fill, symbol, symlink))
    }

    /// An Arcade app's glyph (other glyphs are kit icons drawn by the renderer).
    pub fn glyph(&mut self, g: Glyph, px: u32, color: Rgba) -> Option<&Pixmap> {
        let svg = match g {
            Glyph::App(a) => app_svg(a),
            _ => OTHER_APP,
        };
        self.get(Key::Glyph(glyph_key(g)), px, color, || svg.to_string())
    }
}

/// The launcher icon from the kit (`design/app-icons/find/`): lime tile,
/// white glyph. Windows, packages and the manifest use it.
pub fn app_icon_svg() -> String {
    include_str!("../../../../assets/app-icon/icon.svg").to_string()
}

/// RGBA pixels of the launcher icon at `px` (straight alpha).
pub fn app_icon_rgba(px: u32) -> Vec<u8> {
    straight(render_svg(&app_icon_svg(), px, Palette::light().text), px)
}

/// The tray icon: Find's glyph, monochrome (STANDARD §8), white for dark
/// panels or black for light ones and macOS template images.
pub fn tray_icon_rgba(px: u32, white: bool) -> Vec<u8> {
    let colour =
        if white { Rgba::hex(arcade_link::design::palette(arcade_link::design::Theme::Light, crate::link::ME).on_fill) } else { Palette::light().text };
    straight(render_svg(FIND_GLYPH, px, colour), px)
}

/// An Arcade app's glyph as a white mask (straight RGBA), for kit glyph tiles.
pub fn glyph_mask_rgba(app_id: &str, px: u32) -> Vec<u8> {
    let svg = if app_id == crate::link::ME { FIND_GLYPH } else { app_svg(AppGlyph::of(app_id)) };
    let mask = arcade_link::design::palette(arcade_link::design::Theme::Light, crate::link::ME).on_fill;
    straight(render_svg(svg, px, Rgba::hex(mask)), px)
}

fn straight(pm: Option<Pixmap>, px: u32) -> Vec<u8> {
    let Some(pm) = pm else { return vec![0; (px * px * 4) as usize] };
    let mut out = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        let c = p.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    out
}

/// PNG bytes of the launcher icon: the kit's own export at its sizes.
pub fn app_icon_png(px: u32) -> Vec<u8> {
    let kit: &[u8] = match px {
        16 => include_bytes!("../../../../assets/app-icon/png/16.png"),
        24 => include_bytes!("../../../../assets/app-icon/png/24.png"),
        32 => include_bytes!("../../../../assets/app-icon/png/32.png"),
        48 => include_bytes!("../../../../assets/app-icon/png/48.png"),
        64 => include_bytes!("../../../../assets/app-icon/png/64.png"),
        128 => include_bytes!("../../../../assets/app-icon/png/128.png"),
        256 => include_bytes!("../../../../assets/app-icon/png/256.png"),
        512 => include_bytes!("../../../../assets/app-icon/png/512.png"),
        1024 => include_bytes!("../../../../assets/app-icon/png/1024.png"),
        _ => &[],
    };
    if !kit.is_empty() {
        return kit.to_vec();
    }
    render_svg(&app_icon_svg(), px, Palette::light().text).and_then(|p| p.encode_png().ok()).unwrap_or_default()
}

/// The kit's Windows icon (16–256 px).
pub fn app_icon_ico() -> &'static [u8] {
    include_bytes!("../../../../assets/app-icon/arcade-find.ico")
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
            let p = icons.kind(k, k == Kind::Folder, 28, &Palette::light()).expect("kind icon");
            assert!(p.pixels().iter().any(|px| px.alpha() > 0), "{k:?} is empty");
        }
        for g in
            [Glyph::Preview, Glyph::Open, Glyph::Reveal, Glyph::Copy, Glyph::CopyFile, Glyph::Rename, Glyph::Pin, Glyph::Trash, Glyph::Info].into_iter().chain(
                [AppGlyph::Box, AppGlyph::Look, AppGlyph::Wheel, AppGlyph::Clipboard, AppGlyph::Lens, AppGlyph::Tools, AppGlyph::Shelf, AppGlyph::Other]
                    .map(Glyph::App),
            )
        {
            let p = icons.glyph(g, 16, Palette::light().text).expect("glyph");
            assert!(p.pixels().iter().any(|px| px.alpha() > 0), "{g:?} is empty");
        }
        assert_eq!(app_icon_rgba(32).len(), 32 * 32 * 4);
        assert!(app_icon_rgba(32).chunks(4).any(|p| p[3] == 255), "the launcher icon renders");
        for px in [16, 22, 64, 256] {
            assert!(app_icon_png(px).starts_with(&[0x89, b'P', b'N', b'G']), "{px}");
        }
        assert_eq!(&app_icon_ico()[..4], &[0, 0, 1, 0]);
        let white = tray_icon_rgba(32, true);
        assert!(white.chunks(4).filter(|p| p[3] > 0).all(|p| p[..3] == [255, 255, 255]), "the tray glyph is monochrome");
    }
}
