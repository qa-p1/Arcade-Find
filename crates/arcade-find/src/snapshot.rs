//! `--snapshot DIR` (hidden): renders overlay states to PNG files, for
//! reviewing the design and in tests. No window, index or Link involved.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use find_core::kind::Kind;
use find_core::settings::Theme;

use crate::theme::Palette;
use crate::ui::model::{ActionId, ActionItem, AppGlyph, Glyph, Key, KeyEvent, Mode, Mods, Overlay, ResultsInfo, Row, ToastKind};
use crate::ui::render::Renderer;

struct NoLook;
impl crate::ui::model::Context for NoLook {
    fn can_preview(&self, _: &[Row]) -> bool {
        true
    }
}

fn row(path: &str, size: Option<u64>, age_days: i64, hl: &[(usize, usize)]) -> Row {
    let p = PathBuf::from(path);
    let name = p.file_name().unwrap().to_string_lossy().into_owned();
    let is_dir = size.is_none();
    Row {
        parent: find_core::paths::tilde(p.parent().unwrap()),
        kind: Kind::of(&name, is_dir),
        highlights: hl.to_vec(),
        name,
        path: p,
        size,
        mtime: find_core::now_secs() - age_days * 86_400 - 3_600,
        is_dir,
        is_symlink: false,
        pinned: false,
        section: None,
        missing: false,
    }
}

fn sample_rows() -> Vec<Row> {
    let h = find_core::paths::home_dir();
    let h = h.to_string_lossy();
    vec![
        row(&format!("{h}/Documents/Reports/report-2024-q3.pdf"), Some(2_480_000), 0, &[(0, 6)]),
        row(&format!("{h}/Projects/site/src/report.rs"), Some(14_200), 1, &[(0, 6)]),
        row(&format!("{h}/Documents/Reports"), None, 3, &[(0, 6)]),
        row(&format!("{h}/Pictures/2024/Summer/report-cover.png"), Some(5_600_000), 40, &[(0, 6)]),
        row(&format!("{h}/Downloads/quarterly report draft.docx"), Some(88_000), 400, &[(10, 16)]),
        row(&format!("{h}/Music/Reporter - Live at the Hall.flac"), Some(31_000_000), 9, &[(0, 6)]),
        row(&format!("{h}/Videos/report walkthrough.mp4"), Some(240_000_000), 2, &[(0, 6)]),
        row(&format!("{h}/Projects/archive/reports-2019.tar.gz"), Some(9_100_000), 1200, &[(0, 6)]),
        row(&format!("{h}/Projects/site/reporting"), None, 5, &[(0, 6)]),
    ]
}

fn typed(o: &mut Overlay, s: &str) {
    for c in s.chars() {
        let ev = if c == ' ' { KeyEvent::new(Key::Space) } else { KeyEvent::ch(c) };
        let _ = o.key(&ev, &NoLook);
    }
}

fn results(o: &mut Overlay, rows: Vec<Row>) {
    let n = rows.len();
    let info = ResultsInfo { matched: n * 37, elapsed_ms: 3.1, ..Default::default() };
    o.set_results(o.seq, rows, info);
}

fn save(r: &mut Renderer, o: &Overlay, theme: Theme, scale: f32, dir: &Path, name: &str) -> Result<(), String> {
    let p = Palette::for_theme(theme, true);
    let pm = r.render(o, &p, scale, find_core::now_secs()).ok_or("render failed")?;
    pm.save_png(dir.join(format!("{name}.png"))).map_err(|e| e.to_string())
}

pub fn run(dir: PathBuf) -> ExitCode {
    match render_all(&dir) {
        Ok(n) => {
            println!("{n} snapshots in {}", dir.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("arcade-find: snapshot: {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn render_all(dir: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut r = Renderer::new();
    let mut n = 0;
    for (theme, suffix) in [(Theme::Dark, "dark"), (Theme::Light, "light")] {
        let mut o = Overlay::default();
        o.show(None, None);
        save(&mut r, &o, theme, 1.0, dir, &format!("01-rest-{suffix}"))?;

        typed(&mut o, "report");
        results(&mut o, sample_rows()[..4].to_vec());
        save(&mut r, &o, theme, 1.0, dir, &format!("02-results-{suffix}"))?;

        results(&mut o, sample_rows());
        for _ in 0..2 {
            o.key(&KeyEvent::new(Key::Down), &NoLook);
        }
        save(&mut r, &o, theme, 1.0, dir, &format!("03-scrolling-{suffix}"))?;

        let shift = Mods { shift: true, ..Mods::default() };
        o.key(&KeyEvent::new(Key::Down).with(shift), &NoLook);
        o.key(&KeyEvent::new(Key::Down).with(shift), &NoLook);
        let targets = o.targets();
        let items = vec![
            ActionItem::new(ActionId::Preview, "Quick Look", Glyph::Preview).hint("⏎"),
            ActionItem::new(ActionId::Open, "Open all", Glyph::Open).hint("⇧⏎"),
            ActionItem::new(ActionId::CopyPath, "Copy 3 paths", Glyph::Copy).hint("Ctrl C"),
            ActionItem {
                detail: Some("report-2024-q3.pdf, report.rs +1".into()),
                outbound: true,
                ..ActionItem::new(ActionId::CopyFile, "Send to my devices", Glyph::App(AppGlyph::Clipboard))
            },
            ActionItem::new(ActionId::CopyFile, "Compress", Glyph::App(AppGlyph::Box)),
            ActionItem {
                disabled: true,
                detail: Some("Too large for Arcade Box (limit 16 MB).".into()),
                ..ActionItem::new(ActionId::CopyFile, "Convert to PDF", Glyph::App(AppGlyph::Box))
            },
            ActionItem { danger: true, ..ActionItem::new(ActionId::Trash, "Move to Trash", Glyph::Trash).hint("Del") },
        ];
        o.set_actions(targets, items);
        save(&mut r, &o, theme, 1.0, dir, &format!("04-actions-{suffix}"))?;

        o.mode = Mode::Results;
        o.anchor = None;
        o.key(&KeyEvent::new(Key::F2), &NoLook);
        save(&mut r, &o, theme, 1.0, dir, &format!("05-rename-{suffix}"))?;
        o.mode = Mode::Results;

        if let Some(row) = o.rows.get(o.sel).cloned() {
            o.mode = Mode::Details { row };
            save(&mut r, &o, theme, 1.0, dir, &format!("06-details-{suffix}"))?;
            o.mode = Mode::Results;
        }

        o.toast("Arcade Look isn't running.", ToastKind::Error);
        save(&mut r, &o, theme, 1.0, dir, &format!("07-error-{suffix}"))?;
        o.toast = None;

        let mut empty = Overlay::default();
        empty.show(Some("zzqx"), None);
        empty.set_results(empty.seq, vec![], ResultsInfo::default());
        save(&mut r, &empty, theme, 1.0, dir, &format!("08-no-matches-{suffix}"))?;

        save(&mut r, &o, theme, 1.5, dir, &format!("09-results-1.5x-{suffix}"))?;
        n += 9;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders_every_state() {
        let dir = std::env::temp_dir().join(format!("af-snap-{}", std::process::id()));
        assert_eq!(super::render_all(&dir).unwrap(), 18);
        assert!(dir.join("04-actions-dark.png").is_file());
        std::fs::remove_dir_all(dir).ok();
    }
}
