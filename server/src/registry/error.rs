//! OCI Distribution error codes and the spec error body
//! `{"errors":[{"code","message","detail"}]}`.

use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OciCode {
    BlobUnknown,
    BlobUploadInvalid,
    BlobUploadUnknown,
    DigestInvalid,
    ManifestBlobUnknown,
    ManifestInvalid,
    ManifestUnknown,
    NameInvalid,
    NameUnknown,
    PaginationNumberInvalid,
    RangeInvalid,
    SizeInvalid,
    TagInvalid,
    Unauthorized,
    Denied,
    Unsupported,
    Unknown,
}

impl OciCode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            OciCode::BlobUnknown => "BLOB_UNKNOWN",
            OciCode::BlobUploadInvalid => "BLOB_UPLOAD_INVALID",
            OciCode::BlobUploadUnknown => "BLOB_UPLOAD_UNKNOWN",
            OciCode::DigestInvalid => "DIGEST_INVALID",
            OciCode::ManifestBlobUnknown => "MANIFEST_BLOB_UNKNOWN",
            OciCode::ManifestInvalid => "MANIFEST_INVALID",
            OciCode::ManifestUnknown => "MANIFEST_UNKNOWN",
            OciCode::NameInvalid => "NAME_INVALID",
            OciCode::NameUnknown => "NAME_UNKNOWN",
            OciCode::PaginationNumberInvalid => "PAGINATION_NUMBER_INVALID",
            OciCode::RangeInvalid => "RANGE_INVALID",
            OciCode::SizeInvalid => "SIZE_INVALID",
            OciCode::TagInvalid => "TAG_INVALID",
            OciCode::Unauthorized => "UNAUTHORIZED",
            OciCode::Denied => "DENIED",
            OciCode::Unsupported => "UNSUPPORTED",
            OciCode::Unknown => "UNKNOWN",
        }
    }
}

pub(crate) fn body(code: OciCode, message: &str, detail: Option<&Value>) -> Value {
    let mut error = json!({ "code": code.as_str(), "message": message });
    if let Some(detail) = detail {
        error["detail"] = detail.clone();
    }
    json!({ "errors": [error] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_shape() {
        let v = body(OciCode::BlobUnknown, "blob unknown to registry", Some(&json!({"digest": "sha256:x"})));
        assert_eq!(v["errors"][0]["code"], "BLOB_UNKNOWN");
        assert_eq!(v["errors"][0]["detail"]["digest"], "sha256:x");
        assert!(body(OciCode::Denied, "no", None)["errors"][0].get("detail").is_none());
    }
}
