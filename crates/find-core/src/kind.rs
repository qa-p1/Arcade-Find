//! What kind of thing a file is, for icons and Link content types. The
//! extension table mirrors Arcade Link's `file_kind_for_extension` (v0.2.0).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Folder,
    Image,
    Video,
    Audio,
    Pdf,
    Document,
    Spreadsheet,
    Presentation,
    Archive,
    Text,
    Code,
    Font,
    Model,
    Other,
}

impl Kind {
    pub fn of(name: &str, is_dir: bool) -> Kind {
        if is_dir {
            return Kind::Folder;
        }
        let ext = match name.rfind('.') {
            Some(0) | None => return Kind::Other,
            Some(i) => name[i + 1..].to_ascii_lowercase(),
        };
        Kind::from_link(link_kind_for_extension(&ext))
    }

    pub fn from_link(k: &str) -> Kind {
        match k {
            "image" => Kind::Image,
            "video" => Kind::Video,
            "audio" => Kind::Audio,
            "pdf" => Kind::Pdf,
            "document" => Kind::Document,
            "spreadsheet" => Kind::Spreadsheet,
            "presentation" => Kind::Presentation,
            "archive" => Kind::Archive,
            "text" => Kind::Text,
            "code" => Kind::Code,
            "font" => Kind::Font,
            "model" => Kind::Model,
            _ => Kind::Other,
        }
    }

    /// The Link content type for this entry (`file/pdf`, `folder/reference`).
    pub fn link_type(self) -> &'static str {
        match self {
            Kind::Folder => "folder/reference",
            Kind::Image => "file/image",
            Kind::Video => "file/video",
            Kind::Audio => "file/audio",
            Kind::Pdf => "file/pdf",
            Kind::Document => "file/document",
            Kind::Spreadsheet => "file/spreadsheet",
            Kind::Presentation => "file/presentation",
            Kind::Archive => "file/archive",
            Kind::Text => "file/text",
            Kind::Code => "file/code",
            Kind::Font => "file/font",
            Kind::Model => "file/model",
            Kind::Other => "file/any",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Folder => "Folder",
            Kind::Image => "Image",
            Kind::Video => "Video",
            Kind::Audio => "Audio",
            Kind::Pdf => "PDF",
            Kind::Document => "Document",
            Kind::Spreadsheet => "Spreadsheet",
            Kind::Presentation => "Presentation",
            Kind::Archive => "Archive",
            Kind::Text => "Text",
            Kind::Code => "Code",
            Kind::Font => "Font",
            Kind::Model => "3D model",
            Kind::Other => "File",
        }
    }
}

/// Arcade Link's file kind for a lower-case extension.
pub fn link_kind_for_extension(ext: &str) -> &'static str {
    match ext {
        "png" | "jpg" | "jpeg" | "jpe" | "jfif" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "heic" | "heif" | "avif" | "ico" | "svg"
        | "jxl" | "tga" | "qoi" | "psd" | "raw" | "cr2" | "nef" | "dng" | "arw" | "exr" | "hdr" => "image",
        "mp4" | "mkv" | "mov" | "webm" | "avi" | "m4v" | "wmv" | "flv" | "mpg" | "mpeg" | "3gp" | "ogv" => "video",
        "mp3" | "wav" | "flac" | "ogg" | "oga" | "opus" | "m4a" | "aac" | "wma" | "aiff" | "aif" | "alac" | "mid" | "midi" => "audio",
        "pdf" => "pdf",
        "doc" | "docx" | "odt" | "rtf" | "pages" | "epub" => "document",
        "xls" | "xlsx" | "ods" | "csv" | "tsv" | "numbers" => "spreadsheet",
        "ppt" | "pptx" | "odp" | "key" => "presentation",
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "zst" | "lz" | "lzma" | "cab" | "iso" => "archive",
        "txt" | "md" | "markdown" | "log" | "ini" | "cfg" | "conf" | "nfo" => "text",
        "rs" | "c" | "h" | "cpp" | "hpp" | "cc" | "py" | "js" | "mjs" | "ts" | "tsx" | "jsx" | "java" | "kt" | "go" | "rb" | "php"
        | "swift" | "cs" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "json" | "yaml" | "yml" | "toml" | "xml" | "html" | "htm" | "css"
        | "scss" | "sql" | "lua" | "dart" | "qml" | "vue" | "svelte" => "code",
        "ttf" | "otf" | "woff" | "woff2" | "ttc" => "font",
        "obj" | "stl" | "gltf" | "glb" | "fbx" | "3mf" | "dae" | "ply" => "model",
        _ => "any",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_link_vectors() {
        // Same cases as Arcade Link's spec/vectors/content.json "kinds".
        for (name, kind) in [
            ("a.PNG", "image"),
            ("photo.heic", "image"),
            ("clip.mkv", "video"),
            ("song.flac", "audio"),
            ("paper.pdf", "pdf"),
            ("letter.docx", "document"),
            ("sheet.csv", "spreadsheet"),
            ("deck.pptx", "presentation"),
            ("backup.tar.gz", "archive"),
            ("notes.md", "text"),
            ("main.rs", "code"),
            ("app.ts", "code"),
            ("Inter.woff2", "font"),
            ("teapot.glb", "model"),
            ("README", "any"),
            ("data.bin", "any"),
        ] {
            let k = Kind::of(name, false);
            assert_eq!(k.link_type(), format!("file/{kind}"), "{name}");
        }
        assert_eq!(Kind::of("x", true), Kind::Folder);
        assert_eq!(Kind::of(".bashrc", false), Kind::Other);
    }
}
