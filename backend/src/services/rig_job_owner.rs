//! Whose a rig job is (`Owner`), kept on its record as `owner_scope` (with
//! `owner_user_id` for a site account), and where what its jobs make is kept
//! (`Storage`). The steps of a job know its owner only through these: the
//! account its paid calls are written to, and the root its files go under.

use std::path::{Path, PathBuf};

/// The scope of the site's own jobs, as a record keeps it.
const SITE: &str = "site";

/// Whose a job is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owner {
    /// The site, asked by its owner's account. Its jobs keep what they make
    /// where the site always has, one place for the whole site.
    Site { user_id: i32 },
    /// A scope of its own, for tests of what keeps one owner's jobs apart
    /// from another's: no site account, its own root.
    #[cfg(test)]
    Isolated(String),
}

impl Owner {
    /// The owner's scope, as the job's record keeps it (`owner_scope`).
    pub fn scope(&self) -> String {
        match self {
            Self::Site { .. } => SITE.to_owned(),
            #[cfg(test)]
            Self::Isolated(name) => format!("test:{name}"),
        }
    }

    /// Who asks for the owner's jobs, as their records and keys are kept
    /// (`caller`): every lookup by caller is the owner's alone. The site's
    /// is its account, `user:<id>`.
    pub fn caller(&self) -> String {
        match self {
            Self::Site { user_id } => super::user_caller(*user_id),
            #[cfg(test)]
            Self::Isolated(name) => format!("test:{name}"),
        }
    }

    /// The site account the job belongs to, when the owner is one (`owner_user_id`).
    pub fn user_id(&self) -> Option<i32> {
        match self {
            Self::Site { user_id } => Some(*user_id),
            #[cfg(test)]
            Self::Isolated(_) => None,
        }
    }

    /// The ledger subject the job's paid calls are written under: the site
    /// account, or none (`0`, as unattributed calls are) for an owner without one.
    pub fn ledger_subject(&self) -> i32 {
        self.user_id().unwrap_or(0)
    }

    /// Where what the owner's jobs make is kept.
    pub fn storage(&self) -> Storage {
        match self {
            Self::Site { .. } => Storage::site(),
            #[cfg(test)]
            Self::Isolated(name) => Storage {
                root: std::env::temp_dir().join(format!("rig-owner-{name}")),
            },
        }
    }

    /// The owner a record names: its scope and its site account. None for a
    /// scope this build does not know, or a site job whose account is gone.
    pub fn of_record(scope: &str, user_id: Option<i32>) -> Option<Self> {
        match scope {
            SITE => user_id.map(|user_id| Self::Site { user_id }),
            #[cfg(test)]
            scope => scope
                .strip_prefix("test:")
                .map(|name| Self::Isolated(name.to_owned())),
            #[cfg(not(test))]
            _ => None,
        }
    }
}

/// One owner's root for what its jobs make; each kind of file has its own
/// directory under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Storage {
    root: PathBuf,
}

impl Storage {
    /// The site's: `merope/` under the data directory.
    pub fn site() -> Self {
        Self {
            root: crate::services::data_paths::paths().root.join("merope"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_owner_reads_back_from_its_record_as_it_was_written() {
        let site = Owner::Site { user_id: 7 };
        assert_eq!(
            Owner::of_record(&site.scope(), site.user_id()),
            Some(site.clone())
        );
        let isolated = Owner::Isolated("a".into());
        assert_eq!(
            Owner::of_record(&isolated.scope(), isolated.user_id()),
            Some(isolated.clone())
        );
        // A site job whose account is gone has no owner; nor has a scope this build does not know.
        assert_eq!(Owner::of_record("site", None), None);
        assert_eq!(Owner::of_record("tenant:1", Some(7)), None);
        assert_eq!((site.ledger_subject(), isolated.ledger_subject()), (7, 0));
        // Each owner asks under its own caller: the site's is its account.
        assert_eq!(
            (site.caller(), isolated.caller()),
            ("user:7".to_owned(), "test:a".to_owned())
        );
    }

    #[test]
    fn every_site_account_keeps_to_the_sites_one_root_and_another_scope_to_its_own() {
        let root = crate::services::data_paths::paths().root.join("merope");
        assert_eq!(Owner::Site { user_id: 7 }.storage().root(), root);
        assert_eq!(Owner::Site { user_id: 8 }.storage(), Storage::site());
        let a = Owner::Isolated("a".into()).storage();
        let b = Owner::Isolated("b".into()).storage();
        assert_ne!(a, b);
        assert!(!a.root().starts_with(&root) && !b.root().starts_with(a.root()));
    }
}
