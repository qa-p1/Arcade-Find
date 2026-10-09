//! Per-keystroke search latency on a saved index, as a user types.
//!
//!     cargo run --release -p find-core --example typing -- path/to/index.bin

use std::time::Instant;

use find_core::search::{search_narrowing, Narrowing};
use find_core::{Query, SearchOptions};

fn main() {
    let path = std::env::args().nth(1).expect("usage: typing <index.bin>");
    let (ix, _) = find_core::persist::load(std::path::Path::new(&path)).expect("load index");
    println!("{} entries", ix.live());
    for word in ["report", "invoice 2024", "photo/img", "needle-unique", "budget ext:pdf", "zzzz"] {
        let mut nw = Narrowing::default();
        let mut typed = String::new();
        let mut line = Vec::new();
        let mut worst = 0.0f64;
        for c in word.chars() {
            typed.push(c);
            let q = Query::parse(&typed);
            let opts = SearchOptions { limit: 200, now: find_core::now_secs(), ..Default::default() };
            let t = Instant::now();
            let r = search_narrowing(&ix, &q, &opts, Some(&mut nw));
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            worst = worst.max(ms);
            line.push(format!("{typed:?} {ms:.1}ms ({})", r.matched));
        }
        println!("{word:>16}: worst {worst:.1} ms | {}", line.join(" · "));
    }
}
