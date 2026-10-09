//! Synthetic benchmark: builds an index of N entries (default 1,000,000)
//! shaped like a home folder, then measures memory, search latency for
//! typical queries, and save/load time.
//!
//! cargo bench -p find-core --bench synthetic -- [entries]

use std::path::Path;
use std::time::Instant;

use find_core::index::{flag, key_child, key_root, Index, Meta};
use find_core::query::Query;
use find_core::search::{search, SearchOptions};

const WORDS: &[&str] = &[
    "report",
    "invoice",
    "photo",
    "img",
    "screenshot",
    "notes",
    "draft",
    "final",
    "project",
    "budget",
    "meeting",
    "summary",
    "resume",
    "letter",
    "scan",
    "backup",
    "config",
    "main",
    "index",
    "test",
    "utils",
    "readme",
    "data",
    "export",
    "video",
    "track",
    "song",
    "album",
    "chapter",
    "thesis",
    "paper",
    "design",
    "logo",
    "icon",
    "banner",
    "slides",
    "deck",
    "plan",
    "todo",
    "journal",
];
const EXTS: &[&str] =
    &["pdf", "jpg", "png", "txt", "md", "rs", "py", "js", "json", "docx", "xlsx", "mp4", "mp3", "zip", "html", "css", "ts", "go", "c", "h"];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a>(&mut self, v: &[&'a str]) -> &'a str {
        v[(self.next() % v.len() as u64) as usize]
    }
}

fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| l.split_whitespace().nth(1)?.parse().ok()))
        .unwrap_or(0)
}

fn main() {
    let n: usize = std::env::args().skip(1).find_map(|a| a.parse().ok()).unwrap_or(1_000_000);
    let rss0 = rss_kib();
    let t0 = Instant::now();
    let mut ix = Index::new();
    let root = Path::new("/home/user");
    let rk = key_root(root);
    let r = ix.add_root(root, Meta::default());
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    // ~10% folders, nested up to depth 8.
    let mut dirs: Vec<(u32, u64)> = vec![(r, rk)];
    let mut count = 1;
    while count < n {
        let (parent, pk) = dirs[(rng.next() % dirs.len() as u64) as usize];
        if rng.next().is_multiple_of(10) && ix.depth(parent) < 8 {
            let name = format!("{}-{}", rng.pick(WORDS), rng.next() % 1000);
            let k = key_child(pk, name.as_bytes());
            let id = ix.push(
                parent,
                name.as_bytes(),
                Meta { flags: flag::DIR | if rng.next().is_multiple_of(40) { flag::HIDDEN } else { 0 }, mtime: 1_700_000_000, size: 0 },
                k,
            );
            dirs.push((id, k));
        } else {
            let name = match rng.next() % 4 {
                0 => format!("{}_{}.{}", rng.pick(WORDS), rng.next() % 10_000, rng.pick(EXTS)),
                1 => format!("{} {} {}.{}", rng.pick(WORDS), rng.pick(WORDS), 2000 + rng.next() % 27, rng.pick(EXTS)),
                2 => format!("IMG_{:08}.{}", rng.next() % 100_000_000, rng.pick(EXTS)),
                _ => format!("{}{}.{}", rng.pick(WORDS), rng.pick(WORDS), rng.pick(EXTS)),
            };
            ix.push(
                parent,
                name.as_bytes(),
                Meta { flags: 0, mtime: 1_600_000_000 + (rng.next() % 200_000_000) as i64, size: rng.next() % (512 << 20) },
                0,
            );
        }
        count += 1;
    }
    let build = t0.elapsed();
    let rss1 = rss_kib();
    println!("entries: {} ({} folders)", ix.live(), dirs.len());
    println!("build: {:.0} ms", build.as_secs_f64() * 1000.0);
    println!("index heap estimate: {:.1} MiB", ix.heap_bytes() as f64 / 1048576.0);
    println!("process RSS growth: {:.1} MiB (RSS {:.1} MiB)", (rss1 - rss0) as f64 / 1024.0, rss1 as f64 / 1024.0);

    let queries = [
        "re",
        "report",
        "invoice 2024",
        "reprt",
        "IMG_1234",
        "ext:pdf budget",
        "notes ext:md modified:<30d",
        "dir: project",
        "size:>400mb",
        "photo/img",
        "zzzz",
        "budget final 2019",
    ];
    let opts = SearchOptions { limit: 200, now: 1_800_000_000, ..Default::default() };
    println!("\n{:<32} {:>10} {:>10} {:>10}", "query", "matched", "p50 ms", "max ms");
    for q in queries {
        let parsed = Query::parse_at(q, 1_800_000_000, 0);
        let mut times = Vec::new();
        let mut matched = 0;
        for _ in 0..15 {
            let r = search(&ix, &parsed, &opts);
            matched = r.matched;
            times.push(r.elapsed.as_secs_f64() * 1000.0);
        }
        times.sort_by(|a, b| a.total_cmp(b));
        println!("{:<32} {:>10} {:>10.2} {:>10.2}", q, matched, times[times.len() / 2], times[times.len() - 1]);
    }

    let dir = std::env::temp_dir().join(format!("arcade-find-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("index.bin");
    let t = Instant::now();
    find_core::persist::save(&file, &ix, 1, 1).unwrap();
    let save = t.elapsed();
    let size = std::fs::metadata(&file).unwrap().len();
    drop(ix);
    let t = Instant::now();
    let (back, _) = find_core::persist::load(&file).unwrap();
    let load = t.elapsed();
    println!(
        "\nsave: {:.0} ms, load (warm start): {:.0} ms, file {:.1} MiB, {} entries",
        save.as_secs_f64() * 1000.0,
        load.as_secs_f64() * 1000.0,
        size as f64 / 1048576.0,
        back.live()
    );
    std::fs::remove_dir_all(dir).ok();
}
