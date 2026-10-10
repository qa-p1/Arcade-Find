//! Maps window-system key events to the model's [`KeyEvent`].

use super::model::{Key, KeyEvent, Mods};

/// Text a key produced, without control characters.
pub fn printable(text: Option<&str>) -> Option<String> {
    text.filter(|t| !t.is_empty() && !t.chars().any(char::is_control)).map(str::to_string)
}

/// From an XKB keysym (Wayland).
#[cfg(target_os = "linux")]
pub fn from_keysym(sym: smithay_client_toolkit::seat::keyboard::Keysym, utf8: Option<&str>, mods: Mods) -> KeyEvent {
    use smithay_client_toolkit::seat::keyboard::Keysym as K;
    let key = match sym {
        K::Return | K::KP_Enter | K::ISO_Enter => Key::Enter,
        K::Escape => Key::Escape,
        K::Tab | K::ISO_Left_Tab => Key::Tab,
        K::BackSpace => Key::Backspace,
        K::Delete | K::KP_Delete => Key::Delete,
        K::Up | K::KP_Up => Key::Up,
        K::Down | K::KP_Down => Key::Down,
        K::Left | K::KP_Left => Key::Left,
        K::Right | K::KP_Right => Key::Right,
        K::Home | K::KP_Home => Key::Home,
        K::End | K::KP_End => Key::End,
        K::Page_Up | K::KP_Page_Up => Key::PageUp,
        K::Page_Down | K::KP_Page_Down => Key::PageDown,
        K::space | K::KP_Space => Key::Space,
        K::F2 => Key::F2,
        K::F5 => Key::F5,
        other => match other.key_char() {
            Some(c) if !c.is_control() => Key::Char(c),
            _ => Key::Other,
        },
    };
    let text = if key == Key::Space { Some(" ".to_string()) } else { printable(utf8) };
    KeyEvent { key, text, mods }
}

/// From a winit key event.
pub fn from_winit(logical: &winit::keyboard::Key, text: Option<&str>, mods: Mods) -> KeyEvent {
    use winit::keyboard::{Key as WK, NamedKey as N};
    let key = match logical {
        WK::Named(n) => match n {
            N::Enter => Key::Enter,
            N::Escape => Key::Escape,
            N::Tab => Key::Tab,
            N::Backspace => Key::Backspace,
            N::Delete => Key::Delete,
            N::ArrowUp => Key::Up,
            N::ArrowDown => Key::Down,
            N::ArrowLeft => Key::Left,
            N::ArrowRight => Key::Right,
            N::Home => Key::Home,
            N::End => Key::End,
            N::PageUp => Key::PageUp,
            N::PageDown => Key::PageDown,
            N::Space => Key::Space,
            N::F2 => Key::F2,
            N::F5 => Key::F5,
            _ => Key::Other,
        },
        WK::Character(s) => match s.chars().next() {
            Some(c) if s.chars().count() == 1 && !c.is_control() => Key::Char(c),
            _ => Key::Other,
        },
        _ => Key::Other,
    };
    let text = if key == Key::Space { Some(" ".to_string()) } else { printable(text) };
    KeyEvent { key, text, mods }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_drops_control_text() {
        assert_eq!(printable(Some("a")).as_deref(), Some("a"));
        assert_eq!(printable(Some("\u{3}")), None);
        assert_eq!(printable(Some("")), None);
        assert_eq!(printable(None), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn keysyms() {
        let ctrl = Mods { ctrl: true, ..Mods::default() };
        let e = from_keysym(smithay_client_toolkit::seat::keyboard::Keysym::c, Some("\u{3}"), ctrl);
        assert_eq!(e.key, Key::Char('c'));
        assert_eq!(e.text, None, "control characters aren't text");
        let e = from_keysym(smithay_client_toolkit::seat::keyboard::Keysym::adiaeresis, Some("ä"), Mods::default());
        assert_eq!(e.key, Key::Char('ä'));
        assert_eq!(e.text.as_deref(), Some("ä"));
        assert_eq!(from_keysym(smithay_client_toolkit::seat::keyboard::Keysym::KP_Enter, None, Mods::default()).key, Key::Enter);
        assert_eq!(
            from_keysym(smithay_client_toolkit::seat::keyboard::Keysym::space, Some(" "), Mods::default()).text.as_deref(),
            Some(" ")
        );
    }
}
