use rusqlite::Connection;

/// ISO 8601 UTC timestamp for "now".
pub(crate) fn chrono_now() -> String {
    // Use std only, no chrono dependency. Format: 2026-03-22T10:15:30Z
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    epoch_to_iso8601(dur.as_secs())
}

/// Convert unix epoch seconds to ISO 8601 string (UTC, second precision).
pub(crate) fn epoch_to_iso8601(epoch: u64) -> String {
    // Manual conversion without chrono.
    let secs = epoch;
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since 1970-01-01 to year/month/day.
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert unix epoch milliseconds to ISO 8601 string with millisecond precision.
/// Format: "2026-03-27T10:15:30.500Z"
pub(crate) fn epoch_to_iso8601_ms(epoch_ms: u64) -> String {
    let secs = epoch_ms / 1000;
    let ms = epoch_ms % 1000;
    let base = epoch_to_iso8601(secs);
    // epoch_to_iso8601 returns "YYYY-MM-DDTHH:MM:SSZ"; strip the Z, append .mmmZ
    let without_z = base.trim_end_matches('Z');
    format!("{}.{:03}Z", without_z, ms)
}

/// Convert days since 1970-01-01 to (year, month, day).
pub(crate) fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Get the current time as ISO 8601 UTC string.
pub fn now_iso8601() -> String {
    chrono_now()
}

pub(crate) fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Ensure an HMAC secret exists in the settings table. Creates one if missing.
/// Returns the 32-byte secret used for HMAC-SHA256 key hashing.
/// The secret is generated from two UUID v4s (uuid is already a dep) to avoid
/// adding a CSPRNG dependency; the entropy is sufficient for HMAC keying.
pub fn ensure_hmac_secret(conn: &Connection) -> Vec<u8> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value BLOB NOT NULL);",
    )
    .expect("create settings table");

    let existing: Option<Vec<u8>> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'hmac_secret'",
            [],
            |row| row.get(0),
        )
        .ok();

    if let Some(secret) = existing {
        return secret;
    }

    // Generate 256-bit CSPRNG secret directly.
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).expect("CSPRNG failed");

    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('hmac_secret', ?1)",
        [&buf[..]],
    )
    .expect("insert hmac_secret");

    buf.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_to_iso8601_known_value() {
        let result = epoch_to_iso8601(0);
        assert_eq!(result, "1970-01-01T00:00:00Z");
    }

    #[test]
    fn epoch_to_iso8601_ms_formats_fractional_seconds() {
        assert_eq!(epoch_to_iso8601_ms(500), "1970-01-01T00:00:00.500Z");
        assert_eq!(epoch_to_iso8601_ms(1000), "1970-01-01T00:00:01.000Z");
        assert_eq!(epoch_to_iso8601_ms(1001), "1970-01-01T00:00:01.001Z");
        let result = epoch_to_iso8601_ms(1774070400000);
        assert!(result.ends_with(".000Z"), "got: {result}");
    }
}
