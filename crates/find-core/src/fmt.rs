//! Calendar dates for exact timestamps. Sizes, counts and relative dates
//! use the Arcade design kit's format helpers (`arcade_link::design::format`).

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

/// `2026-10-09 14:32` for detail views.
pub fn datetime(t: i64, offset: i64) -> String {
    let (y, m, d, hh, mm) = civil(t, offset);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_timestamps() {
        assert_eq!(datetime(0, 0), "1970-01-01 00:00");
        let t = crate::query::parse_date("2026-10-09", 0).unwrap() + 15 * 3600 + 5 * 60;
        assert_eq!(datetime(t, 0), "2026-10-09 15:05");
        assert_eq!(datetime(t, 3600), "2026-10-09 16:05");
        assert_eq!(civil(t, 0), (2026, 10, 9, 15, 5));
    }
}
