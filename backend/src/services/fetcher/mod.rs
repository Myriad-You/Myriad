//! Platform data fetching (Bilibili, Steam, GitHub, X, Discord, MAL, Xbox, PSN, YouTube, …).
//!
//! Real submodules: [`types`], platform impls, and [`x_share`] text helpers.

mod platforms_core;
mod platforms_extended;
mod types;
mod x_share;

pub use types::PlatformFetcher;
pub use x_share::{build_x_intent_url, compose_x_share_text, X_SHARE_DEFAULT_MAX_LEN};

// Re-export response types commonly used by API handlers.
pub use types::SteamUserInfo;

#[cfg(test)]
mod platform_fetcher_agent_tests {
    use super::PlatformFetcher;

    #[test]
    fn bangumi_user_agent_falls_back_when_missing_or_blank() {
        let fallback = PlatformFetcher::bangumi_user_agent(None);
        assert!(!fallback.is_empty());
        assert_eq!(PlatformFetcher::bangumi_user_agent(Some("")), fallback);
        assert_eq!(PlatformFetcher::bangumi_user_agent(Some("   ")), fallback);
        assert_eq!(
            PlatformFetcher::bangumi_user_agent(Some("MyriadBot/1.0")),
            "MyriadBot/1.0"
        );
    }
}
