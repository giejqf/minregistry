//! `Range` (blob downloads) and `Content-Range` (chunked uploads) headers.

use crate::storage::Range;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RangeRequest {
    /// No usable range: serve the whole blob.
    Full,
    Partial(Range),
    /// Syntactically valid but outside the blob: 416.
    Unsatisfiable,
}

/// Interprets `Range: bytes=…` against a blob of `size` bytes. Multiple
/// ranges and malformed headers are ignored (the full blob is served), as
/// RFC 9110 permits.
pub(crate) fn parse_range(header: Option<&str>, size: u64) -> RangeRequest {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return RangeRequest::Full;
    };
    if spec.contains(',') {
        return RangeRequest::Full;
    }
    let Some((start, end)) = spec.trim().split_once('-') else {
        return RangeRequest::Full;
    };
    let (start, end) = (start.trim(), end.trim());
    let parsed = match (start.is_empty(), end.is_empty()) {
        // bytes=-N: the last N bytes
        (true, false) => match end.parse::<u64>() {
            Ok(0) => return RangeRequest::Unsatisfiable,
            Ok(n) if size == 0 => return if n > 0 { RangeRequest::Unsatisfiable } else { RangeRequest::Full },
            Ok(n) => Some((size.saturating_sub(n), size - 1)),
            Err(_) => None,
        },
        (false, true) => start.parse::<u64>().ok().map(|s| (s, size.saturating_sub(1))),
        (false, false) => match (start.parse::<u64>(), end.parse::<u64>()) {
            (Ok(s), Ok(e)) if e >= s => Some((s, e.min(size.saturating_sub(1)))),
            _ => None,
        },
        (true, true) => None,
    };
    match parsed {
        None => RangeRequest::Full,
        Some((start, _)) if start >= size => RangeRequest::Unsatisfiable,
        Some((start, end)) => RangeRequest::Partial(Range { start, end }),
    }
}

/// Parses an upload `Content-Range`: `<start>-<end>` as the spec writes it,
/// also accepting the RFC forms `bytes <start>-<end>/<total>` and `bytes=…`.
pub(crate) fn parse_content_range(value: &str) -> Option<Range> {
    let v = value.trim();
    let v = v.strip_prefix("bytes=").or_else(|| v.strip_prefix("bytes ")).unwrap_or(v);
    let v = v.split('/').next()?.trim();
    let (start, end) = v.split_once('-')?;
    let (start, end) = (start.trim().parse::<u64>().ok()?, end.trim().parse::<u64>().ok()?);
    (end >= start).then_some(Range { start, end })
}

#[cfg(test)]
mod tests {
    use super::{RangeRequest::*, *};

    fn r(start: u64, end: u64) -> RangeRequest {
        Partial(Range { start, end })
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range(None, 10), Full);
        assert_eq!(parse_range(Some("bytes=0-4"), 10), r(0, 4));
        assert_eq!(parse_range(Some("bytes=5-"), 10), r(5, 9));
        assert_eq!(parse_range(Some("bytes=-3"), 10), r(7, 9));
        assert_eq!(parse_range(Some("bytes=-30"), 10), r(0, 9));
        assert_eq!(parse_range(Some("bytes=8-100"), 10), r(8, 9));
        assert_eq!(parse_range(Some("bytes=10-"), 10), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=10-12"), 10), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=-0"), 10), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=0-0"), 0), Unsatisfiable);
        assert_eq!(parse_range(Some("bytes=4-2"), 10), Full);
        assert_eq!(parse_range(Some("bytes=0-1,4-5"), 10), Full);
        assert_eq!(parse_range(Some("items=0-1"), 10), Full);
        assert_eq!(parse_range(Some("bytes=a-b"), 10), Full);
    }

    #[test]
    fn content_ranges() {
        assert_eq!(parse_content_range("0-21"), Some(Range { start: 0, end: 21 }));
        assert_eq!(parse_content_range("bytes 22-41/42"), Some(Range { start: 22, end: 41 }));
        assert_eq!(parse_content_range("bytes=5-9"), Some(Range { start: 5, end: 9 }));
        assert_eq!(parse_content_range("9-5"), None);
        assert_eq!(parse_content_range("x"), None);
    }
}
