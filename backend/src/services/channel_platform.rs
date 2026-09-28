//! The chat platforms a paired user can work through, as one closed set.
//!
//! Every per-platform table (storage namespaces, pairing provider, bot
//! configuration keys) is a method here with an exhaustive `match`: adding a
//! platform fails to compile until each table has its row, and an unknown
//! string can no longer fall through to QQ's namespaces. The strings are the
//! stored values, so no data changes.

use crate::config::DynamicConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChannelPlatform {
    Qq,
    Telegram,
    Discord,
    Feishu,
    OneBot,
}

impl ChannelPlatform {
    pub const ALL: [Self; 5] = [
        Self::Qq,
        Self::Telegram,
        Self::Discord,
        Self::Feishu,
        Self::OneBot,
    ];

    /// Platform slug used in session keys and logs.
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Qq => "qq",
            Self::Telegram => "telegram",
            Self::Discord => "discord",
            Self::Feishu => "feishu",
            Self::OneBot => "onebot",
        }
    }

    /// `user_identities.provider` of a pairing. Discord pairings are
    /// `discord_dm`: `discord` is the OAuth login / data-platform provider.
    /// OneBot is its own platform, not a second QQ transport: a QQ number
    /// reached through NapCat is a different identity from the official bot's.
    pub const fn provider(self) -> &'static str {
        match self {
            Self::Qq => "qq",
            Self::Telegram => "telegram",
            Self::Discord => "discord_dm",
            Self::Feishu => "feishu",
            Self::OneBot => "onebot",
        }
    }

    pub fn from_provider(provider: &str) -> Option<Self> {
        let provider = provider.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|platform| platform.provider() == provider)
    }

    pub const fn session_ns(self) -> &'static str {
        match self {
            Self::Qq => "qq_c2c_session",
            Self::Telegram => "telegram_dm_session",
            Self::Discord => "discord_dm_session",
            Self::Feishu => "feishu_p2p_session",
            Self::OneBot => "onebot_private_session",
        }
    }

    pub const fn pending_ns(self) -> &'static str {
        match self {
            Self::Qq => "qq_c2c_pending",
            Self::Telegram => "telegram_dm_pending",
            Self::Discord => "discord_dm_pending",
            Self::Feishu => "feishu_p2p_pending",
            Self::OneBot => "onebot_private_pending",
        }
    }

    pub const fn outbound_ns(self) -> &'static str {
        match self {
            Self::Qq => "qq_c2c_outbound",
            Self::Telegram => "telegram_dm_outbound",
            Self::Discord => "discord_dm_outbound",
            Self::Feishu => "feishu_p2p_outbound",
            Self::OneBot => "onebot_private_outbound",
        }
    }

    pub const fn inbound_ns(self) -> &'static str {
        match self {
            Self::Qq => "qq_c2c_msg",
            Self::Telegram => "telegram_dm_update",
            Self::Discord => "discord_dm_msg",
            Self::Feishu => "feishu_p2p_msg",
            Self::OneBot => "onebot_private_msg",
        }
    }

    pub const fn pairing_code_ns(self) -> &'static str {
        match self {
            Self::Qq => "qq_pairing_code",
            Self::Telegram => "telegram_pairing_code",
            Self::Discord => "discord_dm_pairing_code",
            Self::Feishu => "feishu_pairing_code",
            Self::OneBot => "onebot_pairing_code",
        }
    }

    /// `configurations` keys: (enabled, app id or "", secret).
    /// OneBot's second slot is its WebSocket URL, which is routing information
    /// rather than a credential: it is readable back, unlike the access token.
    pub const fn config_keys(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Qq => ("qq_bot_enabled", "qq_bot_app_id", "qq_bot_app_secret"),
            Self::Telegram => ("telegram_bot_enabled", "", "telegram_bot_token"),
            Self::Discord => ("discord_bot_enabled", "", "discord_bot_token"),
            Self::Feishu => (
                "feishu_bot_enabled",
                "feishu_bot_app_id",
                "feishu_bot_app_secret",
            ),
            Self::OneBot => (
                "onebot_bot_enabled",
                "onebot_bot_ws_url",
                "onebot_bot_access_token",
            ),
        }
    }

    pub fn enabled(self, config: &DynamicConfig) -> bool {
        match self {
            Self::Qq => config.qq_bot_enabled,
            Self::Telegram => config.telegram_bot_enabled,
            Self::Discord => config.discord_bot_enabled,
            Self::Feishu => config.feishu_bot_enabled,
            Self::OneBot => config.onebot_bot_enabled,
        }
    }

    /// (app id or "", secret) from the in-memory configuration.
    pub fn credentials(self, config: &DynamicConfig) -> (&str, Option<&str>) {
        match self {
            Self::Qq => (
                config.qq_bot_app_id.as_str(),
                config.qq_bot_app_secret.as_deref(),
            ),
            Self::Telegram => ("", config.telegram_bot_token.as_deref()),
            Self::Discord => ("", config.discord_bot_token.as_deref()),
            Self::Feishu => (
                config.feishu_bot_app_id.as_str(),
                config.feishu_bot_app_secret.as_deref(),
            ),
            Self::OneBot => (
                config.onebot_bot_ws_url.as_str(),
                config.onebot_bot_access_token.as_deref(),
            ),
        }
    }
}

impl std::fmt::Display for ChannelPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.slug())
    }
}

#[cfg(test)]
mod tests {
    use super::ChannelPlatform;

    #[test]
    fn providers_round_trip_and_unknown_is_none() {
        for platform in ChannelPlatform::ALL {
            assert_eq!(
                ChannelPlatform::from_provider(platform.provider()),
                Some(platform)
            );
        }
        assert_eq!(ChannelPlatform::from_provider("discord"), None);
        assert_eq!(ChannelPlatform::from_provider("slack"), None);
    }

    #[test]
    fn storage_namespaces_are_distinct_per_platform() {
        let mut seen = std::collections::HashSet::new();
        for platform in ChannelPlatform::ALL {
            for ns in [
                platform.session_ns(),
                platform.pending_ns(),
                platform.outbound_ns(),
                platform.inbound_ns(),
                platform.pairing_code_ns(),
            ] {
                assert!(seen.insert(ns), "{ns} reused");
            }
        }
    }
}
