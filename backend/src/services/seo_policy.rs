//! Site visibility / GEO policy helpers for robots.txt and llms.txt.

/// Canonical GEO / search visibility policies (config + robots + llms).
pub const VISIBILITY_PRIVATE: &str = "private";
pub const VISIBILITY_SEARCH_ONLY: &str = "search_only";
pub const VISIBILITY_AI_CITATION: &str = "ai_citation";
pub const VISIBILITY_AI_FULL: &str = "ai_full";

/// Normalize a raw policy string. Empty / unknown → `noindex=true` → private, else ai_full.
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

/// How often Agent should inspect public SEO/GEO copy. Not a visibility policy.
pub const SEO_REVIEW_OFF: &str = "off";
pub const SEO_REVIEW_DAILY: &str = "daily";
pub const SEO_REVIEW_WEEKLY: &str = "weekly";

/// Empty / unknown → off.
pub fn normalize_seo_review_cadence(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        SEO_REVIEW_DAILY => SEO_REVIEW_DAILY,
        SEO_REVIEW_WEEKLY => SEO_REVIEW_WEEKLY,
        _ => SEO_REVIEW_OFF,
    }
}

/// Cron for a cadence, in server local time. `None` means do not run.
pub fn seo_review_cron(cadence: &str) -> Option<&'static str> {
    match normalize_seo_review_cadence(cadence) {
        SEO_REVIEW_DAILY => Some("0 9 * * *"),
        SEO_REVIEW_WEEKLY => Some("0 9 * * 1"),
        _ => None,
    }
}

/// AI training-oriented user-agents blocked under `ai_citation`.
/// Best-effort public UA names; crawlers not in this list are outside our control.
const AI_TRAINING_BOTS: &[&str] = &[
    "GPTBot",
    "Google-Extended",
    "Applebot-Extended",
    "CCBot",
    "Bytespider",
    "anthropic-ai",
    "Claude-Web",
    "Meta-ExternalAgent",
    "cohere-ai",
    "AI2Bot",
];

/// AI search / answer bots blocked under `search_only` (in addition to training).
const AI_SEARCH_BOTS: &[&str] = &[
    "ChatGPT-User",
    "OAI-SearchBot",
    "ClaudeBot",
    "PerplexityBot",
    "Perplexity-User",
    "Google-CloudVertexBot",
    "Meta-ExternalFetcher",
    "DuckAssistBot",
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
Disallow: /tapp/detail/\n\
Disallow: /journal/starred\n\
Disallow: /journal/workbench\n\
Disallow: /agent/settings\n";

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
        VISIBILITY_PRIVATE => "User-agent: *\n\
Disallow: /\n\
\n\
# Myriad visibility: private (no indexing)\n"
            .to_string(),
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

/// Append a markdown link list. `items` are (label, url). Empty list is a no-op.
pub fn append_llms_link_section(body: &mut String, heading: &str, items: &[(String, String)]) {
    if items.is_empty() {
        return;
    }
    body.push_str("\n## ");
    body.push_str(heading.trim());
    body.push_str("\n\n");
    for (label, url) in items {
        let safe_label = label.replace(['[', ']', '(', ')'], "");
        body.push_str("- [");
        body.push_str(&safe_label);
        body.push_str("](");
        body.push_str(url);
        body.push_str(")\n");
    }
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
    fn seo_review_cadence_normalizes_and_maps_cron() {
        assert_eq!(normalize_seo_review_cadence(""), SEO_REVIEW_OFF);
        assert_eq!(normalize_seo_review_cadence("DAILY"), SEO_REVIEW_DAILY);
        assert_eq!(normalize_seo_review_cadence("weekly"), SEO_REVIEW_WEEKLY);
        assert_eq!(seo_review_cron("off"), None);
        assert_eq!(seo_review_cron("daily"), Some("0 9 * * *"));
        assert_eq!(seo_review_cron("weekly"), Some("0 9 * * 1"));
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
        assert!(body.contains("User-agent: Meta-ExternalAgent"));
        assert!(body.contains("User-agent: cohere-ai"));
        assert!(body.contains("Sitemap: https://ex.com/sitemap.xml"));
        assert!(!body.contains("User-agent: ChatGPT-User"));
        assert!(!body.contains("User-agent: Meta-ExternalFetcher"));
    }

    #[test]
    fn robots_search_only_blocks_ai_search() {
        let body = build_robots_txt(Some("https://ex.com"), VISIBILITY_SEARCH_ONLY);
        assert!(body.contains("User-agent: ChatGPT-User"));
        assert!(body.contains("User-agent: PerplexityBot"));
        assert!(body.contains("User-agent: Meta-ExternalFetcher"));
        assert!(body.contains("User-agent: DuckAssistBot"));
    }

    #[test]
    fn robots_omits_sitemap_without_durable_origin() {
        let body = build_robots_txt(None, VISIBILITY_AI_FULL);
        assert!(!body.contains("Sitemap:"));
        assert!(body.contains("Allow: /"));
    }

    #[test]
    fn robots_non_private_disallows_admin_spa() {
        let body = build_robots_txt(Some("https://ex.com"), VISIBILITY_AI_FULL);
        for path in [
            "/login",
            "/config",
            "/journal/starred",
            "/journal/workbench",
            "/agent/settings",
        ] {
            assert!(
                body.contains(&format!("Disallow: {path}")),
                "missing Disallow for {path}"
            );
        }
        assert!(!body.contains("Disallow: /journal/friends"));
    }

    #[test]
    fn llms_lists_routes() {
        let body = build_llms_txt(
            "My Site",
            "A personal hub.",
            Some("https://ex.com"),
            &[("Home", "/"), ("Journal", "/journal")],
        );
        assert!(body.contains("# My Site"));
        assert!(body.contains("[Journal](https://ex.com/journal)"));
    }

    #[test]
    fn llms_uses_relative_paths_without_origin() {
        let body = build_llms_txt(
            "My Site",
            "A personal hub.",
            None,
            &[("Home", "/"), ("Journal", "/journal")],
        );
        assert!(body.contains("[Journal](/journal)"));
        assert!(!body.contains("https://"));
    }

    #[test]
    fn llms_appends_named_entries() {
        let mut body = String::from("# Site\n");
        append_llms_link_section(
            &mut body,
            "Apps",
            &[("Todo".into(), "https://ex.com/tapp/run/todo".into())],
        );
        assert!(body.contains("## Apps"));
        assert!(body.contains("[Todo](https://ex.com/tapp/run/todo)"));
    }
}
