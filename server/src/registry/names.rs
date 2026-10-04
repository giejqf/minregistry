//! Repository names, tags and references, validated per the distribution spec,
//! and parsing of `/v2/` paths (names contain slashes, so routes are matched
//! from the end of the path).

use std::sync::LazyLock;

use regex::Regex;

static NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*(/[a-z0-9]+((\.|_|__|-+)[a-z0-9]+)*)*$").expect("valid regex")
});
static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_][a-zA-Z0-9._-]{0,127}$").expect("valid regex"));

pub(crate) const MAX_NAME_LEN: usize = 255;

pub(crate) fn valid_name(name: &str) -> bool {
    name.len() <= MAX_NAME_LEN && NAME_RE.is_match(name)
}

pub(crate) fn valid_tag(tag: &str) -> bool {
    TAG_RE.is_match(tag)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Endpoint {
    /// `/v2/`
    Base,
    /// `/v2/<name>/tags/list`
    Tags { name: String },
    /// `/v2/<name>/manifests/<reference>`
    Manifest { name: String, reference: String },
    /// `/v2/<name>/blobs/<digest>`
    Blob { name: String, digest: String },
    /// `/v2/<name>/blobs/uploads/`
    UploadStart { name: String },
    /// `/v2/<name>/blobs/uploads/<uuid>`
    Upload { name: String, uuid: String },
    /// `/v2/<name>/referrers/<digest>`
    Referrers { name: String, digest: String },
}

/// Parses the part of the path after `/v2/`.
pub(crate) fn parse_path(rest: &str) -> Option<Endpoint> {
    if rest.is_empty() {
        return Some(Endpoint::Base);
    }
    let segs: Vec<&str> = rest.split('/').collect();
    let n = segs.len();
    let name = |upto: usize| -> Option<String> {
        let name = segs[..upto].join("/");
        (!name.is_empty()).then_some(name)
    };
    if n >= 3 && segs[n - 3] == "blobs" && segs[n - 2] == "uploads" {
        let name = name(n - 3)?;
        return Some(if segs[n - 1].is_empty() {
            Endpoint::UploadStart { name }
        } else {
            Endpoint::Upload { name, uuid: segs[n - 1].to_string() }
        });
    }
    if n < 3 {
        return None;
    }
    let last = segs[n - 1];
    if last.is_empty() {
        return None;
    }
    let name = name(n - 2)?;
    match (segs[n - 2], last) {
        ("tags", "list") => Some(Endpoint::Tags { name }),
        ("blobs", "uploads") => Some(Endpoint::UploadStart { name }),
        ("blobs", digest) => Some(Endpoint::Blob { name, digest: digest.to_string() }),
        ("manifests", reference) => Some(Endpoint::Manifest { name, reference: reference.to_string() }),
        ("referrers", digest) => Some(Endpoint::Referrers { name, digest: digest.to_string() }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Endpoint::*, *};

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn names() {
        for ok in ["a", "library/alpine", "a.b_c__d--e/f0", "x/y/z", "conformance-1234"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", "A", "a/", "/a", "a//b", "a_", "-a", "a..b", "a___b", "a:b", &"a".repeat(256)] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn tags() {
        assert!(valid_tag("latest"));
        assert!(valid_tag("v1.0_rc-1"));
        assert!(valid_tag(&"a".repeat(128)));
        assert!(!valid_tag(&"a".repeat(129)));
        assert!(!valid_tag(".INVALID_MANIFEST_NAME"));
        assert!(!valid_tag("-x"));
        assert!(!valid_tag("a/b"));
    }

    #[test]
    fn paths() {
        assert_eq!(parse_path(""), Some(Base));
        assert_eq!(parse_path("a/b/tags/list"), Some(Tags { name: s("a/b") }));
        assert_eq!(parse_path("a/manifests/latest"), Some(Manifest { name: s("a"), reference: s("latest") }));
        assert_eq!(parse_path("a/blobs/sha256:00"), Some(Blob { name: s("a"), digest: s("sha256:00") }));
        assert_eq!(parse_path("a/b/blobs/uploads/"), Some(UploadStart { name: s("a/b") }));
        assert_eq!(parse_path("a/b/blobs/uploads"), Some(UploadStart { name: s("a/b") }));
        assert_eq!(parse_path("a/blobs/uploads/123"), Some(Upload { name: s("a"), uuid: s("123") }));
        assert_eq!(parse_path("a/referrers/sha256:00"), Some(Referrers { name: s("a"), digest: s("sha256:00") }));
        // Components that look like endpoint keywords belong to the name.
        assert_eq!(parse_path("x/manifests/blobs/uploads/"), Some(UploadStart { name: s("x/manifests") }));
        assert_eq!(parse_path("tags/list/manifests/v1"), Some(Manifest { name: s("tags/list"), reference: s("v1") }));
        for bad in ["a", "a/b", "manifests/latest", "a/manifests/", "a/unknown/x", "blobs/uploads/"] {
            assert_eq!(parse_path(bad), None, "{bad}");
        }
    }
}
