//! The quote (一言) sources the site knows: their ids, built-in hosts and the
//! default address. The quote card's proxy and the agent's `hitokoto.get`
//! both use it.

/// Config UI source ids — keep in sync with frontend `BUILTIN_HITOKOTO_SOURCES`
/// (+ `"custom"`) in `frontend/src/utils/quote.ts`.
pub const HITOKOTO_SOURCE_IDS: [&str; 5] = [
    "hitokoto-cn",
    "hitokoto-anime",
    "quotable-en",
    "meigen-ja",
    "custom",
];

/// Builtin quote API hosts (no port) matching FE `BUILTIN_HITOKOTO_SOURCES` URLs.
/// Proxy SSRF policy is still `outbound_security`; this list is catalog alignment.
pub const HITOKOTO_BUILTIN_HOSTS: [&str; 3] =
    ["v1.hitokoto.cn", "api.quotable.io", "meigen.doodlenote.net"];

/// Default literary-category URL on the first builtin host (FE `hitokoto-cn`).
pub fn default_hitokoto_url() -> String {
    format!(
        "https://{}/?c=d&c=i&c=k&encode=json",
        HITOKOTO_BUILTIN_HOSTS[0]
    )
}
