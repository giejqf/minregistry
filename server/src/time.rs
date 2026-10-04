//! Timestamps are stored and served as RFC 3339 UTC strings with millisecond
//! precision (`2026-10-03T19:07:00.123Z`); this fixed width sorts lexically.

use std::time::Duration;

use time::{format_description::well_known::Rfc3339, macros::format_description, OffsetDateTime, UtcOffset};

const FORMAT: &[time::format_description::BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

pub(crate) fn format(ts: OffsetDateTime) -> String {
    // Formatting a UTC timestamp with a static, valid description cannot fail.
    ts.to_offset(UtcOffset::UTC).format(FORMAT).unwrap_or_default()
}

pub(crate) fn now() -> String {
    format(OffsetDateTime::now_utc())
}

/// The timestamp `ago` before now.
pub(crate) fn before_now(ago: Duration) -> String {
    let ago = time::Duration::try_from(ago).unwrap_or(time::Duration::MAX);
    format(OffsetDateTime::now_utc().checked_sub(ago).unwrap_or(OffsetDateTime::UNIX_EPOCH))
}

/// Parses any RFC 3339 timestamp and normalizes it to the storage format.
pub(crate) fn normalize(input: &str) -> Option<String> {
    OffsetDateTime::parse(input, &Rfc3339).ok().map(format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_width_utc() {
        let ts = OffsetDateTime::parse("2026-01-02T03:04:05.6+02:00", &Rfc3339).unwrap();
        assert_eq!(format(ts), "2026-01-02T01:04:05.600Z");
        assert_eq!(normalize("2026-01-02T03:04:05Z").unwrap(), "2026-01-02T03:04:05.000Z");
        assert!(normalize("yesterday").is_none());
        assert!(before_now(Duration::from_secs(60)) < now());
    }
}
