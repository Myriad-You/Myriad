//! Best-effort secret redaction for logs and JSON error bodies.
//!
//! Ensures `UPDATE_TOKEN`, `UPDATER_GATEWAY_SECRET`, `JWT_SECRET`, `POSTGRES_PASSWORD`,
//! and `GITHUB_TOKEN` values (and common header patterns) never appear in operator-facing output.

const SECRET_ENV_KEYS: &[&str] = &[
    "UPDATE_TOKEN",
    "UPDATER_GATEWAY_SECRET",
    "JWT_SECRET",
    "POSTGRES_PASSWORD",
    "GITHUB_TOKEN",
];

/// Redact known secrets from `input`. Safe on every error / log path.
pub fn redact_secrets(input: &str) -> String {
    let mut out = input.to_string();
    for key in SECRET_ENV_KEYS {
        if let Ok(val) = std::env::var(key) {
            let val = val.trim();
            if val.len() >= 8 && out.contains(val) {
                out = out.replace(val, &format!("[{key}_REDACTED]"));
            }
        }
    }
    // Bearer before Authorization so "Authorization: Bearer <token>" loses the token first.
    out = redact_pattern(&out, "Bearer ");
    out = redact_pattern(&out, "X-Update-Token:");
    out = redact_pattern(&out, "X-Updater-Gateway-Secret:");
    out = redact_pattern(&out, "Authorization:");
    out = redact_kv_assignment(&out, "password");
    out = redact_kv_assignment(&out, "token");
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
    let needle = format!("{key}=");
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
}
