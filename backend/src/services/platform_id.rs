//! Core platform registry: the one list of platform ids, their aliases, display
//! names, and the single rule for "credentials present" / "enabled".
//!
//! `credentials_present` mirrors exactly what the fetch arms need to run
//! (`platform_refresh::arms_*`): trimmed values, and for Xbox / PSN the same
//! stored values the arms resolve through [`xbox_gamertag`] and friends.
//! Agent connection flags, report generate-all and the refresh gate all derive
//! from here, so they cannot drift apart again.

use crate::config::DynamicConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformId {
    Github,
    Bilibili,
    Steam,
    Youtube,
    Netease,
    Bangumi,
    X,
    Discord,
    Mal,
    Xbox,
    Psn,
}

/// Canonical slugs in [`PlatformId::ALL`] order (for `&[&str]` call sites and JSON schemas).
pub const PLATFORM_SLUGS: &[&str] = &PlatformId::SLUGS;

impl PlatformId {
    /// Canonical order (matches the default `platforms` seed order).
    pub const ALL: [PlatformId; 11] = [
        PlatformId::Github,
        PlatformId::Bilibili,
        PlatformId::Steam,
        PlatformId::Youtube,
        PlatformId::Netease,
        PlatformId::Bangumi,
        PlatformId::X,
        PlatformId::Discord,
        PlatformId::Mal,
        PlatformId::Xbox,
        PlatformId::Psn,
    ];

    const SLUGS: [&'static str; 11] = {
        let mut out = [""; 11];
        let mut i = 0;
        while i < out.len() {
            out[i] = Self::ALL[i].slug();
            i += 1;
        }
        out
    };

    /// Internal id: cache file names, JSON keys, API params.
    pub const fn slug(self) -> &'static str {
        match self {
            PlatformId::Github => "github",
            PlatformId::Bilibili => "bilibili",
            PlatformId::Steam => "steam",
            PlatformId::Youtube => "youtube",
            PlatformId::Netease => "netease",
            PlatformId::Bangumi => "bangumi",
            PlatformId::X => "x",
            PlatformId::Discord => "discord",
            PlatformId::Mal => "mal",
            PlatformId::Xbox => "xbox",
            PlatformId::Psn => "psn",
        }
    }

    /// Catalog display name (same as the `platforms` seed rows and the frontend labels).
    pub const fn display_name(self) -> &'static str {
        match self {
            PlatformId::Github => "GitHub",
            PlatformId::Bilibili => "Bilibili",
            PlatformId::Steam => "Steam",
            PlatformId::Youtube => "YouTube",
            PlatformId::Netease => "Netease Music",
            PlatformId::Bangumi => "Bangumi",
            PlatformId::X => "X",
            PlatformId::Discord => "Discord",
            PlatformId::Mal => "MyAnimeList",
            PlatformId::Xbox => "Xbox",
            PlatformId::Psn => "PlayStation",
        }
    }

    /// Exact canonical slug only (no aliases, case-sensitive).
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|id| id.slug() == slug)
    }

    /// Lenient parse: trims, ignores case, treats spaces / hyphens as `_`, and
    /// accepts the known aliases (display names, catalog `netease_music`, …).
    pub fn parse(name: &str) -> Option<Self> {
        let key = name.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        Self::ALL
            .into_iter()
            .find(|id| id.slug() == key || id.aliases().contains(&key.as_str()))
    }

    /// Accepted aliases, already in `parse`'s normalized form.
    pub const fn aliases(self) -> &'static [&'static str] {
        match self {
            PlatformId::Github => &[],
            PlatformId::Bilibili => &[],
            PlatformId::Steam => &[],
            PlatformId::Youtube => &["yt"],
            PlatformId::Netease => &["netease_music", "neteasecloud", "netease_cloud_music"],
            PlatformId::Bangumi => &["bgm"],
            PlatformId::X => &["twitter", "x_twitter"],
            PlatformId::Discord => &[],
            PlatformId::Mal => &["myanimelist", "my_anime_list"],
            PlatformId::Xbox => &[],
            PlatformId::Psn => &["playstation", "play_station", "playstation_network"],
        }
    }

    /// Credentials the fetch arm needs are all present (non-blank). Ignores `*_enabled`.
    pub fn credentials_present(self, config: &DynamicConfig) -> bool {
        let has = |v: &Option<String>| nonblank(v.as_deref()).is_some();
        match self {
            PlatformId::Github => has(&config.github_username),
            PlatformId::Bilibili => has(&config.bilibili_uid),
            PlatformId::Steam => has(&config.steam_api_key) && has(&config.steam_id),
            PlatformId::Youtube => has(&config.youtube_api_key) && has(&config.youtube_channel_id),
            PlatformId::Netease => has(&config.netease_user_id),
            PlatformId::Bangumi => {
                has(&config.bangumi_username) || has(&config.bangumi_access_token)
            }
            PlatformId::X => has(&config.x_username) && has(&config.x_bearer_token),
            PlatformId::Discord => has(&config.discord_access_token),
            PlatformId::Mal => has(&config.mal_username),
            PlatformId::Xbox => {
                xbox_gamertag(config).is_some() && openxbl_api_key(config).is_some()
            }
            PlatformId::Psn => psn_online_id(config).is_some() && psn_npsso(config).is_some(),
        }
    }

    /// Administrator's explicit report/agent switch (`None` = never set).
    pub fn explicit_enabled(self, config: &DynamicConfig) -> Option<bool> {
        match self {
            PlatformId::Github => config.github_enabled,
            PlatformId::Bilibili => config.bilibili_enabled,
            PlatformId::Steam => config.steam_enabled,
            PlatformId::Youtube => config.youtube_enabled,
            PlatformId::Netease => config.netease_enabled,
            PlatformId::Bangumi => config.bangumi_enabled,
            PlatformId::X => config.x_enabled,
            PlatformId::Discord => config.discord_enabled,
            PlatformId::Mal => config.mal_enabled,
            PlatformId::Xbox => config.xbox_enabled,
            PlatformId::Psn => config.psn_enabled,
        }
    }

    /// Enabled = explicit switch, else whether credentials are present.
    pub fn enabled(self, config: &DynamicConfig) -> bool {
        self.explicit_enabled(config)
            .unwrap_or_else(|| self.credentials_present(config))
    }
}

impl std::fmt::Display for PlatformId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.slug())
    }
}

fn nonblank(value: Option<&str>) -> Option<&str> {
    value.filter(|s| !s.trim().is_empty())
}

/// The stored value when it is not blank. Environment variables are first
/// values only (see `config_service::env_seed`).
fn stored(value: Option<&String>) -> Option<String> {
    nonblank(value.map(String::as_str)).map(str::to_string)
}

/// Agent `config.get` / `platform.connection` / `auth.status` 共用：平台是否按配置视为已接通。
///
/// 与报告一键生成、刷新闸门同源：[`PlatformId::enabled`]。
pub(crate) fn platform_configured_flags(
    config: &crate::config::DynamicConfig,
) -> Vec<(&'static str, bool)> {
    PlatformId::ALL
        .into_iter()
        .map(|id| (id.slug(), id.enabled(config)))
        .collect()
}

pub fn xbox_gamertag(config: &DynamicConfig) -> Option<String> {
    stored(config.xbox_gamertag.as_ref())
}

pub fn openxbl_api_key(config: &DynamicConfig) -> Option<String> {
    stored(config.openxbl_api_key.as_ref())
}

pub fn psn_online_id(config: &DynamicConfig) -> Option<String> {
    stored(config.psn_online_id.as_ref())
}

pub fn psn_npsso(config: &DynamicConfig) -> Option<String> {
    stored(config.psn_npsso.as_ref())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    fn set(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    /// Named config rows: empty strings, whitespace, partial credentials, explicit switches.
    fn config_table() -> Vec<(&'static str, DynamicConfig)> {
        let mut rows = Vec::new();
        rows.push(("default", DynamicConfig::default()));

        let mut empty = DynamicConfig::default();
        empty.github_username = set("");
        empty.bilibili_uid = set("");
        empty.steam_api_key = set("  ");
        empty.steam_id = set("");
        empty.youtube_api_key = set("");
        empty.youtube_channel_id = set(" ");
        empty.netease_user_id = set("");
        empty.bangumi_username = set("");
        empty.bangumi_access_token = set("\t");
        empty.x_username = set("");
        empty.x_bearer_token = set("");
        empty.discord_access_token = set(" ");
        empty.mal_username = set("");
        empty.xbox_gamertag = set("");
        empty.openxbl_api_key = set(" ");
        empty.psn_online_id = set("");
        empty.psn_npsso = set("");
        rows.push(("empty strings", empty));

        let mut partial = DynamicConfig::default();
        partial.steam_api_key = set("key");
        partial.youtube_api_key = set("key");
        partial.x_username = set("me");
        partial.xbox_gamertag = set("tag");
        partial.psn_npsso = set("npsso");
        rows.push(("partial credentials", partial));

        let mut full = DynamicConfig::default();
        full.github_username = set("octocat");
        full.bilibili_uid = set("1");
        full.steam_api_key = set("key");
        full.steam_id = set("7656");
        full.youtube_api_key = set("key");
        full.youtube_channel_id = set("UC1");
        full.netease_user_id = set("2");
        full.bangumi_access_token = set("token");
        full.x_username = set("me");
        full.x_bearer_token = set("bearer");
        full.discord_access_token = set("token");
        full.mal_username = set("me");
        full.xbox_gamertag = set("tag");
        full.openxbl_api_key = set("key");
        full.psn_online_id = set("me");
        full.psn_npsso = set("npsso");
        rows.push(("full credentials", full.clone()));

        let mut switched = full;
        switched.github_enabled = Some(false);
        switched.steam_enabled = Some(false);
        switched.psn_enabled = Some(false);
        switched.mal_username = None;
        switched.mal_enabled = Some(true);
        rows.push(("explicit switches", switched));

        let mut id_only = DynamicConfig::default();
        id_only.psn_online_id = set("me");
        rows.push(("an id without its secret", id_only));
        rows
    }

    /// The three former sources of truth (refresh gate, Agent flags, report generate-all)
    /// must agree for every platform on every row.
    #[test]
    fn refresh_gate_agent_flags_and_report_enabled_agree() {
        for (name, config) in config_table() {
            {
                let flags: HashMap<&str, bool> =
                    platform_configured_flags(&config).into_iter().collect();
                let report = crate::api::reports::enabled_report_platforms(&config);
                let fetchable_ids =
                    crate::services::platform_refresh::configured_platform_ids(&config);
                let cards = crate::api::config::public_platform_cards(Some(&config));
                assert_eq!(cards.len(), PlatformId::ALL.len(), "{name}");
                let public: HashMap<PlatformId, bool> = cards
                    .iter()
                    .map(|card| {
                        (
                            PlatformId::parse(&card.name).expect(&card.name),
                            card.enabled,
                        )
                    })
                    .collect();
                // The settings page's switch is the runtime one.
                let admin: HashMap<PlatformId, bool> =
                    crate::api::config::admin_platform_enabled(&config)
                        .into_iter()
                        .collect();
                assert_eq!(flags.len(), PlatformId::ALL.len(), "{name}");
                for id in PlatformId::ALL {
                    let slug = id.slug();
                    let fetchable = fetchable_ids.contains(&slug);
                    assert_eq!(fetchable, id.credentials_present(&config), "{name}: {slug}");
                    assert_eq!(flags[slug], id.enabled(&config), "{name}: {slug}");
                    assert_eq!(public[&id], id.enabled(&config), "{name}: public {slug}");
                    assert_eq!(admin[&id], id.enabled(&config), "{name}: admin {slug}");
                    assert_eq!(
                        report.iter().any(|p| p == slug),
                        id.enabled(&config),
                        "{name}: {slug}"
                    );
                    if id.explicit_enabled(&config).is_none() {
                        assert_eq!(flags[slug], fetchable, "{name}: {slug}");
                    }
                }
            }
        }
    }

    #[test]
    fn credential_table_expectations() {
        let rows: HashMap<_, _> = config_table().into_iter().collect();
        let present = |row: &str| -> Vec<&'static str> {
            PlatformId::ALL
                .into_iter()
                .filter(|id| id.credentials_present(&rows[row]))
                .map(PlatformId::slug)
                .collect()
        };
        assert!(present("default").is_empty());
        assert!(present("empty strings").is_empty());
        assert!(present("partial credentials").is_empty());
        assert_eq!(present("full credentials"), PLATFORM_SLUGS.to_vec());
        assert!(present("an id without its secret").is_empty());

        let switched = &rows["explicit switches"];
        assert!(!PlatformId::Github.enabled(switched));
        assert!(!PlatformId::Steam.enabled(switched));
        assert!(PlatformId::Mal.enabled(switched));
        assert!(!PlatformId::Mal.credentials_present(switched));
        assert!(PlatformId::Bilibili.enabled(switched));
    }

    #[test]
    fn console_resolvers_return_the_value_the_fetch_arm_uses() {
        let mut config = DynamicConfig::default();
        assert_eq!(openxbl_api_key(&config), None);
        config.openxbl_api_key = Some("db".into());
        assert_eq!(openxbl_api_key(&config).as_deref(), Some("db"));
        config.openxbl_api_key = Some("  ".into());
        assert_eq!(openxbl_api_key(&config), None, "blank is missing");
    }

    #[test]
    fn parse_round_trips_slugs_display_names_and_aliases() {
        for id in PlatformId::ALL {
            assert_eq!(PlatformId::from_slug(id.slug()), Some(id));
            assert_eq!(PlatformId::parse(id.slug()), Some(id));
            assert_eq!(PlatformId::parse(&id.slug().to_uppercase()), Some(id));
            assert_eq!(PlatformId::parse(&format!("  {}  ", id.slug())), Some(id));
            assert_eq!(PlatformId::parse(id.display_name()), Some(id), "{id}");
            assert_eq!(id.to_string(), id.slug());
            for alias in id.aliases() {
                assert_eq!(PlatformId::parse(alias), Some(id), "{alias}");
                assert_eq!(PlatformId::from_slug(alias), None, "{alias}");
            }
        }
        // Former alias sets (platform_auto_refresh + api/cache + platform_test labels).
        for (raw, slug) in [
            ("yt", "youtube"),
            ("netease music", "netease"),
            ("netease_music", "netease"),
            ("netease-music", "netease"),
            ("neteasecloud", "netease"),
            ("Netease", "netease"),
            ("twitter", "x"),
            ("x_twitter", "x"),
            ("MyAnimeList", "mal"),
            ("my_anime_list", "mal"),
            ("PSN", "psn"),
            ("PlayStation", "psn"),
            ("play_station", "psn"),
            ("playstation network", "psn"),
            ("bgm", "bangumi"),
        ] {
            assert_eq!(
                PlatformId::parse(raw).map(PlatformId::slug),
                Some(slug),
                "{raw}"
            );
        }
        assert_eq!(PlatformId::parse("unknown"), None);
        assert_eq!(PlatformId::parse(""), None);
        assert_eq!(PlatformId::from_slug("PSN"), None);

        // No alias collides with another platform's slug or alias.
        let mut seen = std::collections::HashSet::new();
        for id in PlatformId::ALL {
            assert!(seen.insert(id.slug()));
            for alias in id.aliases() {
                assert!(seen.insert(alias), "duplicate alias {alias}");
            }
        }
    }

    #[test]
    fn default_platform_seeds_match_the_registry() {
        let seeds = crate::db::schema_check::default_platform_seeds();
        assert_eq!(seeds.len(), PlatformId::ALL.len());
        for (seed, id) in seeds.iter().zip(PlatformId::ALL) {
            assert_eq!(PlatformId::parse(seed.name), Some(id), "{}", seed.name);
            assert_eq!(seed.display_name, id.display_name());
        }
    }

    /// `"slug"` used as a list element (`"slug",` / `"slug"]` / `"slug".to_string(),`),
    /// as opposed to a per-platform match arm (`"slug" =>` / `"slug" |`) or the key
    /// of a per-platform tuple row (`("slug", …)`).
    fn lists_slug(text: &str, slug: &str) -> bool {
        let needle = format!("\"{slug}\"");
        text.match_indices(&needle).any(|(at, _)| {
            if text[..at].trim_end().ends_with('(') {
                return false;
            }
            let rest = &text[at + needle.len()..];
            let rest = rest.strip_prefix(".to_string()").unwrap_or(rest);
            matches!(rest.trim_start().chars().next(), Some(',' | ']'))
        })
    }

    /// Platform slug lists live here only. A file listing (nearly) every slug as
    /// string literals within a few lines is a second registry in disguise.
    #[test]
    fn no_other_file_hardcodes_the_platform_slug_list() {
        // Different namespaces, not the core platform id list.
        const ALLOWED: &[(&str, &str)] = &[
            (
                "services/library_items.rs",
                "persisted library platform names (Steam, Netease, …), not slugs",
            ),
            (
                "db/schema_check/seeds.rs",
                "DB seed rows; checked against the registry above",
            ),
        ];
        const WINDOW: usize = 14;
        const THRESHOLD: usize = 8;

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read src dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let rel = path
                    .strip_prefix(&root)
                    .expect("under src")
                    .to_string_lossy()
                    .replace('\\', "/");
                if rel == "services/platform_id.rs" || ALLOWED.iter().any(|(f, _)| *f == rel) {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("read source");
                let lines: Vec<&str> = text.lines().collect();
                for start in 0..lines.len() {
                    let window = lines[start..(start + WINDOW).min(lines.len())].join("\n");
                    let hits = PLATFORM_SLUGS
                        .iter()
                        .filter(|slug| lists_slug(&window, slug))
                        .count();
                    if hits >= THRESHOLD {
                        offenders.push(format!("{rel}:{}", start + 1));
                        break;
                    }
                }
            }
        }
        offenders.sort();
        assert!(
            offenders.is_empty(),
            "platform slug lists outside services/platform_id.rs (use PlatformId::ALL / PLATFORM_SLUGS): {offenders:?}"
        );
    }
}
