//! Dragging results out of the overlay: what a drag carries and the gesture
//! that starts one. The window backends own the platform protocols.

use std::path::{Path, PathBuf};

/// Files and folders, as file managers and Arcade Shelf read them.
pub const URI_LIST: &str = "text/uri-list";
/// The paths themselves, for text fields and terminals.
pub const TEXT: &str = "text/plain;charset=utf-8";

/// How far (logical pixels) the pointer moves with the button held before a
/// press becomes a drag.
pub const THRESHOLD: f64 = 6.0;

/// A `file://` URI for an absolute path (RFC 8089), percent-encoding every
/// byte outside the unreserved set and `/`.
pub fn file_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    #[cfg(unix)]
    let bytes: Vec<u8> = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(unix))]
    let bytes: Vec<u8> = {
        // C:\a b -> /C:/a%20b
        let s = path.to_string_lossy().replace('\\', "/");
        if s.starts_with('/') {
            s.into_bytes()
        } else {
            format!("/{s}").into_bytes()
        }
    };
    for b in bytes {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => out.push(b as char),
            #[cfg(not(unix))]
            b':' => out.push(':'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `text/uri-list`: one URI per line, CRLF-terminated.
pub fn uri_list(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| file_uri(p) + "\r\n").collect()
}

/// Plain text: one path per line.
pub fn plain_text(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>().join("\n")
}

/// The bytes to send for a requested MIME type (`None`: not offered).
pub fn payload(paths: &[PathBuf], mime: &str) -> Option<Vec<u8>> {
    match mime {
        URI_LIST => Some(uri_list(paths).into_bytes()),
        TEXT | "text/plain" | "UTF8_STRING" | "STRING" | "TEXT" => Some(plain_text(paths).into_bytes()),
        _ => None,
    }
}

/// A press on a result row that may turn into a drag.
#[derive(Debug, Clone)]
pub struct Press {
    pub row: usize,
    pub at: (f64, f64),
    /// The click is applied on release (pressing inside a multi-selection
    /// keeps it, so the whole selection can be dragged).
    pub deferred_click: bool,
}

impl Press {
    /// Whether the pointer has moved far enough to start a drag.
    pub fn moved_past(&self, to: (f64, f64)) -> bool {
        let (dx, dy) = (to.0 - self.at.0, to.1 - self.at.1);
        dx * dx + dy * dy >= THRESHOLD * THRESHOLD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn uris_are_percent_encoded() {
        assert_eq!(file_uri(Path::new("/home/u/a b/ré#1.txt")), "file:///home/u/a%20b/r%C3%A9%231.txt");
        assert_eq!(uri_list(&[PathBuf::from("/a"), PathBuf::from("/b c")]), "file:///a\r\nfile:///b%20c\r\n");
    }

    #[cfg(windows)]
    #[test]
    fn windows_uris_keep_the_drive() {
        assert_eq!(file_uri(Path::new(r"C:\Users\u\a b.txt")), "file:///C:/Users/u/a%20b.txt");
    }

    #[test]
    fn payloads_by_mime() {
        let p = vec![PathBuf::from("/x/one"), PathBuf::from("/x/two")];
        assert_eq!(payload(&p, TEXT).unwrap(), b"/x/one\n/x/two");
        assert!(payload(&p, URI_LIST).unwrap().ends_with(b"\r\n"));
        assert!(payload(&p, "image/png").is_none());
    }

    #[test]
    fn drag_threshold() {
        let p = Press { row: 0, at: (10.0, 10.0), deferred_click: false };
        assert!(!p.moved_past((13.0, 13.0)));
        assert!(p.moved_past((10.0, 16.5)));
    }
}
