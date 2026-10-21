//! Report writing styles. Only the site owner edits them, and a style only
//! shapes the prose: who speaks, tone, length, chapter order, phrases to avoid.
//! Colors and fonts are not a style's business. The prompt's rules, checks and
//! omit list come after the style block and cannot be overridden by it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Configuration key: the owner's styles and which one is current.
pub(crate) const REPORT_STYLES_KEY: &str = "report_styles";

pub(crate) const DEFAULT_STYLE_ID: &str = "default";
pub(crate) const MAX_CUSTOM_STYLES: usize = 12;
pub(crate) const MAX_NAME_CHARS: usize = 24;
pub(crate) const MAX_TONE_CHARS: usize = 200;
pub(crate) const MAX_SAMPLE_CHARS: usize = 300;
pub(crate) const MAX_AVOID_ITEMS: usize = 20;
pub(crate) const MAX_AVOID_CHARS: usize = 30;
const MAX_ID_CHARS: usize = 32;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Voice {
    /// An observer describes the owner.
    #[default]
    Narrator,
    /// The owner speaks about themself, in the first person.
    Owner,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Length {
    Short,
    #[default]
    Standard,
    Long,
}

/// How long the script may run under one [`Length`]. Every length allows a
/// one-beat chapter: thin data is never padded to reach a count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LengthBudget {
    pub headline_chars: usize,
    pub beat_chars: usize,
    pub min_beats: usize,
    pub max_beats: usize,
}

impl Length {
    pub(crate) fn budget(self) -> LengthBudget {
        match self {
            Self::Short => LengthBudget {
                headline_chars: 20,
                beat_chars: 40,
                min_beats: 1,
                max_beats: 2,
            },
            Self::Standard => LengthBudget {
                headline_chars: 30,
                beat_chars: 60,
                min_beats: 1,
                max_beats: 3,
            },
            Self::Long => LengthBudget {
                headline_chars: 40,
                beat_chars: 90,
                min_beats: 1,
                max_beats: 4,
            },
        }
    }
}

/// Chapter keys a style may order. Fixed so the stage can lay out by key.
pub(crate) const CHAPTER_KEYS: &[(&str, &str)] = &[
    ("portrait", "who this is, in one stroke"),
    ("taste", "what they like, with real names"),
    ("rhythm", "pace: how often, how deep, how long"),
    ("highlight", "the one thing that stands out"),
    ("contrast", "what does not fit the rest"),
    ("closing", "a short last line, no recap"),
];

pub(crate) fn chapter_meaning(key: &str) -> Option<&'static str> {
    CHAPTER_KEYS
        .iter()
        .find(|(known, _)| *known == key)
        .map(|(_, meaning)| *meaning)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportStyle {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub voice: Voice,
    /// Free text, ≤200 chars. Empty: follow each platform's own temperament.
    #[serde(default)]
    pub tone: String,
    #[serde(default)]
    pub length: Length,
    /// Ordered chapter keys from [`CHAPTER_KEYS`].
    #[serde(default)]
    pub chapters: Vec<String>,
    /// Phrases the prose must not use, ≤20.
    #[serde(default)]
    pub avoid: Vec<String>,
    /// A sample of the wanted voice, ≤300 chars. Read for rhythm, never copied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<String>,
}

fn builtin(
    id: &str,
    name: &str,
    voice: Voice,
    tone: &str,
    length: Length,
    chapters: &[&str],
) -> ReportStyle {
    ReportStyle {
        id: id.to_string(),
        name: name.to_string(),
        voice,
        tone: tone.to_string(),
        length,
        chapters: chapters.iter().map(|s| s.to_string()).collect(),
        avoid: Vec::new(),
        sample: None,
    }
}

/// Built-in presets, read-only. `default` keeps each platform's own critic voice.
pub(crate) fn builtin_styles() -> Vec<ReportStyle> {
    vec![
        builtin(
            DEFAULT_STYLE_ID,
            "默认",
            Voice::Narrator,
            "",
            Length::Standard,
            &["portrait", "taste", "highlight"],
        ),
        builtin(
            "plain",
            "平实",
            Voice::Narrator,
            "Plain and factual, like a museum caption. No jokes, no exclamation marks, no metaphors.",
            Length::Standard,
            &["portrait", "taste", "rhythm"],
        ),
        builtin(
            "diary",
            "自述",
            Voice::Owner,
            "Relaxed, like a short note the owner leaves for visitors. Honest about gaps; no bragging.",
            Length::Standard,
            &["portrait", "rhythm", "highlight", "closing"],
        ),
        builtin(
            "sketch",
            "速写",
            Voice::Narrator,
            "Quick sketch: punchy short lines, one idea each, a little wry.",
            Length::Short,
            &["portrait", "highlight"],
        ),
    ]
}

pub(crate) fn is_builtin_id(id: &str) -> bool {
    builtin_styles().iter().any(|style| style.id == id)
}

/// Stored under [`REPORT_STYLES_KEY`]: the owner's own styles and the current id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportStylesConfig {
    #[serde(default = "default_current")]
    pub current: String,
    #[serde(default)]
    pub custom: Vec<ReportStyle>,
}

fn default_current() -> String {
    DEFAULT_STYLE_ID.to_string()
}

impl Default for ReportStylesConfig {
    fn default() -> Self {
        Self {
            current: default_current(),
            custom: Vec::new(),
        }
    }
}

/// Why a style was refused on save. The code goes to the client as is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StyleRejection {
    pub code: &'static str,
    pub detail: String,
}

fn reject(code: &'static str, detail: impl Into<String>) -> StyleRejection {
    StyleRejection {
        code,
        detail: detail.into(),
    }
}

fn clip(text: &str, max: usize) -> String {
    text.trim()
        .chars()
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= MAX_ID_CHARS
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

impl ReportStyle {
    /// Lenient shape for reads and restores: trim and clip every field, drop
    /// unknown or repeated chapter keys, fall back to the default chapters.
    pub(crate) fn normalized(mut self) -> Self {
        self.id = self.id.trim().to_ascii_lowercase();
        self.name = clip(&self.name, MAX_NAME_CHARS);
        self.tone = clip(&self.tone, MAX_TONE_CHARS);
        let mut seen = std::collections::HashSet::new();
        self.chapters = self
            .chapters
            .into_iter()
            .map(|key| key.trim().to_ascii_lowercase())
            .filter(|key| chapter_meaning(key).is_some() && seen.insert(key.clone()))
            .collect();
        if self.chapters.is_empty() {
            self.chapters = builtin_styles()[0].chapters.clone();
        }
        let mut seen = std::collections::HashSet::new();
        self.avoid = self
            .avoid
            .into_iter()
            .map(|phrase| clip(&phrase, MAX_AVOID_CHARS))
            .filter(|phrase| !phrase.is_empty() && seen.insert(phrase.clone()))
            .take(MAX_AVOID_ITEMS)
            .collect();
        self.sample = self
            .sample
            .map(|sample| clip(&sample, MAX_SAMPLE_CHARS))
            .filter(|sample| !sample.is_empty());
        self
    }

    /// Strict check for a save: say what is wrong instead of clipping it.
    pub(crate) fn validate(&self) -> Result<(), StyleRejection> {
        if !valid_id(self.id.trim()) {
            return Err(reject(
                "report_style_invalid_id",
                "id must be 1-32 lowercase letters, digits, - or _",
            ));
        }
        if is_builtin_id(self.id.trim()) {
            return Err(reject(
                "report_style_builtin_readonly",
                format!("{} is a built-in style", self.id.trim()),
            ));
        }
        let name = self.name.trim().chars().count();
        if name == 0 || name > MAX_NAME_CHARS {
            return Err(reject(
                "report_style_invalid",
                "name must be 1-24 characters",
            ));
        }
        if self.tone.trim().chars().count() > MAX_TONE_CHARS {
            return Err(reject(
                "report_style_too_long",
                "tone is over 200 characters",
            ));
        }
        if self
            .sample
            .as_deref()
            .is_some_and(|s| s.trim().chars().count() > MAX_SAMPLE_CHARS)
        {
            return Err(reject(
                "report_style_too_long",
                "sample is over 300 characters",
            ));
        }
        if self.avoid.len() > MAX_AVOID_ITEMS {
            return Err(reject(
                "report_style_too_long",
                "avoid has more than 20 phrases",
            ));
        }
        if self
            .avoid
            .iter()
            .any(|phrase| phrase.trim().chars().count() > MAX_AVOID_CHARS)
        {
            return Err(reject(
                "report_style_too_long",
                "an avoid phrase is over 30 characters",
            ));
        }
        if self.chapters.is_empty() || self.chapters.len() > CHAPTER_KEYS.len() {
            return Err(reject(
                "report_style_invalid",
                "chapters must list 1-6 chapter keys",
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for key in &self.chapters {
            if chapter_meaning(key.trim()).is_none() {
                return Err(reject(
                    "report_style_invalid",
                    format!("unknown chapter key {}", key.trim()),
                ));
            }
            if !seen.insert(key.trim()) {
                return Err(reject(
                    "report_style_invalid",
                    format!("chapter key {} repeats", key.trim()),
                ));
            }
        }
        Ok(())
    }

    /// Stable hash of everything that shapes the prose. A report carries the
    /// hash it was written with; a mismatch marks it stale.
    pub(crate) fn hash(&self) -> String {
        let canonical = serde_json::to_vec(&serde_json::json!({
            "voice": self.voice,
            "tone": self.tone,
            "length": self.length,
            "chapters": self.chapters,
            "avoid": self.avoid,
            "sample": self.sample,
        }))
        .unwrap_or_default();
        let digest = Sha256::digest(&canonical);
        digest[..8].iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl ReportStylesConfig {
    /// Lenient: keep readable custom styles, drop the rest, point `current`
    /// at an existing style.
    pub(crate) fn normalized(self) -> Self {
        let mut ids = std::collections::HashSet::new();
        let custom: Vec<ReportStyle> = self
            .custom
            .into_iter()
            .map(ReportStyle::normalized)
            .filter(|style| {
                valid_id(&style.id)
                    && !is_builtin_id(&style.id)
                    && !style.name.is_empty()
                    && ids.insert(style.id.clone())
            })
            .take(MAX_CUSTOM_STYLES)
            .collect();
        let current = self.current.trim().to_ascii_lowercase();
        let current = if is_builtin_id(&current) || custom.iter().any(|s| s.id == current) {
            current
        } else {
            default_current()
        };
        Self { current, custom }
    }

    pub(crate) fn validate(&self) -> Result<(), StyleRejection> {
        if self.custom.len() > MAX_CUSTOM_STYLES {
            return Err(reject(
                "report_style_limit",
                format!("at most {MAX_CUSTOM_STYLES} custom styles"),
            ));
        }
        let mut ids = std::collections::HashSet::new();
        for style in &self.custom {
            style.validate()?;
            if !ids.insert(style.id.trim().to_ascii_lowercase()) {
                return Err(reject(
                    "report_style_invalid_id",
                    format!("style id {} repeats", style.id.trim()),
                ));
            }
        }
        let current = self.current.trim().to_ascii_lowercase();
        if !is_builtin_id(&current) && !ids.contains(&current) {
            return Err(reject(
                "report_style_not_found",
                format!("no style {current}"),
            ));
        }
        Ok(())
    }

    pub(crate) fn find(&self, id: &str) -> Option<ReportStyle> {
        builtin_styles()
            .into_iter()
            .chain(self.custom.iter().cloned())
            .find(|style| style.id == id)
    }

    /// The style new reports are written in.
    pub(crate) fn current_style(&self) -> ReportStyle {
        self.find(&self.current)
            .unwrap_or_else(|| builtin_styles().remove(0))
    }

    /// Hash of the whole stored config: any change to any style or to the
    /// current id moves it. A save names the revision it was edited from.
    pub(crate) fn revision(&self) -> String {
        let canonical = serde_json::to_vec(self).unwrap_or_default();
        let digest = Sha256::digest(&canonical);
        digest[..8].iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl ReportStylesConfig {
    /// Read a stored value entry by entry: a custom style that does not parse
    /// is dropped alone, the others survive. Only a value that is not a styles
    /// object at all (not an object, `custom` not a list) is refused.
    pub(crate) fn from_stored(value: Value) -> Result<Self, String> {
        let Value::Object(mut map) = value else {
            return Err("report styles must be an object".to_string());
        };
        let current = match map.remove("current") {
            None | Some(Value::Null) => default_current(),
            Some(Value::String(id)) => id,
            Some(_) => return Err("current must be a style id".to_string()),
        };
        let custom = match map.remove("custom") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(entries)) => entries
                .into_iter()
                .filter_map(|entry| match serde_json::from_value::<ReportStyle>(entry) {
                    Ok(style) => Some(style),
                    Err(error) => {
                        tracing::warn!(%error, "dropping an unreadable report style");
                        None
                    }
                })
                .collect(),
            Some(_) => return Err("custom must be a list".to_string()),
        };
        Ok(Self { current, custom }.normalized())
    }
}

/// Parse a stored `report_styles` value. A value that is not a styles object
/// reads as the default, so a bad row never blocks report generation.
pub(crate) fn parse_stored_styles(value: Value) -> ReportStylesConfig {
    ReportStylesConfig::from_stored(value).unwrap_or_else(|error| {
        tracing::warn!(%error, "invalid report styles, using defaults");
        ReportStylesConfig::default()
    })
}

pub(crate) async fn load_report_styles(db: &sea_orm::DatabaseConnection) -> ReportStylesConfig {
    stored_styles(
        crate::services::config_service::ConfigService::load_values_on(db, &[REPORT_STYLES_KEY])
            .await,
    )
}

/// Styles out of a [`ConfigService::load_values_on`] result that asked for
/// [`REPORT_STYLES_KEY`]; a failed read is the default.
///
/// [`ConfigService::load_values_on`]: crate::services::config_service::ConfigService::load_values_on
pub(crate) fn stored_styles(
    loaded: anyhow::Result<std::collections::HashMap<String, Value>>,
) -> ReportStylesConfig {
    match loaded {
        Ok(mut values) => values
            .remove(REPORT_STYLES_KEY)
            .map(parse_stored_styles)
            .unwrap_or_default(),
        Err(error) => {
            tracing::warn!(%error, "failed to load report styles");
            ReportStylesConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn custom(id: &str) -> ReportStyle {
        ReportStyle {
            id: id.into(),
            name: "我的".into(),
            voice: Voice::Owner,
            tone: "slow".into(),
            length: Length::Long,
            chapters: vec!["taste".into(), "closing".into()],
            avoid: vec!["宝藏".into()],
            sample: Some("今天也在听歌。".into()),
        }
    }

    #[test]
    fn stored_styles_read_back_exactly_as_saved() {
        let saved = ReportStylesConfig {
            current: "mine".into(),
            custom: vec![custom("mine")],
        };
        saved.validate().expect("valid");
        let row = serde_json::to_value(&saved).unwrap();
        assert_eq!(parse_stored_styles(row), saved);
        assert_eq!(saved.current_style().id, "mine");
    }

    #[test]
    fn a_missing_or_broken_row_reads_as_the_default() {
        assert_eq!(
            parse_stored_styles(json!({})),
            ReportStylesConfig::default()
        );
        assert_eq!(
            parse_stored_styles(json!("nope")),
            ReportStylesConfig::default()
        );
        let dangling = parse_stored_styles(json!({ "current": "gone", "custom": [] }));
        assert_eq!(dangling.current, DEFAULT_STYLE_ID);
        assert_eq!(dangling.current_style(), builtin_styles()[0]);
    }

    #[test]
    fn reads_clip_and_drop_what_a_save_would_refuse() {
        let mut long = custom("mine");
        long.tone = "x".repeat(400);
        long.chapters = vec!["taste".into(), "taste".into(), "bogus".into()];
        long.avoid = (0..30).map(|i| format!("p{i}")).collect();
        let builtin_clash = custom("default");
        let read = parse_stored_styles(json!({
            "current": "mine",
            "custom": [long, builtin_clash, custom("mine")]
        }));
        assert_eq!(read.custom.len(), 1);
        let style = &read.custom[0];
        assert_eq!(style.tone.chars().count(), 200);
        assert_eq!(style.chapters, vec!["taste".to_string()]);
        assert_eq!(style.avoid.len(), 20);
    }

    #[test]
    fn one_unreadable_style_does_not_take_the_others_with_it() {
        let read = parse_stored_styles(json!({
            "current": "mine",
            "custom": [{ "id": "bad", "voice": "shouting" }, custom("mine"), 7]
        }));
        assert_eq!(read.custom, vec![custom("mine")]);
        assert_eq!(read.current, "mine");
    }

    #[test]
    fn saves_say_what_is_wrong() {
        let mut bad = custom("mine");
        bad.tone = "x".repeat(201);
        assert_eq!(bad.validate().unwrap_err().code, "report_style_too_long");
        assert_eq!(
            custom("default").validate().unwrap_err().code,
            "report_style_builtin_readonly"
        );
        assert_eq!(
            custom("Has Space").validate().unwrap_err().code,
            "report_style_invalid_id"
        );
        let mut chapters = custom("mine");
        chapters.chapters = vec!["epilogue".into()];
        assert_eq!(
            chapters.validate().unwrap_err().code,
            "report_style_invalid"
        );
        let missing = ReportStylesConfig {
            current: "ghost".into(),
            custom: vec![],
        };
        assert_eq!(
            missing.validate().unwrap_err().code,
            "report_style_not_found"
        );
        let twice = ReportStylesConfig {
            current: "mine".into(),
            custom: vec![custom("mine"), custom("mine")],
        };
        assert_eq!(
            twice.validate().unwrap_err().code,
            "report_style_invalid_id"
        );
    }

    #[test]
    fn builtins_are_valid_shapes_and_default_comes_first() {
        let builtins = builtin_styles();
        assert_eq!(builtins[0].id, DEFAULT_STYLE_ID);
        assert!(
            builtins[0].tone.is_empty(),
            "default keeps the platform voice"
        );
        assert!((3..=4).contains(&builtins.len()));
        for style in builtins {
            assert_eq!(
                style.clone().normalized(),
                style,
                "{} is not normal",
                style.id
            );
            assert!(valid_id(&style.id));
        }
    }

    #[test]
    fn the_revision_moves_with_any_change_to_the_config() {
        let base = ReportStylesConfig {
            current: "mine".into(),
            custom: vec![custom("mine")],
        };
        let same = parse_stored_styles(serde_json::to_value(&base).unwrap());
        assert_eq!(base.revision(), same.revision());
        let mut renamed = base.clone();
        renamed.custom[0].name = "改名".into();
        assert_ne!(base.revision(), renamed.revision());
        let mut moved = base.clone();
        moved.current = DEFAULT_STYLE_ID.into();
        assert_ne!(base.revision(), moved.revision());
        assert_eq!(base.revision().len(), 16);
    }

    #[test]
    fn the_hash_follows_prose_settings_not_the_name() {
        let a = custom("mine");
        let mut renamed = a.clone();
        renamed.name = "别的名字".into();
        renamed.id = "other".into();
        assert_eq!(a.hash(), renamed.hash());
        let mut toned = a.clone();
        toned.tone = "fast".into();
        assert_ne!(a.hash(), toned.hash());
        assert_eq!(a.hash().len(), 16);
    }
}
