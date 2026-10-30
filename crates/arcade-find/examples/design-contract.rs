//! Emit the renderer's actual mapped colours for the design contrast check.
use arcade_find::theme::{Palette, Rgba};
use find_core::kind::Kind;
use serde_json::json;
fn rgba(c: Rgba) -> [u8; 4] {
    [c.r, c.g, c.b, c.a]
}
fn main() {
    let themes: Vec<_> = [Palette::light(), Palette::dark()]
        .into_iter()
        .map(|p| {
            json!({
                "theme": if p.dark {"dark"} else {"light"}, "panel":rgba(p.panel),
                "selection":rgba(p.selection),"selection_multi":rgba(p.selection_multi),"text_selection":rgba(p.text_selection),
                "hover":rgba(p.hover),"chip":rgba(p.chip),"text":rgba(p.text),"muted":rgba(p.muted),"highlight":rgba(p.highlight),
                "danger":rgba(p.danger),"accent":rgba(p.accent),"accent_text":rgba(p.accent_text),"ok":rgba(p.ok),"warn":rgba(p.warn),"danger_soft":rgba(p.danger_soft),"warn_soft":rgba(p.warn_soft),
                "kind_symbol":rgba(p.kind_symbol),
                "kinds":([Kind::Folder,Kind::Image,Kind::Video,Kind::Audio,Kind::Pdf,Kind::Document,Kind::Spreadsheet,Kind::Presentation,
                    Kind::Archive,Kind::Code,Kind::Text,Kind::Font,Kind::Model,Kind::Other].map(|kind|json!({"kind":kind.label(),"fill":rgba(p.kind(kind))})))
            })
        })
        .collect();
    println!("{}", json!(themes));
}
