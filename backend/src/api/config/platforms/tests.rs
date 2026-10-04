use super::*;
use serde_json::{Value, json};

#[test]
fn public_platform_summary_matches_admin_projection_without_configuration_values() {
    let configured = DynamicConfig {
        github_username: Some("octocat".into()),
        github_token: Some("private-github-token".into()),
        github_enabled: Some(false),
        bangumi_access_token: Some("private-bangumi-token".into()),
        netease_user_id: Some("123".into()),
        platform_order: Some(vec![
            "netease music".into(),
            "GITHUB".into(),
            "missing".into(),
        ]),
        ..DynamicConfig::default()
    };
    let whitespace = DynamicConfig {
        github_token: Some(" \n ".into()),
        bangumi_username: Some("  ".into()),
        ..DynamicConfig::default()
    };
    for stored in [DynamicConfig::default(), configured, whitespace] {
        let admin = build_platforms(&stored, false, true);
        let expected: Vec<Value> = admin
            .into_iter()
            .map(|platform| {
                json!({
                    "name": platform.name, "enabled": platform.enabled,
                    "has_token": platform.has_token, "icon": platform.icon,
                    "description": platform.description,
                })
            })
            .collect();
        let actual = serde_json::to_value(public_platform_summaries(&stored)).unwrap();
        assert_eq!(actual, json!(expected));
        assert_eq!(actual.as_array().unwrap().len(), 11);
        assert!(
            actual
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p.as_object().unwrap().len() == 5)
        );
        assert!(!actual.to_string().contains("private-"));
        assert!(
            build_platforms(&stored, false, false)
                .iter()
                .all(|p| p.config_fields.is_empty())
        );
    }
}

#[test]
fn admin_platform_fields_keep_secret_masking_and_backup_reveal() {
    let stored = DynamicConfig {
        github_token: Some("private-token".into()),
        ..DynamicConfig::default()
    };
    let masked = build_platforms(&stored, false, true);
    let revealed = build_platforms(&stored, true, true);
    let token = |platforms: Vec<PlatformConfig>| {
        platforms
            .into_iter()
            .find(|p| p.name == "GitHub")
            .unwrap()
            .config_fields
            .into_iter()
            .find(|f| f.key == "token")
            .unwrap()
            .value
    };
    assert_eq!(token(masked), mask_secret_display_value());
    assert_eq!(token(revealed), "private-token");
}
