//! Domain types for the platform media service.
//!
//! Callers construct [`MediaContext`] from an already-authenticated subject.
//! Request bodies must not choose owner, scope, or storage key.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use super::error::MediaError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaScope {
    Site,
    User,
    LegacyUnknown,
}

impl MediaScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Site => "site",
            Self::User => "user",
            Self::LegacyUnknown => "legacy_unknown",
        }
    }

    pub fn parse(value: &str) -> Result<Self, MediaError> {
        match value {
            "site" => Ok(Self::Site),
            "user" => Ok(Self::User),
            "legacy_unknown" => Ok(Self::LegacyUnknown),
            _ => Err(MediaError::invalid("Invalid media scope")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaState {
    Staging,
    Ready,
    Deleting,
    Missing,
    Deleted,
}

impl MediaState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Staging => "staging",
            Self::Ready => "ready",
            Self::Deleting => "deleting",
            Self::Missing => "missing",
            Self::Deleted => "deleted",
        }
    }

    pub fn parse(value: &str) -> Result<Self, MediaError> {
        match value {
            "staging" => Ok(Self::Staging),
            "ready" => Ok(Self::Ready),
            "deleting" => Ok(Self::Deleting),
            "missing" => Ok(Self::Missing),
            "deleted" => Ok(Self::Deleted),
            _ => Err(MediaError::invalid("Invalid media state")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaExposure {
    Private,
    Public,
}

impl MediaExposure {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Public => "public",
        }
    }

    pub fn parse(value: &str) -> Result<Self, MediaError> {
        match value {
            "private" => Ok(Self::Private),
            "public" => Ok(Self::Public),
            _ => Err(MediaError::invalid("Invalid media exposure")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaSource {
    Upload,
    Generated,
    Channel,
    Import,
    Legacy,
}

impl MediaSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Generated => "generated",
            Self::Channel => "channel",
            Self::Import => "import",
            Self::Legacy => "legacy",
        }
    }

    pub fn parse(value: &str) -> Result<Self, MediaError> {
        match value {
            "upload" => Ok(Self::Upload),
            "generated" => Ok(Self::Generated),
            "channel" => Ok(Self::Channel),
            "import" => Ok(Self::Import),
            "legacy" => Ok(Self::Legacy),
            _ => Err(MediaError::invalid("Invalid media source")),
        }
    }

    /// Compatibility projection onto the historical `kind` column.
    pub fn catalog_kind(self) -> &'static str {
        match self {
            Self::Generated => "generated",
            Self::Upload | Self::Channel | Self::Import | Self::Legacy => "upload",
        }
    }
}

/// Already-authenticated caller. Never built from a request-body owner field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaActor {
    pub user_id: Option<i32>,
    pub is_admin: bool,
}

impl MediaActor {
    pub fn user(user_id: i32) -> Result<Self, MediaError> {
        if user_id <= 0 {
            return Err(MediaError::invalid("Invalid media owner"));
        }
        Ok(Self {
            user_id: Some(user_id),
            is_admin: false,
        })
    }

    pub fn admin(user_id: i32) -> Result<Self, MediaError> {
        if user_id <= 0 {
            return Err(MediaError::invalid("Invalid media owner"));
        }
        Ok(Self {
            user_id: Some(user_id),
            is_admin: true,
        })
    }

    pub fn site_operator(user_id: Option<i32>, is_admin: bool) -> Self {
        Self { user_id, is_admin }
    }
}

/// Server-constructed write context. Scope and owner come from the business path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaContext {
    pub actor: MediaActor,
    pub scope: MediaScope,
    pub source: MediaSource,
    pub producer_key: Option<String>,
}

impl MediaContext {
    pub fn site(actor: MediaActor, source: MediaSource) -> Self {
        Self {
            actor,
            scope: MediaScope::Site,
            source,
            producer_key: None,
        }
    }

    pub fn user(actor: MediaActor, source: MediaSource) -> Result<Self, MediaError> {
        let ctx = Self {
            actor,
            scope: MediaScope::User,
            source,
            producer_key: None,
        };
        ctx.validate()?;
        Ok(ctx)
    }

    pub fn with_producer_key(mut self, key: impl Into<String>) -> Self {
        let key = key.into();
        self.producer_key = (!key.is_empty()).then_some(key);
        self
    }

    pub fn owner_user_id(&self) -> Option<i32> {
        match self.scope {
            MediaScope::User => self.actor.user_id.filter(|id| *id > 0),
            MediaScope::Site | MediaScope::LegacyUnknown => None,
        }
    }

    pub fn created_by(&self) -> Option<i32> {
        self.actor.user_id.filter(|id| *id > 0)
    }

    pub fn validate(&self) -> Result<(), MediaError> {
        match self.scope {
            MediaScope::User => {
                if self.owner_user_id().is_none() {
                    Err(MediaError::invalid("User media requires an owner"))
                } else {
                    Ok(())
                }
            }
            MediaScope::Site | MediaScope::LegacyUnknown => Ok(()),
        }
    }
}

/// Owner for AI/task results. Never uses a worker identity.
pub fn task_media_context(subject_id: i32, owner_id: i32) -> MediaContext {
    if let Ok(actor) = MediaActor::user(subject_id) {
        if let Ok(ctx) = MediaContext::user(actor, MediaSource::Generated) {
            return ctx;
        }
    }
    let actor =
        MediaActor::admin(owner_id).unwrap_or_else(|_| MediaActor::site_operator(None, true));
    MediaContext::site(actor, MediaSource::Generated)
}

#[derive(Clone, Debug)]
pub struct NewMediaBytes {
    /// Storage only borrows this; `Bytes` lets multipart bodies pass through uncopied.
    pub bytes: axum::body::Bytes,
    pub claimed_mime: String,
    pub filename: String,
    pub max_bytes: usize,
    pub derived_from_id: Option<i32>,
    /// Workbench/journal stay public until the privacy stage lands.
    pub exposure: MediaExposure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaAsset {
    pub id: i32,
    pub public_id: Uuid,
    pub scope: MediaScope,
    pub owner_user_id: Option<i32>,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub source: MediaSource,
    pub state: MediaState,
    pub exposure: MediaExposure,
    pub kind: String,
    pub content_path: String,
    pub public_path: Option<String>,
    pub created_at: DateTime<Utc>,
    pub usage_count: i64,
    pub references_complete: bool,
    pub checksum_sha256: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub derived_from_id: Option<i32>,
    pub first_published_at: Option<DateTime<Utc>>,
}

impl MediaAsset {
    pub fn catalog_url(&self) -> String {
        self.public_path
            .clone()
            .unwrap_or_else(|| self.content_path.clone())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted,
    PendingRetry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryReport {
    pub claimed: u32,
    pub completed: u32,
    pub cleaned: u32,
}
