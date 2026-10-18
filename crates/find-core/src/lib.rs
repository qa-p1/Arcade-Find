//! Arcade Find core: the in-memory file index, crawler and watchers, the
//! query language, ranking, frecency and settings. No UI and no networking.

pub mod content;
pub mod engine;
pub mod exclude;
pub mod fmt;
pub mod frecency;
pub mod index;
pub mod kind;
pub mod matcher;
pub mod mounts;
pub mod paths;
pub mod persist;
pub mod query;
pub mod scan;
pub mod search;
pub mod settings;
pub mod watch;

pub use engine::{Engine, EngineEvent, EngineStatus, Phase};
pub use index::Index;
pub use query::Query;
pub use search::{Hit, SearchOptions, SearchResult};
pub use settings::Settings;

/// The current time in Unix seconds.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The local UTC offset in seconds (east positive), for day boundaries.
pub fn local_offset_secs() -> i64 {
    local_offset_at(now_secs())
}

/// The local UTC offset in effect at `time` (Unix seconds): a January date
/// in New York is UTC−5 even when it's UTC−4 today.
pub fn local_offset_at(time: i64) -> i64 {
    #[cfg(unix)]
    {
        let now = time as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: valid pointers; localtime_r is thread-safe.
        if unsafe { libc::localtime_r(&now, &mut tm) }.is_null() {
            return 0;
        }
        // libc's c_long is 32 bits on some Unix targets.
        #[allow(clippy::unnecessary_cast)]
        {
            tm.tm_gmtoff as i64
        }
    }
    #[cfg(windows)]
    {
        // Windows reports the current zone state only; good enough there.
        let _ = time;
        use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        let mut tz: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: valid out pointer.
        let r = unsafe { GetTimeZoneInformation(&mut tz) };
        let bias = tz.Bias + if r == 2 { tz.DaylightBias } else { tz.StandardBias };
        -(bias as i64) * 60
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = time;
        0
    }
}

/// A fresh, empty folder for a test.
#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("arcade-find-test-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
