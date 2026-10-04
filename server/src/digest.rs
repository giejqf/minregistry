//! Content digests (`sha256:<64 hex>`). Only sha256 is supported for content
//! we store; other well-formed algorithms are reported as unsupported.

use std::fmt;

use sha2::{Digest as _, Sha256};

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Digest(String);

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum DigestError {
    #[error("invalid digest")]
    Invalid,
    #[error("unsupported digest algorithm")]
    Unsupported,
}

impl Digest {
    pub(crate) fn parse(s: &str) -> Result<Self, DigestError> {
        let (algo, encoded) = s.split_once(':').ok_or(DigestError::Invalid)?;
        if !valid_algorithm(algo) || encoded.is_empty() || !encoded.bytes().all(valid_encoded_byte) {
            return Err(DigestError::Invalid);
        }
        match algo {
            "sha256" if encoded.len() == 64 && encoded.bytes().all(is_lower_hex) => Ok(Digest(s.to_string())),
            "sha256" => Err(DigestError::Invalid),
            "sha512" if encoded.len() != 128 || !encoded.bytes().all(is_lower_hex) => Err(DigestError::Invalid),
            _ => Err(DigestError::Unsupported),
        }
    }

    pub(crate) fn from_sha256(hash: &[u8]) -> Self {
        Digest(format!("sha256:{}", hex::encode(hash)))
    }

    pub(crate) fn of(bytes: &[u8]) -> Self {
        Self::from_sha256(&Sha256::digest(bytes))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn algorithm(&self) -> &str {
        self.0.split_once(':').map(|(a, _)| a).unwrap_or_default()
    }

    pub(crate) fn hex(&self) -> &str {
        self.0.split_once(':').map(|(_, h)| h).unwrap_or_default()
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn valid_algorithm(a: &str) -> bool {
    // algorithm-component ([+._-] algorithm-component)*, component = [a-z0-9]+
    !a.is_empty()
        && a.split(['+', '.', '_', '-'])
            .all(|c| !c.is_empty() && c.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

fn valid_encoded_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'=' | b'_' | b'-')
}

fn is_lower_hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sha256() {
        let d = Digest::of(b"hello world");
        assert_eq!(d.as_str(), "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
        assert_eq!(Digest::parse(d.as_str()).unwrap(), d);
        assert_eq!(d.algorithm(), "sha256");
        assert_eq!(d.hex().len(), 64);
    }

    #[test]
    fn rejects_bad_digests() {
        assert_eq!(Digest::parse("sha256:totallywrong"), Err(DigestError::Invalid));
        assert_eq!(Digest::parse("sha256:ABCDEF"), Err(DigestError::Invalid));
        assert_eq!(Digest::parse(&format!("sha256:{}", "A".repeat(64))), Err(DigestError::Invalid));
        assert_eq!(Digest::parse("nocolon"), Err(DigestError::Invalid));
        assert_eq!(Digest::parse(":abc"), Err(DigestError::Invalid));
        assert_eq!(Digest::parse("sha256:"), Err(DigestError::Invalid));
        assert_eq!(Digest::parse(&format!("sha512:{}", "a".repeat(128))), Err(DigestError::Unsupported));
        assert_eq!(Digest::parse("blake3:abc"), Err(DigestError::Unsupported));
        assert_eq!(Digest::parse("sha512:abc"), Err(DigestError::Invalid));
    }
}
