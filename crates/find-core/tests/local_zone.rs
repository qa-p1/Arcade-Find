//! Absolute dates use the UTC offset of their own season in the real local
//! time zone (its own test binary: it changes TZ for the process).

#[test]
fn january_dates_use_standard_time_in_october() {
    if !std::path::Path::new("/usr/share/zoneinfo/America/New_York").exists() {
        eprintln!("no zoneinfo; skipped");
        return;
    }
    // SAFETY: the only test in this binary; nothing else reads the environment.
    unsafe { std::env::set_var("TZ", "America/New_York") };
    let october = 1_792_252_800; // 2026-10-17, daylight time (UTC−4)
    assert_eq!(find_core::local_offset_at(october), -4 * 3600);
    let q = find_core::Query::parse_local("modified:2026-01-01", october);
    let midnight = 1_767_225_600 + 5 * 3600; // 2026-01-01 00:00 EST
    assert_eq!(q.modified, Some((midnight, midnight + 86_399)));
}
