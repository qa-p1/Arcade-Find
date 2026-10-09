//! Matching one word against one name, and scoring the match.
//!
//! Exact > stem (name without extension) > prefix > substring at a word
//! boundary > substring > fuzzy (characters in order). Matching is
//! case-insensitive: ASCII on the fast path, full Unicode lower-casing when
//! the word or the name needs it.

use crate::exclude::glob_match_ci;

pub const EXACT: i32 = 1000;
pub const STEM: i32 = 950;
pub const PREFIX: i32 = 800;
pub const BOUNDARY: i32 = 620;
pub const SUBSTRING: i32 = 460;
pub const GLOB: i32 = 700;
pub const FUZZY_MAX: i32 = 340;
pub const FUZZY_MIN: i32 = 90;

#[inline]
fn is_sep(b: u8) -> bool {
    matches!(b, b' ' | b'_' | b'-' | b'.' | b'(' | b')' | b'[' | b']' | b',' | b'+' | b'@' | b'#' | b'&' | b'\'' | b'~')
}

/// Is position `i` of `name` the start of a word?
#[inline]
pub fn boundary(name: &[u8], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let (p, c) = (name[i - 1], name[i]);
    is_sep(p)
        || (p.is_ascii_lowercase() && c.is_ascii_uppercase())
        || (p.is_ascii_alphabetic() && c.is_ascii_digit())
        || (p.is_ascii_digit() && c.is_ascii_alphabetic())
}

/// Bonus (0..=100) for how much of the name the word covers.
#[inline]
fn coverage(term_len: usize, name_len: usize) -> i32 {
    if name_len == 0 {
        return 0;
    }
    ((term_len * 100) / name_len).min(100) as i32
}

/// The stem length: up to the last `.` (not counting a leading dot).
#[inline]
pub fn stem_len(name: &[u8]) -> usize {
    match memchr::memrchr(b'.', name) {
        Some(0) | None => name.len(),
        Some(i) => i,
    }
}

#[inline(always)]
pub fn fold(b: u8) -> u8 {
    b | (((b.wrapping_sub(b'A') < 26) as u8) << 5)
}

#[inline(always)]
fn eq_folded(a: &[u8], lower: &[u8]) -> bool {
    a.len() == lower.len() && a.iter().zip(lower).all(|(x, y)| fold(*x) == *y)
}

/// Scores an ASCII lower-case `term` against `name`. `None` if it doesn't match.
pub fn score_ascii(name: &[u8], term: &[u8], fuzzy: bool) -> Option<i32> {
    let (tl, nl) = (term.len(), name.len());
    if tl == 0 {
        return Some(0);
    }
    if tl > nl {
        return None;
    }
    let f = term[0];
    let rest = &term[1..];
    let last_start = nl - tl;
    let mut best: Option<i32> = None;
    let mut i = 0;
    while i <= last_start {
        if fold(name[i]) == f && eq_folded(&name[i + 1..i + tl], rest) {
            if i == 0 {
                if nl == tl {
                    return Some(EXACT);
                }
                if name[tl] == b'.' && stem_len(name) == tl {
                    return Some(STEM);
                }
                return Some(PREFIX + coverage(tl, stem_len(name).max(tl)));
            }
            if boundary(name, i) {
                return Some(BOUNDARY + coverage(tl, stem_len(name).max(tl)));
            }
            if best.is_none() {
                best = Some(SUBSTRING + coverage(tl, stem_len(name).max(tl)));
            }
        }
        i += 1;
    }
    if best.is_some() || !fuzzy || tl < 2 {
        return best;
    }
    fuzzy_ascii(name, term)
}

/// Characters of `term` in order inside `name`, scored by gaps and word starts.
pub fn fuzzy_ascii(name: &[u8], term: &[u8]) -> Option<i32> {
    let first = term[0];
    let mut s = name.iter().position(|&b| fold(b) == first)?;
    let mut best: Option<i32> = None;
    for _ in 0..4 {
        match fuzzy_from(name, term, s) {
            Some(score) => {
                if best.is_none_or(|b| score > b) {
                    best = Some(score);
                }
            }
            None => break,
        }
        match name[s + 1..].iter().position(|&b| fold(b) == first) {
            Some(o) => s = s + 1 + o,
            None => break,
        }
    }
    best.filter(|&b| b >= FUZZY_MIN)
}

fn fuzzy_from(name: &[u8], term: &[u8], start: usize) -> Option<i32> {
    let mut score = FUZZY_MAX;
    let mut ni = start;
    let mut prev: Option<usize> = None;
    let mut run = 0;
    for &t in term {
        let mut found = None;
        while ni < name.len() {
            if fold(name[ni]) == t {
                found = Some(ni);
                break;
            }
            ni += 1;
        }
        let i = found?;
        if let Some(p) = prev {
            let gap = (i - p - 1) as i32;
            if gap == 0 {
                run += 1;
                score += 6 * run.min(4);
            } else {
                run = 0;
                score -= 10 + gap.min(12) * 6;
            }
        } else if i > 0 {
            score -= 15 + (i as i32).min(20);
        }
        if boundary(name, i) {
            score += 14;
        }
        prev = Some(i);
        ni = i + 1;
    }
    Some(score.min(FUZZY_MAX + 40) - (name.len() as i32 / 12))
}

/// Unicode path: lower-case both sides and run the same rules.
pub fn score_unicode(name: &[u8], term_lower: &str, fuzzy: bool) -> Option<i32> {
    let lowered = String::from_utf8_lossy(name).to_lowercase();
    let n = lowered.as_bytes();
    let t = term_lower.as_bytes();
    if t.is_empty() {
        return Some(0);
    }
    if n == t {
        return Some(EXACT);
    }
    let stem = stem_len(n);
    if &n[..stem] == t {
        return Some(STEM);
    }
    if let Some(i) = memchr::memmem::find(n, t) {
        let base = if i == 0 {
            PREFIX
        } else if boundary(n, i) || !n[i - 1].is_ascii_alphanumeric() && n[i - 1] < 0x80 {
            BOUNDARY
        } else {
            SUBSTRING
        };
        return Some(base + coverage(t.len(), stem.max(t.len())));
    }
    if !fuzzy {
        return None;
    }
    // Fuzzy by characters.
    let tc: Vec<char> = term_lower.chars().collect();
    if tc.len() < 2 {
        return None;
    }
    let mut it = lowered.chars();
    let mut matched = 0;
    let mut gaps = 0;
    let mut started = false;
    for want in &tc {
        let mut gap = 0;
        loop {
            match it.next() {
                Some(c) if c == *want => break,
                Some(_) => gap += 1,
                None => return None,
            }
        }
        if started {
            gaps += gap.min(12);
        }
        started = true;
        matched += 1;
    }
    let score = FUZZY_MAX - gaps * 8 - (lowered.chars().count() as i32 / 12);
    (matched == tc.len() && score >= FUZZY_MIN).then_some(score)
}

/// Scores a name word. An ASCII word only ever matches ASCII bytes, so it
/// can be compared byte-wise even inside a non-ASCII name; only non-ASCII
/// words need full Unicode lower-casing.
pub fn score_name(name: &[u8], term: &str, fuzzy: bool) -> Option<i32> {
    if term.is_ascii() {
        score_ascii(name, term.as_bytes(), fuzzy)
    } else {
        score_unicode(name, term, fuzzy)
    }
}

pub fn score_glob(name: &[u8], pattern: &str) -> Option<i32> {
    glob_match_ci(pattern.as_bytes(), name).then_some(GLOB + coverage(pattern.len(), name.len()) / 4)
}

/// Byte ranges of `name` to highlight for `term` (best effort, for display).
pub fn highlight(name: &str, term: &str) -> Vec<(usize, usize)> {
    if term.is_empty() {
        return Vec::new();
    }
    let lower = name.to_lowercase();
    if lower.len() == name.len() {
        if let Some(i) = best_substring(lower.as_bytes(), term.as_bytes()) {
            return vec![(i, i + term.len())];
        }
        // Fuzzy: greedy character positions, merged into runs.
        let mut out: Vec<(usize, usize)> = Vec::new();
        let mut it = lower.char_indices();
        for want in term.chars() {
            loop {
                match it.next() {
                    Some((i, c)) if c == want => {
                        let end = i + c.len_utf8();
                        match out.last_mut() {
                            Some(last) if last.1 == i => last.1 = end,
                            _ => out.push((i, end)),
                        }
                        break;
                    }
                    Some(_) => {}
                    None => return out,
                }
            }
        }
        return out;
    }
    Vec::new()
}

fn best_substring(name: &[u8], term: &[u8]) -> Option<usize> {
    let mut best = None;
    for (i, _) in memchr::memmem::find_iter(name, term).map(|i| (i, ())) {
        if i == 0 || boundary(name, i) {
            return Some(i);
        }
        best.get_or_insert(i);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str, term: &str) -> Option<i32> {
        score_name(name.as_bytes(), &term.to_lowercase(), true)
    }

    #[test]
    fn ordering_of_match_kinds() {
        let exact = s("Report", "report").unwrap();
        let stem = s("report.pdf", "report").unwrap();
        let prefix = s("reports-2026.pdf", "report").unwrap();
        let boundary = s("annual-report.pdf", "report").unwrap();
        let sub = s("misreported.txt", "report").unwrap();
        let fuzzy = s("r_e_p_o_r_t.txt", "report").unwrap();
        assert!(
            exact > stem && stem > prefix && prefix > boundary && boundary > sub && sub > fuzzy,
            "{exact} {stem} {prefix} {boundary} {sub} {fuzzy}"
        );
    }

    #[test]
    fn camel_and_digits_are_boundaries() {
        assert!(s("MyReport.doc", "report").unwrap() >= BOUNDARY);
        assert!(s("v2report", "report").unwrap() >= BOUNDARY);
    }

    #[test]
    fn fuzzy_rejects_scattered() {
        assert!(s("abc", "xyz").is_none());
        assert!(s("main.rs", "mrs").is_some());
        assert!(s("a_very_long_file_name_with_many_parts_and_stuff.txt", "azq").is_none());
        assert!(score_name(b"main.rs", "mrs", false).is_none());
    }

    #[test]
    fn unicode_case() {
        assert_eq!(s("Übersicht.pdf", "übersicht"), Some(STEM));
        assert!(s("ÉTÉ 2026.jpg", "été").is_some());
        // An ASCII word inside a non-ASCII name.
        assert!(s("Ångström-report.pdf", "report").is_some());
    }

    #[test]
    fn globs() {
        assert!(score_glob(b"photo.JPG", "*.jpg").is_some());
        assert!(score_glob(b"photo.jpeg", "*.jpg").is_none());
    }

    #[test]
    fn highlights() {
        assert_eq!(highlight("Annual-Report.pdf", "report"), vec![(7, 13)]);
        assert_eq!(highlight("main.rs", "mrs"), vec![(0, 1), (5, 7)]);
    }
}
