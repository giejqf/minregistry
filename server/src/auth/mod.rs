//! Authentication and authorization.
//!
//! - Registry clients: HTTP Basic (`name:token`) → [`RegistryPrincipal`].
//! - Admins (web UI, `/api/v1/`): GitHub OAuth session → [`AdminSession`].
//!
//! The two extractors are deliberately separate: registry routes are never
//! behind the session layer and management routes never accept Basic auth.

pub(crate) mod authz;
pub(crate) mod basic;
pub(crate) mod github;
pub(crate) mod session;
pub(crate) mod tokens;

pub(crate) use authz::{Action, Level};
pub(crate) use basic::RegistryPrincipal;
pub(crate) use session::AdminSession;

use crate::{config::Config, db::principals::PrincipalRow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PrincipalKind {
    Github,
    Identity,
}

/// An authenticated actor.
#[derive(Clone, Debug)]
pub(crate) struct Principal {
    pub id: i64,
    pub kind: PrincipalKind,
    pub name: String,
    pub is_admin: bool,
}

impl Principal {
    /// Builds a principal from its row. GitHub principals are admins exactly
    /// while their login is in `MINREGISTRY_ADMIN_GITHUB_LOGINS`; a GitHub
    /// principal removed from that list cannot authenticate at all
    /// (docs/adr/0002). Disabled principals cannot authenticate either.
    pub(crate) fn from_row(row: &PrincipalRow, cfg: &Config) -> Option<Principal> {
        if !row.enabled {
            return None;
        }
        let kind = match row.kind.as_str() {
            crate::db::principals::KIND_GITHUB => PrincipalKind::Github,
            _ => PrincipalKind::Identity,
        };
        let is_admin = kind == PrincipalKind::Github && cfg.is_admin_login(&row.name);
        if kind == PrincipalKind::Github && !is_admin {
            return None;
        }
        Some(Principal { id: row.id, kind, name: row.name.clone(), is_admin })
    }
}
