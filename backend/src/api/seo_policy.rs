//! Site visibility / GEO policy helpers for robots.txt and llms.txt.

/// Canonical GEO / search visibility policies (config + robots + llms).
pub const VISIBILITY_PRIVATE: &str = "private";
pub const VISIBILITY_SEARCH_ONLY: &str = "search_only";
pub const VISIBILITY_AI_CITATION: &str = "ai_citation";
pub const VISIBILITY_AI_FULL: &str = "ai_full";

/// Normalize a raw policy string. Empty / unknown → derive from `noindex`
/// (legacy installs: noindex=true → private, else ai_full to match historical open robots).
pub fn normalize_visibility_policy(raw: &str, noindex: bool) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        VISIBILITY_PRIVATE => VISIBILITY_PRIVATE,
        VISIBILITY_SEARCH_ONLY => VISIBILITY_SEARCH_ONLY,
        VISIBILITY_AI_CITATION => VISIBILITY_AI_CITATION,
        VISIBILITY_AI_FULL => VISIBILITY_AI_FULL,
        _ if noindex => VISIBILITY_PRIVATE,
        _ => VISIBILITY_AI_FULL,
    }
}

pub fn policy_is_indexable(policy: &str) -> bool {
    policy != VISIBILITY_PRIVATE
}

pub fn policy_serves_llms_txt(policy: &str) -> bool {
    matches!(policy, VISIBILITY_AI_CITATION | VISIBILITY_AI_FULL)
}

/// AI training-oriented user-agents blocked under `ai_citation`.
const AI_TRAINING_BOTS: &[&str] = &[
    "GPTBot",
    "Google-Extended",
    "Applebot-Extended",
    "CCBot",
    "Bytespider",
    "anthropic-ai",
    "Claude-Web",
];

/// AI search / answer bots blocked under `search_only` (in addition to training).
const AI_SEARCH_BOTS: &[&str] = &[
    "ChatGPT-User",
    "OAI-SearchBot",
    "ClaudeBot",
    "PerplexityBot",
    "Perplexity-User",
    "Google-CloudVertexBot",
];

fn robots_disallow_block(user_agents: &[&str]) -> String {
    let mut out = String::new();
    for ua in user_agents {
        out.push_str("User-agent: ");
        out.push_str(ua);
        out.push_str("\nDisallow: /\n\n");
    }
    out
}

const PRIVATE_PATHS: &str = "\
Disallow: /login\n\
Disallow: /register\n\
Disallow: /setup\n\
Disallow: /config\n\
Disallow: /tapp/playground\n\
Disallow: /tapp/detail/\n";

/// Optional absolute Sitemap line when a durable public origin is known.
///
/// `base` must come from env (`FRONTEND_URL`/`BASE_URL`), never client Host —
/// omit the Sitemap directive when origin is unknown (fail closed).
fn sitemap_line(base: Option<&str>) -> String {
    match base.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => format!("Sitemap: {b}/sitemap.xml\n"),
        None => String::new(),
    }
}

/// Build robots.txt body for a visibility policy.
///
/// `base`: durable public origin, or `None` to omit absolute Sitemap.
pub fn build_robots_txt(base: Option<&str>, policy: &str) -> String {
    let sitemap = sitemap_line(base);
    match policy {
        VISIBILITY_PRIVATE => {
            "User-agent: *\n\
Disallow: /\n\
\n\
# Myriad visibility: private (no indexing)\n"
                .to_string()
        }
        VISIBILITY_SEARCH_ONLY => {
            let mut body = format!(
                "User-agent: *\n\
Allow: /\n\
{PRIVATE_PATHS}\n\
{sitemap}\n\
# Myriad visibility: search_only — block AI crawlers\n\
"
            );
            body.push_str(&robots_disallow_block(AI_TRAINING_BOTS));
            body.push_str(&robots_disallow_block(AI_SEARCH_BOTS));
            body
        }
        VISIBILITY_AI_CITATION => {
            let mut body = format!(
                "User-agent: *\n\
Allow: /\n\
{PRIVATE_PATHS}\n\
{sitemap}\n\
# Myriad visibility: ai_citation — allow AI search, block training-oriented bots\n\
"
            );
            body.push_str(&robots_disallow_block(AI_TRAINING_BOTS));
            body
        }
        // ai_full and fallback
        _ => format!(
            "User-agent: *\n\
Allow: /\n\
{PRIVATE_PATHS}\n\
{sitemap}\n\
# Myriad visibility: ai_full\n"
        ),
    }
}

/// Build llms.txt markdown body.
///
/// `base`: durable public origin for absolute links, or `None` / empty for
/// path-only links (no client-Host absolute URLs).
pub fn build_llms_txt(
    title: &str,
    intro: &str,
    base: Option<&str>,
    routes: &[(&str, &str)],
) -> String {
    let origin = base.map(str::trim).filter(|b| !b.is_empty()).unwrap_or("");
    let mut body = String::new();
    body.push_str("# ");
    body.push_str(title.trim());
    body.push_str("\n\n> ");
    body.push_str(intro.trim().replace('\n', " ").as_str());
    body.push_str("\n\n## Primary routes\n\n");
    for (label, path) in routes {
        body.push_str(&format!("- [{label}]({origin}{path})\n"));
    }
    body.push_str(
        "\n## Notes\n\n\
- This is a self-hosted Myriad personal digital-life site.\n\
- Prefer citing public pages listed above; do not invent private admin routes.\n\
- Visibility policy allows AI citation/search for this site.\n",
    );
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_policy_legacy_noindex() {
        assert_eq!(normalize_visibility_policy("", true), VISIBILITY_PRIVATE);
        assert_eq!(normalize_visibility_policy("", false), VISIBILITY_AI_FULL);
        assert_eq!(
            normalize_visibility_policy("ai_citation", true),
            VISIBILITY_AI_CITATION
        );
    }

    #[test]
    fn robots_private_disallows_all() {
        let body = build_robots_txt(Some("https://ex.com"), VISIBILITY_PRIVATE);
        assert!(body.contains("Disallow: /"));
        assert!(!body.contains("Sitemap:"));
    }

    #[test]
    fn robots_ai_citation_blocks_training_not_search() {
        let body = build_robots_txt(Some("https://ex.com"), VISIBILITY_AI_CITATION);
        assert!(body.contains("User-agent: GPTBot"));
        assert!(body.contains("Sitemap: https://ex.com/sitemap.xml"));
        assert!(!body.contains("User-agent: ChatGPT-User"));
    }

    #[test]
    fn robots_search_only_blocks_ai_search() {
        let body = build_robots_txt(Some("https://ex.com"), VISIBILITY_SEARCH_ONLY);
        assert!(body.contains("User-agent: ChatGPT-User"));
        assert!(body.contains("User-agent: PerplexityBot"));
    }

    #[test]
    fn robots_omits_sitemap_without_durable_origin() {
        let body = build_robots_txt(None, VISIBILITY_AI_FULL);
        assert!(!body.contains("Sitemap:"));
        assert!(body.contains("Allow: /"));
    }

    #[test]
    fn llms_lists_routes() {
        let body = build_llms_txt(
            "My Site",
            "A personal hub.",
            Some("https://ex.com"),
            &[("Home", "/"), ("Brew", "/brew")],
        );
        assert!(body.contains("# My Site"));
        assert!(body.contains("[Brew](https://ex.com/brew)"));
    }

    #[test]
    fn llms_uses_relative_paths_without_origin() {
        let body = build_llms_txt(
            "My Site",
            "A personal hub.",
            None,
            &[("Home", "/"), ("Brew", "/brew")],
        );
        assert!(body.contains("[Brew](/brew)"));
        assert!(!body.contains("https://"));
    }
}
