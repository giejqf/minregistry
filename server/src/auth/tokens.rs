//! Registry tokens: `mr_` + 43 base64url characters (32 random bytes).
//! Only `sha256(token)` (hex) and the first 8 characters are stored.

use base64::Engine;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub(crate) const PREFIX_LEN: usize = 8;

pub(crate) struct NewToken {
    pub secret: String,
    pub hash: String,
    pub prefix: String,
}

pub(crate) fn generate() -> anyhow::Result<NewToken> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("random number generator failed: {e}"))?;
    let secret = format!("mr_{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes));
    Ok(NewToken { hash: hash(&secret), prefix: secret[..PREFIX_LEN].to_string(), secret })
}

pub(crate) fn hash(secret: &str) -> String {
    hex::encode(Sha256::digest(secret.as_bytes()))
}

/// Constant-time comparison of two hex hashes.
pub(crate) fn hashes_equal(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_hashed() {
        let a = generate().unwrap();
        let b = generate().unwrap();
        assert_ne!(a.secret, b.secret);
        assert!(a.secret.starts_with("mr_"));
        assert_eq!(a.secret.len(), 46);
        assert_eq!(a.prefix, &a.secret[..8]);
        assert_eq!(a.hash, hash(&a.secret));
        assert_eq!(a.hash.len(), 64);
        assert!(hashes_equal(&a.hash, &hash(&a.secret)));
        assert!(!hashes_equal(&a.hash, &b.hash));
        assert!(!hashes_equal(&a.hash, "short"));
    }
}
