//! `MINREGISTRY_*` environment variables → validated configuration.
//!
//! Every variable is documented in `docs/config.md`; adding one here without
//! documenting it there is a bug.

use std::{fmt, net::SocketAddr, path::PathBuf, str::FromStr, time::Duration};

use base64::Engine;
use url::Url;

/// Where blob content lives.
#[derive(Clone, Debug)]
pub enum StorageConfig {
    Fs { root: PathBuf },
    S3(S3Config),
}

#[derive(Clone)]
pub struct S3Config {
    pub endpoint: Option<String>,
    pub region: String,
    pub bucket: String,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub path_style: bool,
}

impl fmt::Debug for S3Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Config")
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("access_key", &self.access_key.as_ref().map(|_| "<set>"))
            .field("secret_key", &self.secret_key.as_ref().map(|_| "<redacted>"))
            .field("path_style", &self.path_style)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogFormat {
    Auto,
    Json,
    Pretty,
}

/// Settings needed by every command that touches data (`serve`, `gc`, `migrate`).
#[derive(Clone, Debug)]
pub struct CoreConfig {
    pub db_path: PathBuf,
    pub storage: StorageConfig,
    pub upload_dir: PathBuf,
    pub upload_ttl: Duration,
    pub audit_blob_reads: bool,
    pub audit_retention_days: u32,
    pub gc_cron: Option<String>,
    pub gc_min_age: Duration,
    pub log: String,
    pub log_format: LogFormat,
}

#[derive(Clone)]
pub struct GithubConfig {
    pub client_id: String,
    pub client_secret: String,
    /// Web base URL (authorize + token endpoints live under it).
    pub base_url: Url,
    /// REST API base URL (`/user`).
    pub api_url: Url,
}

impl fmt::Debug for GithubConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GithubConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("base_url", &self.base_url.as_str())
            .field("api_url", &self.api_url.as_str())
            .finish()
    }
}

/// Full configuration of the `serve` command.
#[derive(Clone)]
pub struct ServeConfig {
    pub core: CoreConfig,
    pub listen: SocketAddr,
    pub public_url: Url,
    pub trust_proxy: bool,
    pub github: GithubConfig,
    /// Lower-cased GitHub logins allowed to sign in.
    pub admin_logins: Vec<String>,
    pub session_secret: Vec<u8>,
}

impl fmt::Debug for ServeConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServeConfig")
            .field("core", &self.core)
            .field("listen", &self.listen)
            .field("public_url", &self.public_url.as_str())
            .field("trust_proxy", &self.trust_proxy)
            .field("github", &self.github)
            .field("admin_logins", &self.admin_logins)
            .field("session_secret", &"<redacted>")
            .finish()
    }
}

impl ServeConfig {
    /// Reads the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(&|key| std::env::var(key).ok())
    }

    /// Reads configuration through `lookup` (the environment in production, a
    /// map in tests). All problems are reported at once.
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let mut r = Reader::new(lookup);
        let core = read_core(&mut r);
        let listen =
            r.parse("MINREGISTRY_LISTEN", "0.0.0.0:5000", |v| SocketAddr::from_str(v).map_err(|e| e.to_string()));
        let public_url = r.required("MINREGISTRY_PUBLIC_URL").and_then(|v| {
            r.check(
                "MINREGISTRY_PUBLIC_URL",
                parse_http_url(&v).map(|mut u| {
                    // The OAuth callback and cookie scope are derived from it.
                    u.set_query(None);
                    u.set_fragment(None);
                    u
                }),
            )
        });
        let trust_proxy = r.parse("MINREGISTRY_TRUST_PROXY", "false", parse_bool);
        let client_id = r.required("MINREGISTRY_GITHUB_CLIENT_ID");
        let client_secret = r.required("MINREGISTRY_GITHUB_CLIENT_SECRET");
        let base_url = r.parse("MINREGISTRY_GITHUB_URL", "https://github.com", parse_http_url);
        let api_url = r.parse("MINREGISTRY_GITHUB_API_URL", "https://api.github.com", parse_http_url);
        let admin_logins = r.required("MINREGISTRY_ADMIN_GITHUB_LOGINS").and_then(|v| {
            let logins: Vec<String> =
                v.split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).collect();
            let invalid: Vec<&String> = logins.iter().filter(|l| !is_valid_github_login(l)).collect();
            if logins.is_empty() {
                r.fail("MINREGISTRY_ADMIN_GITHUB_LOGINS", "must list at least one GitHub login");
                None
            } else if !invalid.is_empty() {
                r.fail("MINREGISTRY_ADMIN_GITHUB_LOGINS", &format!("invalid GitHub login(s): {invalid:?}"));
                None
            } else {
                Some(logins)
            }
        });
        let session_secret = r.required("MINREGISTRY_SESSION_SECRET").and_then(|v| {
            match base64::engine::general_purpose::STANDARD.decode(v.trim()) {
                Ok(bytes) if bytes.len() >= 32 => Some(bytes),
                Ok(bytes) => {
                    r.fail(
                        "MINREGISTRY_SESSION_SECRET",
                        &format!("must decode to at least 32 bytes (got {})", bytes.len()),
                    );
                    None
                }
                Err(e) => {
                    r.fail("MINREGISTRY_SESSION_SECRET", &format!("must be base64: {e}"));
                    None
                }
            }
        });
        r.finish()?;
        match (
            core,
            listen,
            public_url,
            trust_proxy,
            client_id,
            client_secret,
            base_url,
            api_url,
            admin_logins,
            session_secret,
        ) {
            (
                Some(core),
                Some(listen),
                Some(public_url),
                Some(trust_proxy),
                Some(client_id),
                Some(client_secret),
                Some(base_url),
                Some(api_url),
                Some(admin_logins),
                Some(session_secret),
            ) => Ok(ServeConfig {
                core,
                listen,
                public_url,
                trust_proxy,
                github: GithubConfig { client_id, client_secret, base_url, api_url },
                admin_logins,
                session_secret,
            }),
            _ => Err(ConfigError(vec!["incomplete configuration".into()])),
        }
    }

    /// Whether cookies must carry the `Secure` attribute.
    pub(crate) fn secure_cookies(&self) -> bool {
        self.trust_proxy || self.public_url.scheme() == "https"
    }

    pub(crate) fn is_admin_login(&self, login: &str) -> bool {
        let login = login.to_ascii_lowercase();
        self.admin_logins.contains(&login)
    }
}

impl CoreConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(&|key| std::env::var(key).ok())
    }

    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let mut r = Reader::new(lookup);
        let core = read_core(&mut r);
        r.finish()?;
        core.ok_or_else(|| ConfigError(vec!["incomplete configuration".into()]))
    }
}

fn read_core(r: &mut Reader<'_>) -> Option<CoreConfig> {
    let db_path = r.parse("MINREGISTRY_DB_PATH", "./data/minregistry.db", parse_path);
    let backend = r.parse("MINREGISTRY_STORAGE", "fs", |v| match v {
        "fs" | "s3" => Ok(v.to_string()),
        _ => Err("must be `fs` or `s3`".into()),
    });
    let storage = match backend.as_deref() {
        Some("fs") => r.parse("MINREGISTRY_FS_ROOT", "./data/blobs", parse_path).map(|root| StorageConfig::Fs { root }),
        Some("s3") => {
            let endpoint = r.optional("MINREGISTRY_S3_ENDPOINT").and_then(|v| {
                r.check(
                    "MINREGISTRY_S3_ENDPOINT",
                    parse_http_url(&v).map(|u| u.as_str().trim_end_matches('/').to_string()),
                )
            });
            let region = r.parse("MINREGISTRY_S3_REGION", "us-east-1", |v| Ok(v.to_string()));
            let bucket = r.required("MINREGISTRY_S3_BUCKET");
            let access_key = r.optional("MINREGISTRY_S3_ACCESS_KEY");
            let secret_key = r.optional("MINREGISTRY_S3_SECRET_KEY");
            if access_key.is_some() != secret_key.is_some() {
                r.fail(
                    "MINREGISTRY_S3_ACCESS_KEY",
                    "MINREGISTRY_S3_ACCESS_KEY and MINREGISTRY_S3_SECRET_KEY must be set together",
                );
            }
            let path_style = r.parse("MINREGISTRY_S3_PATH_STYLE", "false", parse_bool);
            match (region, bucket, path_style) {
                (Some(region), Some(bucket), Some(path_style)) => {
                    Some(StorageConfig::S3(S3Config { endpoint, region, bucket, access_key, secret_key, path_style }))
                }
                _ => None,
            }
        }
        _ => None,
    };
    let upload_dir = r.parse("MINREGISTRY_UPLOAD_DIR", "./data/uploads", parse_path);
    let upload_ttl = r.parse("MINREGISTRY_UPLOAD_TTL", "24h", parse_positive_duration);
    let audit_blob_reads = r.parse("MINREGISTRY_AUDIT_BLOB_READS", "false", parse_bool);
    let audit_retention_days = r.parse("MINREGISTRY_AUDIT_RETENTION_DAYS", "0", |v| {
        v.parse::<u32>().map_err(|_| "must be a non-negative integer (days)".to_string())
    });
    let gc_cron = r
        .optional("MINREGISTRY_GC_CRON")
        .and_then(|v| r.check("MINREGISTRY_GC_CRON", crate::tasks::parse_cron(&v).map(|_| v.clone())));
    let gc_min_age = r.parse("MINREGISTRY_GC_MIN_AGE", "1h", |v| {
        humantime::parse_duration(v).map_err(|e| format!("must be a duration like `1h` or `0s`: {e}"))
    });
    let log = r.parse("MINREGISTRY_LOG", "info", |v| {
        tracing_subscriber::EnvFilter::try_new(v)
            .map(|_| v.to_string())
            .map_err(|e| format!("invalid tracing filter: {e}"))
    });
    let log_format = r.parse("MINREGISTRY_LOG_FORMAT", "auto", |v| match v {
        "auto" => Ok(LogFormat::Auto),
        "json" => Ok(LogFormat::Json),
        "pretty" => Ok(LogFormat::Pretty),
        _ => Err("must be `auto`, `json` or `pretty`".into()),
    });
    Some(CoreConfig {
        db_path: db_path?,
        storage: storage?,
        upload_dir: upload_dir?,
        upload_ttl: upload_ttl?,
        audit_blob_reads: audit_blob_reads?,
        audit_retention_days: audit_retention_days?,
        gc_cron,
        gc_min_age: gc_min_age?,
        log: log?,
        log_format: log_format?,
    })
}

/// All configuration problems found, one per line.
#[derive(Debug)]
pub struct ConfigError(pub Vec<String>);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "invalid configuration:")?;
        for e in &self.0 {
            writeln!(f, "  - {e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

struct Reader<'a> {
    lookup: &'a dyn Fn(&str) -> Option<String>,
    errors: Vec<String>,
}

impl<'a> Reader<'a> {
    fn new(lookup: &'a dyn Fn(&str) -> Option<String>) -> Self {
        Reader { lookup, errors: Vec::new() }
    }

    fn optional(&mut self, key: &str) -> Option<String> {
        (self.lookup)(key).filter(|v| !v.trim().is_empty())
    }

    fn required(&mut self, key: &str) -> Option<String> {
        let v = self.optional(key);
        if v.is_none() {
            self.errors.push(format!("{key} is required"));
        }
        v
    }

    fn parse<T>(&mut self, key: &str, default: &str, f: impl FnOnce(&str) -> Result<T, String>) -> Option<T> {
        let raw = self.optional(key).unwrap_or_else(|| default.to_string());
        let parsed = f(raw.trim());
        self.check(key, parsed)
    }

    fn check<T>(&mut self, key: &str, v: Result<T, String>) -> Option<T> {
        match v {
            Ok(v) => Some(v),
            Err(e) => {
                self.fail(key, &e);
                None
            }
        }
    }

    fn fail(&mut self, key: &str, msg: &str) {
        self.errors.push(format!("{key}: {msg}"));
    }

    fn finish(self) -> Result<(), ConfigError> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(ConfigError(self.errors))
        }
    }
}

fn parse_bool(v: &str) -> Result<bool, String> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err("must be `true` or `false`".into()),
    }
}

fn parse_path(v: &str) -> Result<PathBuf, String> {
    if v.is_empty() {
        Err("must not be empty".into())
    } else {
        Ok(PathBuf::from(v))
    }
}

fn parse_http_url(v: &str) -> Result<Url, String> {
    let url = Url::parse(v).map_err(|e| format!("invalid URL: {e}"))?;
    match url.scheme() {
        "http" | "https" if url.host().is_some() => Ok(url),
        _ => Err("must be an http(s) URL".into()),
    }
}

fn parse_positive_duration(v: &str) -> Result<Duration, String> {
    let d = humantime::parse_duration(v).map_err(|e| format!("must be a duration like `24h`: {e}"))?;
    if d.is_zero() {
        Err("must be greater than zero".into())
    } else {
        Ok(d)
    }
}

/// GitHub logins: alphanumerics and single hyphens, at most 39 characters.
pub(crate) fn is_valid_github_login(login: &str) -> bool {
    !login.is_empty()
        && login.len() <= 39
        && !login.starts_with('-')
        && !login.ends_with('-')
        && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| map.get(k).cloned()
    }

    const SECRET: &str = "MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU2Nzg5MDE=";

    fn base() -> Vec<(&'static str, &'static str)> {
        vec![
            ("MINREGISTRY_PUBLIC_URL", "https://registry.example.com"),
            ("MINREGISTRY_GITHUB_CLIENT_ID", "id"),
            ("MINREGISTRY_GITHUB_CLIENT_SECRET", "secret"),
            ("MINREGISTRY_ADMIN_GITHUB_LOGINS", "Alice, bob"),
            ("MINREGISTRY_SESSION_SECRET", SECRET),
        ]
    }

    #[test]
    fn defaults() {
        let cfg = ServeConfig::from_lookup(&lookup(&base())).unwrap();
        assert_eq!(cfg.listen.to_string(), "0.0.0.0:5000");
        assert_eq!(cfg.admin_logins, vec!["alice", "bob"]);
        assert!(cfg.is_admin_login("ALICE"));
        assert!(cfg.secure_cookies());
        assert!(matches!(cfg.core.storage, StorageConfig::Fs { .. }));
        assert_eq!(cfg.core.upload_ttl, Duration::from_secs(24 * 3600));
        assert_eq!(cfg.core.gc_min_age, Duration::from_secs(3600));
        assert_eq!(cfg.github.base_url.as_str(), "https://github.com/");
    }

    #[test]
    fn reports_all_errors() {
        let err = ServeConfig::from_lookup(&lookup(&[
            ("MINREGISTRY_STORAGE", "s3"),
            ("MINREGISTRY_SESSION_SECRET", "c2hvcnQ="),
            ("MINREGISTRY_UPLOAD_TTL", "soon"),
        ]))
        .unwrap_err();
        let text = err.to_string();
        for needle in [
            "MINREGISTRY_PUBLIC_URL is required",
            "MINREGISTRY_S3_BUCKET is required",
            "MINREGISTRY_SESSION_SECRET: must decode to at least 32 bytes",
            "MINREGISTRY_UPLOAD_TTL",
            "MINREGISTRY_ADMIN_GITHUB_LOGINS is required",
        ] {
            assert!(text.contains(needle), "{needle} missing from:\n{text}");
        }
    }

    #[test]
    fn s3_and_cron() {
        let mut vars = base();
        vars.extend([
            ("MINREGISTRY_STORAGE", "s3"),
            ("MINREGISTRY_S3_BUCKET", "registry"),
            ("MINREGISTRY_S3_ENDPOINT", "http://127.0.0.1:9000/"),
            ("MINREGISTRY_S3_ACCESS_KEY", "a"),
            ("MINREGISTRY_S3_SECRET_KEY", "b"),
            ("MINREGISTRY_S3_PATH_STYLE", "true"),
            ("MINREGISTRY_GC_CRON", "0 3 * * *"),
        ]);
        let cfg = ServeConfig::from_lookup(&lookup(&vars)).unwrap();
        let StorageConfig::S3(s3) = &cfg.core.storage else { panic!("expected s3") };
        assert_eq!(s3.endpoint.as_deref(), Some("http://127.0.0.1:9000"));
        assert!(s3.path_style);
        assert!(!format!("{s3:?}").contains("\"b\""));

        vars.push(("MINREGISTRY_GC_CRON", "not a cron"));
        let map: HashMap<_, _> = vars.into_iter().collect();
        let err = ServeConfig::from_lookup(&|k| map.get(k).map(|v| v.to_string())).unwrap_err();
        assert!(err.to_string().contains("MINREGISTRY_GC_CRON"));
    }

    #[test]
    fn github_logins() {
        assert!(is_valid_github_login("octo-cat"));
        assert!(!is_valid_github_login("-octo"));
        assert!(!is_valid_github_login("octo_cat"));
        assert!(!is_valid_github_login(&"a".repeat(40)));
    }
}
