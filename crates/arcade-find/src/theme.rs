//! Colours from the Arcade design kit (`arcade_link::design`, Find's lime
//! accent), Find's file-kind content palette, and the system light/dark
//! preference. No other module defines a colour.

use std::sync::atomic::{AtomicU8, Ordering};

use arcade_link::design::{self, Theme as KitTheme};
use find_core::kind::Kind;
use find_core::settings::Theme;

use crate::link::ME;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn hex(v: u32) -> Rgba {
        Rgba { r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8, a: 255 }
    }
    pub const fn alpha(self, a: u8) -> Rgba {
        Rgba { a, ..self }
    }
    pub fn css(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
    pub fn mix(self, other: Rgba, t: f32) -> Rgba {
        let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgba { r: l(self.r, other.r), g: l(self.g, other.g), b: l(self.b, other.b), a: l(self.a, other.a) }
    }
}

/// The overlay's colours: kit tokens under the names the renderer uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub panel: Rgba,
    /// The panel's 1 px border (`line`).
    pub border: Rgba,
    /// Separators between the field, the list and the footer (`line`).
    pub separator: Rgba,
    pub text: Rgba,
    /// Secondary text: paths, meta, placeholders, the footer hint bar.
    pub muted: Rgba,
    /// Non-text marks only (scrollbar thumb, disabled glyphs): below 4.5:1.
    pub faint: Rgba,
    /// Fills and the caret: the accent itself.
    pub accent: Rgba,
    /// Matched characters in names (weight 600): the accent's readable ink.
    pub highlight: Rgba,
    /// The selected row (`accent-soft`).
    pub selection: Rgba,
    /// Other rows of a multi-selection: half of `selection` over the panel.
    pub selection_multi: Rgba,
    /// Selected text in the fields: the accent at 35 %.
    pub text_selection: Rgba,
    /// Key chips, badges and the rename field (`sunken`).
    pub chip: Rgba,
    /// Key chip and badge edges (`line-strong`).
    pub chip_edge: Rgba,
    pub hover: Rgba,
    /// Text on an accent fill (the drag count badge).
    pub accent_text: Rgba,
    /// Readable danger text, and its soft fill.
    pub danger: Rgba,
    pub danger_soft: Rgba,
    /// Readable success text ("Copied").
    pub ok: Rgba,
    /// Readable warning text, and its soft fill.
    pub warn: Rgba,
    pub warn_soft: Rgba,
    /// Symbols drawn on a file-kind fill.
    pub kind_symbol: Rgba,
}

impl Palette {
    pub fn kit(theme: KitTheme) -> Palette {
        let p = design::palette(theme, ME);
        let ink = design::ink(theme, ME);
        let c = Rgba::hex;
        let selection = c(p.accent_soft);
        Palette {
            dark: theme.is_dark(),
            panel: c(p.panel),
            border: c(p.line),
            separator: c(p.line),
            text: c(p.text),
            muted: c(p.muted),
            faint: c(p.faint),
            accent: c(p.accent),
            highlight: c(ink.accent),
            selection,
            selection_multi: c(p.panel).mix(selection, 0.5),
            text_selection: c(p.accent).alpha(89),
            chip: c(p.sunken),
            chip_edge: c(p.line_strong),
            hover: c(p.hover),
            accent_text: c(p.accent_text),
            danger: c(ink.danger),
            danger_soft: c(p.danger_soft),
            ok: c(ink.ok),
            warn: c(ink.warn),
            warn_soft: c(p.warn_soft),
            kind_symbol: if theme.is_dark() { c(p.panel) } else { c(p.on_fill) },
        }
    }

    pub fn dark() -> Palette {
        Palette::kit(KitTheme::Dark)
    }

    pub fn light() -> Palette {
        Palette::kit(KitTheme::Light)
    }

    pub fn for_theme(theme: Theme) -> Palette {
        Palette::kit(kit_theme(theme))
    }

    /// A file kind's icon fill (the content palette below).
    pub fn kind(&self, k: Kind) -> Rgba {
        let (light, dark) = KIND_COLOURS.iter().find(|(kind, _, _)| *kind == k).map_or((0x80899A, 0x8A94A6), |&(_, l, d)| (l, d));
        Rgba::hex(if self.dark { dark } else { light })
    }
}

/// Find's content palette (STANDARD §2): one fill per file kind, light and
/// dark. Every fill is at least 3:1 against the panel, the hover fill and both
/// selection fills in its theme, and the symbol drawn on it (white in light,
/// the panel colour in dark) is at least 3:1 against the fill. The test below
/// checks both.
const KIND_COLOURS: &[(Kind, u32, u32)] = &[
    (Kind::Folder, 0x5189DA, 0x5B9BF8),
    (Kind::Image, 0x2D9A74, 0x34C38F),
    (Kind::Video, 0xE25B72, 0xE85D75),
    (Kind::Audio, 0xA36AFB, 0xA66CFF),
    (Kind::Pdf, 0xED5250, 0xEF5350),
    (Kind::Document, 0x3B82F6, 0x3B82F6),
    (Kind::Spreadsheet, 0x219B57, 0x22A55B),
    (Kind::Presentation, 0xBB7B10, 0xF59E0B),
    (Kind::Archive, 0xA48456, 0xB08D5B),
    (Kind::Code, 0x14988B, 0x14B8A6),
    (Kind::Text, 0x80899A, 0x8A94A6),
    (Kind::Font, 0xEC4899, 0xEC4899),
    (Kind::Model, 0xDB6717, 0xF97316),
    (Kind::Other, 0x80899A, 0x8A94A6),
];

/// The Appearance setting, resolved against the system preference.
pub fn kit_theme(theme: Theme) -> KitTheme {
    match theme {
        Theme::Dark => KitTheme::Dark,
        Theme::Light => KitTheme::Light,
        Theme::System if system_dark() => KitTheme::Dark,
        Theme::System => KitTheme::Light,
    }
}

/// The Appearance setting as the kit's egui `apply` takes it.
pub fn setting_name(theme: Theme) -> &'static str {
    match theme {
        Theme::System => "system",
        Theme::Light => "light",
        Theme::Dark => "dark",
    }
}

/// 0 unknown, 1 dark, 2 light — refreshed off the UI thread.
static SYSTEM: AtomicU8 = AtomicU8::new(0);

pub fn system_dark() -> bool {
    match SYSTEM.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => true, // until detected: dark is the overlay's default look
    }
}

/// Reads the OS preference (blocking: call from a worker thread).
pub fn refresh_system_theme() -> bool {
    let dark = detect_dark();
    let v = match dark {
        Some(true) => 1,
        Some(false) => 2,
        None => 0,
    };
    SYSTEM.swap(v, Ordering::Relaxed) != v
}

#[cfg(target_os = "linux")]
fn detect_dark() -> Option<bool> {
    // The XDG Settings portal: org.freedesktop.appearance color-scheme (1 dark, 2 light).
    if let Some(v) = portal_color_scheme() {
        if v == 1 {
            return Some(true);
        }
        if v == 2 {
            return Some(false);
        }
    }
    if let Ok(t) = std::env::var("GTK_THEME") {
        return Some(t.to_lowercase().contains("dark"));
    }
    None
}

#[cfg(target_os = "linux")]
pub fn portal_color_scheme() -> Option<u32> {
    let conn = zbus::blocking::Connection::session().ok()?;
    let reply = conn
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.Settings"),
            "ReadOne",
            &("org.freedesktop.appearance", "color-scheme"),
        )
        .ok()?;
    let v: zbus::zvariant::OwnedValue = reply.body().deserialize().ok()?;
    u32_from_value(&v)
}

#[cfg(target_os = "linux")]
fn u32_from_value(v: &zbus::zvariant::Value<'_>) -> Option<u32> {
    match v {
        zbus::zvariant::Value::U32(x) => Some(*x),
        zbus::zvariant::Value::Value(inner) => u32_from_value(inner),
        _ => None,
    }
}

/// Calls `f` whenever the portal reports a color-scheme change (a blocking
/// signal stream on its own thread; no polling).
#[cfg(target_os = "linux")]
pub fn watch_system_theme(f: impl Fn() + Send + 'static) {
    let _ = std::thread::Builder::new().name("find-theme".into()).spawn(move || {
        let Ok(conn) = zbus::blocking::Connection::session() else { return };
        let Ok(proxy) =
            zbus::blocking::Proxy::new(&conn, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.Settings")
        else {
            return;
        };
        let Ok(signals) = proxy.receive_signal("SettingChanged") else { return };
        for msg in signals {
            let body = msg.body();
            if let Ok((ns, key, _v)) = body.deserialize::<(String, String, zbus::zvariant::OwnedValue)>() {
                if ns == "org.freedesktop.appearance" && key == "color-scheme" && refresh_system_theme() {
                    f();
                }
            }
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn watch_system_theme(_f: impl Fn() + Send + 'static) {}

#[cfg(windows)]
fn detect_dark() -> Option<bool> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0".encode_utf16().collect();
    let name: Vec<u16> = "AppsUseLightTheme\0".encode_utf16().collect();
    let mut data: u32 = 0;
    let mut len: u32 = 4;
    // SAFETY: valid NUL-terminated strings and out buffers.
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut data as *mut u32 as *mut _, &mut len)
    };
    (r == 0).then_some(data == 0)
}

#[cfg(target_os = "macos")]
fn detect_dark() -> Option<bool> {
    let out = std::process::Command::new("/usr/bin/defaults").args(["read", "-g", "AppleInterfaceStyle"]).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().eq_ignore_ascii_case("dark"))
}

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
fn detect_dark() -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: Rgba) -> f32 {
        let f = |v: u8| {
            let s = v as f32 / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
    }

    fn ratio(a: Rgba, b: Rgba) -> f32 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn find_uses_the_kit_lime_accent() {
        assert_eq!(Palette::light().accent, Rgba::hex(0x517E0E));
        assert_eq!(Palette::dark().accent, Rgba::hex(0x84CC16));
        assert_eq!(Palette::dark().selection, Rgba::hex(design::palette(KitTheme::Dark, "arcade.find").accent_soft));
    }

    #[test]
    fn text_is_readable_on_every_overlay_surface() {
        for p in [Palette::dark(), Palette::light()] {
            for (name, bg) in [("panel", p.panel), ("selection", p.selection), ("multi", p.selection_multi), ("hover", p.hover), ("chip", p.chip)] {
                assert!(ratio(p.text, bg) >= 7.0, "text on {name}, dark {}", p.dark);
                assert!(ratio(p.muted, bg) >= 4.5, "muted on {name}, dark {}", p.dark);
                assert!(ratio(p.highlight, bg) >= 4.5, "highlight on {name}, dark {}", p.dark);
            }
            assert!(ratio(p.danger, p.panel) >= 4.5 && ratio(p.danger, p.danger_soft) >= 4.5);
            assert!(ratio(p.warn, p.panel) >= 4.5 && ratio(p.warn, p.warn_soft) >= 4.5);
            assert!(ratio(p.ok, p.panel) >= 4.5);
            assert!(ratio(p.accent_text, p.accent) >= 4.5);
            assert!(ratio(p.accent, p.panel) >= 3.0, "the caret and selection bar");
        }
    }

    #[test]
    fn kind_icons_meet_ui_contrast() {
        for p in [Palette::dark(), Palette::light()] {
            for &(kind, _, _) in KIND_COLOURS {
                let fill = p.kind(kind);
                for (name, bg) in [("panel", p.panel), ("selection", p.selection), ("multi", p.selection_multi), ("hover", p.hover)] {
                    assert!(ratio(fill, bg) >= 3.0, "{kind:?} on {name}, dark {}: {:.2}", p.dark, ratio(fill, bg));
                }
                assert!(ratio(p.kind_symbol, fill) >= 3.0, "{kind:?} symbol, dark {}: {:.2}", p.dark, ratio(p.kind_symbol, fill));
            }
        }
    }
}
