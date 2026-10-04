//! Just enough manifest parsing to validate references, record the subject
//! for the referrers API and keep GC bookkeeping. The bytes themselves are
//! stored exactly as received.

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::digest::Digest;

pub(crate) const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
pub(crate) const OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
pub(crate) const DOCKER_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";
pub(crate) const DOCKER_LIST: &str = "application/vnd.docker.distribution.manifest.list.v2+json";
pub(crate) const OCI_CONFIG: &str = "application/vnd.oci.image.config.v1+json";
pub(crate) const DOCKER_CONFIG: &str = "application/vnd.docker.container.image.v1+json";

/// Largest manifest accepted (the spec asks registries to take at least 4 MiB).
pub(crate) const MAX_MANIFEST_SIZE: usize = 4 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Kind {
    Image,
    Index,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
pub(crate) struct Platform {
    pub os: String,
    pub architecture: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// Distinguishes e.g. Windows Server releases of the same os/architecture.
    #[serde(rename = "os.version", default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Parsed {
    pub kind: Kind,
    pub media_type: String,
    /// Config and layer blobs that must exist in the repository.
    pub blobs: Vec<Digest>,
    /// Child manifests (indexes) that must exist in the repository.
    pub manifests: Vec<Digest>,
    pub subject: Option<Digest>,
    /// `artifactType`, or the config media type of an image manifest.
    pub artifact_type: Option<String>,
    pub annotations: Option<Map<String, Value>>,
    /// Platforms listed by an index.
    pub platforms: Vec<Platform>,
    /// The config descriptor of an image manifest.
    pub config: Option<(Digest, String, i64)>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ParseError {
    #[error("{0}")]
    Invalid(String),
    #[error("unsupported manifest media type {0}")]
    Unsupported(String),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDescriptor {
    media_type: Option<String>,
    digest: String,
    size: i64,
    platform: Option<Platform>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawManifest {
    schema_version: Option<i64>,
    media_type: Option<String>,
    artifact_type: Option<String>,
    config: Option<RawDescriptor>,
    layers: Option<Vec<RawDescriptor>>,
    manifests: Option<Vec<RawDescriptor>>,
    subject: Option<RawDescriptor>,
    annotations: Option<Map<String, Value>>,
}

fn kind_of(media_type: &str) -> Option<Kind> {
    match media_type {
        OCI_MANIFEST | DOCKER_MANIFEST => Some(Kind::Image),
        OCI_INDEX | DOCKER_LIST => Some(Kind::Index),
        _ => None,
    }
}

/// Layers that by design are not pushed to the registry (e.g. Windows base layers).
fn is_non_distributable(media_type: Option<&str>) -> bool {
    media_type.is_some_and(|m| m.contains("foreign") || m.contains("nondistributable"))
}

fn descriptor_digest(d: &RawDescriptor, what: &str) -> Result<Digest, ParseError> {
    if d.size < 0 {
        return Err(ParseError::Invalid(format!("{what} has a negative size")));
    }
    Digest::parse(&d.digest).map_err(|_| ParseError::Invalid(format!("{what} has an invalid digest {:?}", d.digest)))
}

/// Parses a manifest pushed with `content_type` (media type without parameters).
pub(crate) fn parse(content_type: Option<&str>, body: &[u8]) -> Result<Parsed, ParseError> {
    let raw: RawManifest =
        serde_json::from_slice(body).map_err(|e| ParseError::Invalid(format!("manifest is not valid JSON: {e}")))?;
    if raw.schema_version != Some(2) {
        return Err(ParseError::Invalid("schemaVersion must be 2".into()));
    }
    let header_type = content_type.filter(|t| kind_of(t).is_some());
    if let (Some(header), Some(body_type)) = (header_type, raw.media_type.as_deref()) {
        if header != body_type {
            return Err(ParseError::Invalid(format!(
                "Content-Type {header} does not match the manifest mediaType {body_type}"
            )));
        }
    }
    let media_type = match (header_type, raw.media_type.as_deref(), content_type) {
        (Some(h), _, _) => h.to_string(),
        (None, Some(b), _) if kind_of(b).is_some() => b.to_string(),
        (None, Some(b), _) => return Err(ParseError::Unsupported(b.to_string())),
        (None, None, Some(t)) if !t.is_empty() && !matches!(t, "application/json" | "application/octet-stream") => {
            return Err(ParseError::Unsupported(t.to_string()))
        }
        // No usable type anywhere: infer from the structure.
        (None, None, _) if raw.manifests.is_some() => OCI_INDEX.to_string(),
        (None, None, _) if raw.config.is_some() => OCI_MANIFEST.to_string(),
        (None, None, _) => return Err(ParseError::Invalid("cannot determine the manifest media type".into())),
    };
    let kind = kind_of(&media_type).ok_or_else(|| ParseError::Unsupported(media_type.clone()))?;

    let subject = raw.subject.as_ref().map(|s| descriptor_digest(s, "subject")).transpose()?;
    let mut parsed = Parsed {
        kind,
        media_type,
        blobs: Vec::new(),
        manifests: Vec::new(),
        subject,
        artifact_type: raw.artifact_type.clone().filter(|t| !t.is_empty()),
        annotations: raw.annotations.filter(|a| !a.is_empty()),
        platforms: Vec::new(),
        config: None,
    };
    match kind {
        Kind::Image => {
            if raw.manifests.is_some() {
                return Err(ParseError::Invalid("an image manifest cannot list manifests".into()));
            }
            let config =
                raw.config.as_ref().ok_or_else(|| ParseError::Invalid("image manifest has no config".into()))?;
            let config_digest = descriptor_digest(config, "config")?;
            let config_type = config.media_type.clone().unwrap_or_default();
            if parsed.artifact_type.is_none() && !config_type.is_empty() {
                parsed.artifact_type = Some(config_type.clone());
            }
            parsed.blobs.push(config_digest.clone());
            parsed.config = Some((config_digest, config_type, config.size));
            for (i, layer) in raw.layers.iter().flatten().enumerate() {
                let digest = descriptor_digest(layer, &format!("layer {i}"))?;
                if !is_non_distributable(layer.media_type.as_deref()) {
                    parsed.blobs.push(digest);
                }
            }
        }
        Kind::Index => {
            if raw.config.is_some() || raw.layers.is_some() {
                return Err(ParseError::Invalid("an index cannot have config or layers".into()));
            }
            for (i, m) in raw.manifests.iter().flatten().enumerate() {
                parsed.manifests.push(descriptor_digest(m, &format!("manifest {i}"))?);
                if let Some(p) = &m.platform {
                    parsed.platforms.push(p.clone());
                }
            }
        }
    }
    parsed.blobs.sort();
    parsed.blobs.dedup();
    parsed.manifests.sort();
    parsed.manifests.dedup();
    Ok(parsed)
}

/// The platform recorded in an image config blob, if it names one.
pub(crate) fn platform_from_config(config: &[u8]) -> Option<Platform> {
    let p: Platform = serde_json::from_slice(config).ok()?;
    (!p.os.is_empty() && !p.architecture.is_empty()).then_some(p)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn d(s: &str) -> String {
        Digest::of(s.as_bytes()).to_string()
    }

    fn image(extra: Value) -> Vec<u8> {
        let mut m = json!({
            "schemaVersion": 2,
            "mediaType": OCI_MANIFEST,
            "config": {"mediaType": OCI_CONFIG, "digest": d("config"), "size": 6, "data": "e30=", "newUnspecifiedField": "x"},
            "layers": [
                {"mediaType": "application/vnd.oci.image.layer.v1.tar+gzip", "digest": d("layer"), "size": 5},
                {"mediaType": "application/vnd.docker.image.rootfs.foreign.diff.tar.gzip", "digest": d("win"), "size": 5, "urls": ["https://x"]}
            ]
        });
        if let (Value::Object(m), Value::Object(e)) = (&mut m, extra) {
            m.extend(e);
        }
        serde_json::to_vec(&m).unwrap()
    }

    #[test]
    fn image_manifest() {
        let p = parse(Some(OCI_MANIFEST), &image(json!({}))).unwrap();
        assert_eq!(p.kind, Kind::Image);
        assert_eq!(p.blobs.len(), 2, "foreign layers are not required");
        assert_eq!(p.artifact_type.as_deref(), Some(OCI_CONFIG));
        assert!(p.subject.is_none());
        assert_eq!(p.config.as_ref().map(|c| c.1.as_str()), Some(OCI_CONFIG));
    }

    #[test]
    fn artifact_with_subject() {
        let body = image(json!({
            "artifactType": "application/vnd.example.sbom",
            "subject": {"mediaType": OCI_MANIFEST, "digest": d("subject"), "size": 10},
            "annotations": {"org.example": "x"}
        }));
        let p = parse(Some(OCI_MANIFEST), &body).unwrap();
        assert_eq!(p.artifact_type.as_deref(), Some("application/vnd.example.sbom"));
        assert_eq!(p.subject.unwrap().to_string(), d("subject"));
        assert_eq!(p.annotations.unwrap()["org.example"], "x");
    }

    #[test]
    fn media_type_resolution() {
        let mut no_type: Value = serde_json::from_slice(&image(json!({}))).unwrap();
        no_type.as_object_mut().unwrap().remove("mediaType");
        let no_type = serde_json::to_vec(&no_type).unwrap();
        assert_eq!(parse(Some(OCI_MANIFEST), &no_type).unwrap().media_type, OCI_MANIFEST);
        assert_eq!(parse(None, &no_type).unwrap().media_type, OCI_MANIFEST);
        assert_eq!(parse(Some("application/json"), &image(json!({}))).unwrap().media_type, OCI_MANIFEST);
        assert!(matches!(parse(Some(DOCKER_MANIFEST), &image(json!({}))), Err(ParseError::Invalid(_))));
        assert!(matches!(
            parse(Some("application/vnd.docker.distribution.manifest.v1+prettyjws"), &no_type),
            Err(ParseError::Unsupported(_))
        ));
    }

    #[test]
    fn index() {
        let body = serde_json::to_vec(&json!({
            "schemaVersion": 2,
            "mediaType": DOCKER_LIST,
            "manifests": [
                {"mediaType": DOCKER_MANIFEST, "digest": d("amd64"), "size": 1, "platform": {"os": "linux", "architecture": "amd64"}},
                {"mediaType": DOCKER_MANIFEST, "digest": d("arm64"), "size": 1, "platform": {"os": "linux", "architecture": "arm64", "variant": "v8"}},
                {"mediaType": DOCKER_MANIFEST, "digest": d("win"), "size": 1, "platform": {"os": "windows", "architecture": "amd64", "os.version": "10.0.20348.2655"}}
            ]
        }))
        .unwrap();
        let p = parse(Some(DOCKER_LIST), &body).unwrap();
        assert_eq!(p.kind, Kind::Index);
        assert_eq!(p.manifests.len(), 3);
        assert_eq!(p.platforms[1].variant.as_deref(), Some("v8"));
        assert_eq!(p.platforms[2].os_version.as_deref(), Some("10.0.20348.2655"));
        // Stored with the OCI field name.
        assert!(serde_json::to_string(&p.platforms[2]).unwrap().contains("\"os.version\":\"10.0.20348.2655\""));
        assert!(p.artifact_type.is_none());
    }

    #[test]
    fn invalid() {
        assert!(parse(Some(OCI_MANIFEST), b"blablabla").is_err());
        assert!(parse(Some(OCI_MANIFEST), br#"{"schemaVersion":1}"#).is_err());
        assert!(parse(
            Some(OCI_MANIFEST),
            br#"{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json"}"#
        )
        .is_err());
        let bad_digest = serde_json::to_vec(&json!({
            "schemaVersion": 2, "config": {"mediaType": OCI_CONFIG, "digest": "sha256:nope", "size": 1}, "layers": []
        }))
        .unwrap();
        assert!(parse(Some(OCI_MANIFEST), &bad_digest).is_err());
    }

    #[test]
    fn config_platform() {
        assert_eq!(
            platform_from_config(br#"{"architecture":"arm","os":"linux","variant":"v7","rootfs":{}}"#),
            Some(Platform {
                os: "linux".into(),
                architecture: "arm".into(),
                variant: Some("v7".into()),
                os_version: None
            })
        );
        assert_eq!(
            platform_from_config(br#"{"architecture":"amd64","os":"windows","os.version":"10.0.17763.6189"}"#)
                .and_then(|p| p.os_version),
            Some("10.0.17763.6189".into())
        );
        assert_eq!(platform_from_config(b"{}"), None);
    }
}
