//! Repository authorization: one function, [`authorize`], used by every
//! registry handler. The decision itself is the pure [`decide`].

use crate::{
    app::AppState,
    auth::Principal,
    db::{self, repositories::RepositoryRow},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Pull,
    Push,
    Delete,
    /// Reading a blob from the *source* repository of a cross-repo mount.
    Mount,
}

/// Per-repository access level; each level implies the ones before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Level {
    Read,
    Write,
    Owner,
}

impl Level {
    pub(crate) fn parse(s: &str) -> Option<Level> {
        match s {
            "read" => Some(Level::Read),
            "write" => Some(Level::Write),
            "owner" => Some(Level::Owner),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Level::Read => "read",
            Level::Write => "write",
            Level::Owner => "owner",
        }
    }
}

/// The level an action requires on an existing repository.
pub(crate) fn required_level(action: Action) -> Level {
    match action {
        Action::Pull | Action::Mount => Level::Read,
        // Deleting manifests, tags and blobs is a write; deleting the
        // repository itself (management API) is the owner's (docs/adr/0004).
        Action::Push | Action::Delete => Level::Write,
    }
}

/// Admins may do anything. A missing repository may be pushed to (it is then
/// created) and reads as empty, so the existence checks clients make before
/// their first push get the spec's 404 (docs/adr/0004). Otherwise the
/// principal's grant must cover the action.
pub(crate) fn decide(is_admin: bool, repo_exists: bool, level: Option<Level>, action: Action) -> bool {
    if is_admin || !repo_exists {
        return true;
    }
    level.is_some_and(|l| l >= required_level(action))
}

#[derive(Debug)]
pub(crate) enum AuthzError {
    Denied,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for AuthzError {
    fn from(e: sqlx::Error) -> Self {
        AuthzError::Db(e)
    }
}

/// Authorizes `action` on `repo_name`. On success returns the repository, or
/// `None` when it does not exist: pushes create it, everything else reports
/// it unknown (404).
pub(crate) async fn authorize(
    state: &AppState,
    principal: &Principal,
    repo_name: &str,
    action: Action,
) -> Result<Option<RepositoryRow>, AuthzError> {
    let repo = db::repositories::by_name(&state.db.read, repo_name).await?;
    let level = match (&repo, principal.is_admin) {
        (Some(repo), false) => {
            db::permissions::level(&state.db.read, principal.id, repo.id).await?.as_deref().and_then(Level::parse)
        }
        _ => None,
    };
    if decide(principal.is_admin, repo.is_some(), level, action) {
        Ok(repo)
    } else {
        Err(AuthzError::Denied)
    }
}

#[cfg(test)]
mod tests {
    use super::{Action::*, Level::*, *};

    #[test]
    fn matrix() {
        let actions = [Pull, Push, Delete, Mount];
        // (level, [pull, push, delete, mount])
        let table: [(Option<Level>, [bool; 4]); 4] = [
            (None, [false, false, false, false]),
            (Some(Read), [true, false, false, true]),
            (Some(Write), [true, true, true, true]),
            (Some(Owner), [true, true, true, true]),
        ];
        for (level, expected) in table {
            for (action, want) in actions.iter().zip(expected) {
                assert_eq!(decide(false, true, level, *action), want, "{level:?} {action:?}");
            }
        }
    }

    #[test]
    fn admins_bypass_everything() {
        for action in [Pull, Push, Delete, Mount] {
            assert!(decide(true, true, None, action));
            assert!(decide(true, false, None, action));
        }
    }

    #[test]
    fn missing_repositories_are_creatable_and_read_as_empty() {
        for action in [Pull, Push, Delete, Mount] {
            assert!(decide(false, false, None, action), "{action:?}");
        }
    }

    #[test]
    fn levels() {
        assert!(Owner > Write && Write > Read);
        for l in [Read, Write, Owner] {
            assert_eq!(Level::parse(l.as_str()), Some(l));
        }
        assert_eq!(Level::parse("admin"), None);
    }
}
