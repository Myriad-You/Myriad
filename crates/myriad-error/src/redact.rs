//! Best-effort secret redaction for logs and JSON error bodies.
//!
//! Covers known env values, common auth headers, and sensitive assignment keys
//! (OIDC / Notion / AI / tokens) so accidental echoes never reach operators.

/// Env keys whose *values* must never appear in logs or error bodies.
const SECRET_ENV_KEYS: &[&str] = &[
    "UPDATE_TOKEN",
    "UPDATER_GATEWAY_SECRET",
    "JWT_SECRET",
    "POSTGRES_PASSWORD",
    "GITHUB_TOKEN",
    "MYRIAD_SETUP_SECRET",
    "MYRIAD_BOOTSTRAP_TOKEN",
    "DATABASE_URL",
    "OIDC_CLIENT_SECRET",
    "OAUTH_CLIENT_SECRET",
    "GITHUB_CLIENT_SECRET",
    "NOTION_TOKEN",
    "NOTION_API_KEY",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "AI_API_KEY",
    "QQ_BOT_APP_SECRET",
];

/// `key=value` / JSON-ish keys to blank after the separator (case-insensitive match on key).
const SECRET_ASSIGNMENT_KEYS: &[&str] = &[
    "password",
    "token",
    "access_token",
    "refresh_token",
    "client_secret",
    "api_key",
    "apikey",
    "authorization",
    "setup_secret",
    "notion_token",
    "openai_api_key",
    "anthropic_api_key",
    "gemini_api_key",
    "app_secret",
    "qq_bot_app_secret",
    "clientSecret",
];

/// Redact known secrets from `input`. Safe on every error / log path.
pub fn redact_secrets(input: &str) -> String {
    let mut out = input.to_string();
    for key in SECRET_ENV_KEYS {
        if let Ok(val) = std::env::var(key) {
            let val = val.trim();
            // Short values create false positives; require a real secret-sized span.
            if val.len() >= 8 && out.contains(val) {
                out = out.replace(val, &format!("[{key}_REDACTED]"));
            }
        }
    }
    // Bearer before Authorization so "Authorization: Bearer <token>" loses the token first.
    out = redact_pattern(&out, "Bearer ");
    out = redact_pattern(&out, "X-Update-Token:");
    out = redact_pattern(&out, "X-Updater-Gateway-Secret:");
    out = redact_pattern(&out, "X-Setup-Secret:");
    out = redact_pattern(&out, "X-Bootstrap-Token:");
    out = redact_pattern(&out, "Authorization:");
    for key in SECRET_ASSIGNMENT_KEYS {
        out = redact_kv_assignment(&out, key);
    }
    out
}

fn redact_pattern(input: &str, prefix: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let pref_lower = prefix.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    let mut rest_lower = lower.as_str();
    while let Some(idx) = rest_lower.find(&pref_lower) {
        out.push_str(&rest[..idx]);
        out.push_str(prefix);
        let after = &rest[idx + prefix.len()..];
        let after_l = &rest_lower[idx + pref_lower.len()..];
        let trim_start = after.chars().take_while(|c| c.is_whitespace()).count();
        out.push_str(&after[..trim_start]);
        let token_part = &after[trim_start..];
        let token_len = token_part
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\'' && *c != ',' && *c != '}')
            .count();
        if token_len > 0 {
            out.push_str("[REDACTED]");
        }
        rest = &token_part[token_len..];
        rest_lower = &after_l[trim_start + token_len..];
    }
    out.push_str(rest);
    out
}

fn redact_kv_assignment(input: &str, key: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let lower = input.to_ascii_lowercase();
    let needle = format!("{}=", key.to_ascii_lowercase());
    let mut rest = input;
    let mut rest_lower = lower.as_str();
    while let Some(idx) = rest_lower.find(&needle) {
        out.push_str(&rest[..idx + needle.len()]);
        let after = &rest[idx + needle.len()..];
        let after_l = &rest_lower[idx + needle.len()..];
        let val_len = after
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '&' && *c != '"' && *c != '\'' && *c != ',')
            .count();
        if val_len > 0 {
            out.push_str("[REDACTED]");
        }
        rest = &after[val_len..];
        rest_lower = &after_l[val_len..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_bearer() {
        let r = redact_secrets("fail Authorization: Bearer supersecrettoken99");
        assert!(!r.contains("supersecrettoken99"));
        assert!(r.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_bootstrap_header_pattern() {
        let r = redact_secrets("header X-Setup-Secret: abcdefghijklmnop");
        assert!(!r.contains("abcdefghijklmnop"));
        assert!(r.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_oidc_and_ai_assignment_keys() {
        let r = redact_secrets(
            "client_secret=super-secret-oidc-value api_key=sk-live-abcdefgh refresh_token=rtok12345678",
        );
        assert!(!r.contains("super-secret-oidc-value"));
        assert!(!r.contains("sk-live-abcdefgh"));
        assert!(!r.contains("rtok12345678"));
        assert!(r.contains("client_secret=[REDACTED]"));
        assert!(r.contains("api_key=[REDACTED]"));
        assert!(r.contains("refresh_token=[REDACTED]"));
        let qq = redact_secrets(
            "qq_bot_app_secret=super-secret-qq-value clientSecret=also-secret-value",
        );
        assert!(!qq.contains("super-secret-qq-value"));
        assert!(!qq.contains("also-secret-value"));
    }

    #[test]
    fn redacts_env_value_when_present() {
        // SAFETY: test-only; serial tests in this crate don't race env mutation.
        const KEY: &str = "JWT_SECRET";
        const VAL: &str = "test-jwt-secret-value-xyz";
        // Use a unique value unlikely to collide with the host env's real JWT_SECRET.
        let prev = std::env::var(KEY).ok();
        std::env::set_var(KEY, VAL);
        let r = redact_secrets(&format!("boom token was {VAL} in body"));
        if let Some(p) = prev {
            std::env::set_var(KEY, p);
        } else {
            std::env::remove_var(KEY);
        }
        assert!(!r.contains(VAL), "secret value must not leak: {r}");
        assert!(r.contains("[JWT_SECRET_REDACTED]"), "got: {r}");
    }
}
