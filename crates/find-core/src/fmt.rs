//! Human-readable sizes, dates and counts.

/// `512 bytes`, `32 KB`, `1.5 MB` (binary units, as Arcade Link writes limits).
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if bytes < 1024 {
        return if bytes == 1 { "1 byte".into() } else { format!("{bytes} bytes") };
    }
    let mut v = bytes as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v >= 100.0 || (v - v.round()).abs() < 0.05 {
        format!("{:.0} {}", v.round(), UNITS[u])
    } else {
        format!("{:.1} {}", v, UNITS[u])
    }
}

/// `1,234,567`
pub fn count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// (year, month 1-12, day, hour, minute) for Unix seconds plus an offset.
pub fn civil(t: i64, offset: i64) -> (i64, u32, u32, u32, u32) {
    let s = t + offset;
    let days = s.div_euclid(86_400);
    let rem = s.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, (rem / 3600) as u32, (rem % 3600 / 60) as u32)
}

/// `Today 14:32`, `Yesterday 09:05`, `3 Oct`, `3 Oct 2024`.
pub fn date(t: i64, now: i64, offset: i64) -> String {
    if t <= 0 {
        return String::new();
    }
    let (y, m, d, hh, mm) = civil(t, offset);
    let (ny, _, _, _, _) = civil(now, offset);
    let today = crate::query::local_midnight(now, offset);
    if t >= today && t < today + 86_400 {
        return format!("Today {hh:02}:{mm:02}");
    }
    if t >= today - 86_400 && t < today {
        return format!("Yesterday {hh:02}:{mm:02}");
    }
    if y == ny {
        format!("{d} {}", MONTHS[(m - 1) as usize])
    } else {
        format!("{d} {} {y}", MONTHS[(m - 1) as usize])
    }
}

/// `2026-10-09 14:32` for detail views.
pub fn datetime(t: i64, offset: i64) -> String {
    let (y, m, d, hh, mm) = civil(t, offset);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(size(0), "0 bytes");
        assert_eq!(size(1), "1 byte");
        assert_eq!(size(1536), "1.5 KB");
        assert_eq!(size(32 * 1024), "32 KB");
        assert_eq!(size(1_572_864), "1.5 MB");
        assert_eq!(size(250 * 1024 * 1024), "250 MB");
    }

    #[test]
    fn counts_and_dates() {
        assert_eq!(count(1_234_567), "1,234,567");
        assert_eq!(count(12), "12");
        let now = crate::query::parse_date("2026-10-09", 0).unwrap() + 15 * 3600;
        assert_eq!(date(now - 3600, now, 0), "Today 14:00");
        assert_eq!(date(now - 86_400, now, 0), "Yesterday 15:00");
        assert_eq!(date(crate::query::parse_date("2026-03-03", 0).unwrap(), now, 0), "3 Mar");
        assert_eq!(date(crate::query::parse_date("2024-12-31", 0).unwrap(), now, 0), "31 Dec 2024");
        assert_eq!(datetime(0, 0), "1970-01-01 00:00");
    }
}
