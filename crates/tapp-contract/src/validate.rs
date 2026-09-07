//! Full Tapp manifest validation shared by install and the offline CLI.

use crate::contract_rules::{
    API_INJECT_RESERVED_PREFIXES, FORBIDDEN_OUTBOUND_HEADERS, HTTP_BODY_METHODS, HTTP_METHODS,
    MAX_AI_OPERATIONS, MAX_CREDENTIAL_HEADER_PREFIX_LEN, MAX_CREDENTIAL_KEY_LEN,
    MAX_DATA_EXCHANGE_DECLARATIONS, MAX_DATA_EXCHANGE_RESPONSE_BYTES, MAX_OPEN_URLS,
    MAX_OPEN_URL_ID_LEN, MAX_TAPP_ASSETS, MAX_TAPP_CREDENTIALS, MAX_TAPP_GAME_ASSETS,
    MAX_TAPP_GAME_MESSAGE_BYTES, MAX_TAPP_GAME_PLAYERS, MAX_TAPP_GAME_PROTOCOL_LEN,
    MAX_TAPP_RUNTIME_MODULES, MAX_WIDGETS_PER_TAPP, MIN_TAPP_GAME_PLAYERS, OPEN_URL_PERMISSION,
    ROUTE_MAX_MAX_SKEW_SECS, ROUTE_MAX_PREFIX_LEN, ROUTE_METHODS, ROUTE_MIN_MAX_SKEW_SECS,
    TAPP_RUNTIME_MODULES,
};
use crate::manifest::{
    valid_agent_name, valid_event_topic, valid_inbound_route_path, valid_inbound_verify_header,
    TappAiContextSource, TappAiOperation, TappAiOutputFormat, TappApiAccess, TappCredentialIn,
    TappCredentialSignAlg, TappHttpBodyMode, TappManifest, TappOpenUrlMatch, TappRouteVerifyOver,
};
use crate::paths::{
    is_safe_path_component, is_valid_widget_size, parse_system_version, valid_data_exchange_id,
    validate_asset_path, validate_inline_data_schema, validate_resource_extension,
    validate_resource_path, validate_tapp_id, validate_tapp_settings,
    validate_widget_refresh_policy,
};
use crate::permission::{tapp_permission_replacement_hint, TappPermission};
use crate::urls::{validate_http_url, validate_open_url_target};

fn validate_open_urls(manifest: &TappManifest) -> Result<(), String> {
    let has_permission = manifest
        .permissions
        .iter()
        .any(|value| value == OPEN_URL_PERMISSION);
    let entries = manifest.open_urls.as_deref().unwrap_or(&[]);

    if entries.is_empty() {
        if has_permission {
            return Err(
                "Tapp permission ui:openUrl requires a non-empty openUrls allowlist".to_string(),
            );
        }
        return Ok(());
    }
    if !has_permission {
        return Err("Tapp openUrls requires manifest permission ui:openUrl".to_string());
    }
    if entries.len() > MAX_OPEN_URLS {
        return Err(format!(
            "Tapp openUrls accepts at most {MAX_OPEN_URLS} entries"
        ));
    }

    let mut ids = std::collections::HashSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let field = format!("openUrls[{index}]");
        if entry.id.is_empty()
            || entry.id.len() > MAX_OPEN_URL_ID_LEN
            || !is_safe_path_component(&entry.id)
            || !entry
                .id
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphanumeric())
        {
            return Err(format!(
                "Tapp {field}.id must use 1-{MAX_OPEN_URL_ID_LEN} ASCII letters, numbers, dots, underscores, or hyphens"
            ));
        }
        if !ids.insert(entry.id.as_str()) {
            return Err(format!("Duplicate Tapp openUrls id: {}", entry.id));
        }
        let parsed = validate_open_url_target(&entry.url, &format!("{field}.url"))?;
        // Prefix match is path-based; require a non-empty path (at least `/`).
        if matches!(entry.match_mode, TappOpenUrlMatch::Prefix) && parsed.path().is_empty() {
            return Err(format!(
                "Tapp {field}.url for match=prefix must include a path (e.g. https://example.com/docs/)"
            ));
        }
        // Reject fragment-only noise in declarations — host drops fragments on open.
        if parsed.fragment().is_some() {
            return Err(format!("Tapp {field}.url must not include a #fragment"));
        }
        let _ = entry.match_mode; // exhaustively known via serde enum
    }
    Ok(())
}

pub fn valid_locale_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 35
        && tag.split('-').all(|part| {
            !part.is_empty() && part.len() <= 8 && part.bytes().all(|b| b.is_ascii_alphanumeric())
        })
        && tag.split('-').next().is_some_and(|lang| {
            (2..=3).contains(&lang.len()) && lang.bytes().all(|b| b.is_ascii_alphabetic())
        })
}

/// 层入口与层内资源路径校验。
///
/// 取代旧的 `main` 加一组平铺 styles/pageTemplate 字段：每层自带入口和资源，
/// 页面存在与否由是否声明 `page` 层决定，不再有能与内容对不上的独立开关。
fn validate_tapp_layers(manifest: &TappManifest) -> Result<(), String> {
    let has_widgets = manifest
        .widgets
        .as_ref()
        .is_some_and(|widgets| !widgets.is_empty());
    if manifest.core.is_none() && manifest.page.is_none() && !has_widgets {
        return Err("Tapp must declare at least one of core, page or widgets".to_string());
    }

    if let Some(core) = &manifest.core {
        validate_resource_path(&core.entry)?;
        validate_resource_extension(&core.entry, ".js", "core.entry")?;
        if let Some(styles) = &core.styles {
            validate_resource_path(styles)?;
            validate_resource_extension(styles, ".css", "core.styles")?;
        }
    } else if manifest
        .background_requirements
        .as_ref()
        .is_some_and(|requirements| !requirements.is_empty())
    {
        // 后台常驻只运行 core：没有 core 就没有任何可常驻的代码。
        return Err("Tapp declaring backgroundRequirements must declare a core layer".to_string());
    }

    if let Some(page) = &manifest.page {
        if page.entry.is_none() && page.template.is_none() {
            return Err("Tapp page layer must declare entry and/or template".to_string());
        }
        if let Some(entry) = &page.entry {
            validate_resource_path(entry)?;
            validate_resource_extension(entry, ".js", "page.entry")?;
        }
        if let Some(template) = &page.template {
            validate_resource_path(template)?;
            validate_resource_extension(template, ".html", "page.template")?;
        }
        if let Some(styles) = &page.styles {
            validate_resource_path(styles)?;
            validate_resource_extension(styles, ".css", "page.styles")?;
        }
    }

    if let Some(widgets) = &manifest.widgets {
        for widget in widgets {
            if let Some(entry) = &widget.entry {
                validate_resource_path(entry)?;
                validate_resource_extension(entry, ".js", "widgets[].entry")?;
            }
            if let Some(styles) = &widget.styles {
                validate_resource_path(styles)?;
                validate_resource_extension(styles, ".css", "widgets[].styles")?;
            }
        }
    }

    Ok(())
}

pub fn validate_tapp_manifest(
    manifest: &TappManifest,
    current_system_version: &semver::Version,
) -> Result<(), String> {
    validate_tapp_id(&manifest.id)?;
    if manifest.name.trim().is_empty() || manifest.name.len() > 255 {
        return Err("Tapp name must contain 1-255 characters".to_string());
    }
    semver::Version::parse(&manifest.version)
        .map_err(|_| "Tapp version must be valid semantic version".to_string())?;
    if manifest.category.is_none() {
        return Err("Tapp category is required".to_string());
    }
    if manifest
        .description
        .as_ref()
        .is_some_and(|description| description.len() > 2_000)
    {
        return Err("Tapp description must not exceed 2000 characters".to_string());
    }
    if let Some(locales) = &manifest.locales {
        if locales.len() > 32 {
            return Err("Tapp locales must not declare more than 32 languages".to_string());
        }
        for (tag, entry) in locales {
            if !valid_locale_tag(tag) {
                return Err(format!(
                    "Tapp locales key '{tag}' must be a BCP-47 language tag (e.g. zh-CN)"
                ));
            }
            if entry
                .name
                .as_ref()
                .is_some_and(|name| name.trim().is_empty() || name.len() > 255)
            {
                return Err(format!(
                    "Tapp locales['{tag}'].name must contain 1-255 characters"
                ));
            }
            if entry
                .description
                .as_ref()
                .is_some_and(|description| description.len() > 2_000)
            {
                return Err(format!(
                    "Tapp locales['{tag}'].description must not exceed 2000 characters"
                ));
            }
        }
    }
    if manifest
        .icon
        .as_ref()
        .is_some_and(|icon| icon.len() > 2_048)
    {
        return Err("Tapp icon must not exceed 2048 characters".to_string());
    }
    if manifest
        .icon_svg
        .as_ref()
        .is_some_and(|icon| icon.len() > 65_536)
    {
        return Err("Tapp iconSvg must not exceed 64 KiB".to_string());
    }
    if manifest.theme_color.as_ref().is_some_and(|color| {
        color.len() != 7
            || !color.starts_with('#')
            || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err("Tapp themeColor must use #RRGGBB format".to_string());
    }
    for (field, value) in [
        ("homepage", manifest.homepage.as_deref()),
        ("repository", manifest.repository.as_deref()),
    ] {
        if let Some(value) = value {
            validate_http_url(value, field)?;
        }
    }
    validate_tapp_layers(manifest)?;
    if let Some(required) = manifest.min_system_version.as_deref() {
        let required = parse_system_version(required)?;
        if current_system_version < &required {
            return Err(format!(
                "Tapp requires Myriad {required} or newer; current version is {current_system_version}"
            ));
        }
    }
    if let Some(author) = &manifest.author {
        if author.name.trim().is_empty() || author.name.len() > 255 {
            return Err("Tapp author.name must contain 1-255 characters".to_string());
        }
        if author.email.as_ref().is_some_and(|email| {
            email.len() > 320 || email.chars().any(char::is_whitespace) || !email.contains('@')
        }) {
            return Err("Invalid Tapp author.email".to_string());
        }
        if let Some(url) = &author.url {
            validate_http_url(url, "author.url")?;
        }
    }
    if manifest.permissions.len() > 64 {
        return Err("Tapp permissions accepts at most 64 entries".to_string());
    }
    let mut permissions = std::collections::HashSet::new();
    for permission in &manifest.permissions {
        if TappPermission::from_str(permission).is_none() {
            // Fail-closed：未知权限不落库、不签发；只有错误提示会带上共享的替代建议
            // （如已移除的 storage 与 Brew 粗权限），不创建任何别名。
            return Err(match tapp_permission_replacement_hint(permission) {
                Some(hint) => format!("Unknown Tapp permission '{permission}'; {hint}"),
                None => format!("Unknown Tapp permission '{permission}'"),
            });
        }
        if !permissions.insert(permission.as_str()) {
            return Err(format!("Duplicate Tapp permission: {permission}"));
        }
    }
    if let Some(modules) = &manifest.runtime_modules {
        if modules.len() > MAX_TAPP_RUNTIME_MODULES {
            return Err(format!(
                "Tapp runtimeModules accepts at most {MAX_TAPP_RUNTIME_MODULES} entries"
            ));
        }
        if !matches!(
            manifest.category,
            Some(crate::manifest::TappCategory::Game)
                | Some(crate::manifest::TappCategory::Developer)
        ) {
            return Err("runtimeModules is only allowed for game or developer Tapps".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for module in modules {
            if !TAPP_RUNTIME_MODULES.contains(&module.as_str()) || !seen.insert(module.as_str()) {
                return Err(format!(
                    "Unknown or duplicate runtime module: {module}; allowed: {}",
                    TAPP_RUNTIME_MODULES.join(", ")
                ));
            }
        }
    }

    if let Some(game) = &manifest.game {
        if game.protocol.is_empty() || game.protocol.len() > MAX_TAPP_GAME_PROTOCOL_LEN {
            return Err(format!(
                "Tapp game.protocol must be 1-{MAX_TAPP_GAME_PROTOCOL_LEN} characters"
            ));
        }
        if !game
            .protocol
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
        {
            return Err("Tapp game.protocol must be lowercase [a-z0-9._-]".to_string());
        }
        if let Some(players) = game.max_players {
            if !(MIN_TAPP_GAME_PLAYERS..=MAX_TAPP_GAME_PLAYERS).contains(&players) {
                return Err(format!(
                    "Tapp game.maxPlayers must be {MIN_TAPP_GAME_PLAYERS}-{MAX_TAPP_GAME_PLAYERS}"
                ));
            }
        }
        if let Some(bytes) = game.max_message_bytes {
            if !(1024..=MAX_TAPP_GAME_MESSAGE_BYTES).contains(&bytes) {
                return Err(format!(
                    "Tapp game.maxMessageBytes must be 1024-{MAX_TAPP_GAME_MESSAGE_BYTES}"
                ));
            }
        }
        if !manifest
            .permissions
            .iter()
            .any(|permission| permission == "game:session")
        {
            return Err("Tapp game requires the game:session permission".to_string());
        }
        for required in ["federation:read", "federation:room", "federation:message"] {
            if !manifest
                .permissions
                .iter()
                .any(|permission| permission == required)
            {
                return Err(format!("Tapp game requires the {required} permission"));
            }
        }
    }

    if let Some(requirements) = &manifest.background_requirements {
        if requirements.len() > 16 {
            return Err("Tapp backgroundRequirements accepts at most 16 entries".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for requirement in requirements {
            if !matches!(
                requirement.as_str(),
                "media" | "sync" | "notification" | "scheduler" | "event-listener" | "realtime"
            ) || !seen.insert(requirement.as_str())
            {
                return Err(format!(
                    "Unknown or duplicate background requirement: {requirement}"
                ));
            }
        }
    }

    if let Some(settings) = &manifest.settings {
        validate_tapp_settings(settings, "Tapp")?;
    }
    let setting_keys: std::collections::HashSet<&str> = manifest
        .settings
        .iter()
        .flat_map(|settings| settings.iter())
        .map(|setting| setting.key.as_str())
        .collect();

    let mut credential_keys = std::collections::HashSet::new();
    if let Some(credentials) = &manifest.credentials {
        if credentials.len() > MAX_TAPP_CREDENTIALS {
            return Err(format!(
                "Tapp credentials accepts at most {MAX_TAPP_CREDENTIALS} entries"
            ));
        }
        for credential in credentials {
            if credential.key.is_empty()
                || credential.key.len() > MAX_CREDENTIAL_KEY_LEN
                || !valid_agent_name(&credential.key)
                || !credential_keys.insert(credential.key.as_str())
            {
                return Err(format!(
                    "Invalid or duplicate Tapp credential key: {}",
                    credential.key
                ));
            }
            if setting_keys.contains(credential.key.as_str()) {
                return Err(format!(
                    "Tapp credential key conflicts with public setting key: {}",
                    credential.key
                ));
            }
            if credential.label.trim().is_empty() || credential.label.len() > 255 {
                return Err(format!(
                    "Invalid label for Tapp credential: {}",
                    credential.key
                ));
            }
            if credential
                .description
                .as_ref()
                .is_some_and(|value| value.len() > 2_000)
                || credential
                    .placeholder
                    .as_ref()
                    .is_some_and(|value| value.len() > 255)
            {
                return Err(format!(
                    "Tapp credential metadata is too long: {}",
                    credential.key
                ));
            }
        }
    }

    if let Some(assets) = &manifest.assets {
        let max_assets = if manifest.uses_game_asset_limits() {
            MAX_TAPP_GAME_ASSETS
        } else {
            MAX_TAPP_ASSETS
        };
        if assets.len() > max_assets {
            return Err(format!("Tapp assets accepts at most {max_assets} entries"));
        }
        let mut seen = std::collections::HashSet::new();
        for path in assets {
            validate_asset_path(path)?;
            if !seen.insert(path.as_str()) {
                return Err(format!("Duplicate Tapp asset path: {path}"));
            }
        }
    }

    if let Some(widgets) = &manifest.widgets {
        if !widgets.is_empty()
            && !manifest
                .permissions
                .iter()
                .any(|permission| permission == "widget:register")
        {
            return Err("Tapp widgets require widget:register permission".to_string());
        }
        if widgets.len() > MAX_WIDGETS_PER_TAPP {
            return Err(format!("Too many Widgets (max {MAX_WIDGETS_PER_TAPP})"));
        }
        let mut widget_ids = std::collections::HashSet::new();
        for widget in widgets {
            if !is_safe_path_component(&widget.id) || !widget_ids.insert(widget.id.as_str()) {
                return Err(format!("Invalid or duplicate Widget ID: {}", widget.id));
            }
            if widget.name.is_empty() || widget.name.len() > 255 {
                return Err(format!("Invalid Widget name: {}", widget.id));
            }
            if widget.sizes.is_empty()
                || widget.sizes.len() > 10
                || widget.sizes.iter().any(|size| !is_valid_widget_size(size))
                || !widget.sizes.contains(&widget.default_size)
            {
                return Err(format!("Invalid Widget sizes: {}", widget.id));
            }
            validate_tapp_settings(&widget.settings, &format!("Widget {}", widget.id))?;
            if let Some(policy) = &widget.refresh_policy {
                validate_widget_refresh_policy(policy, &widget.id)?;
            }
            if let Some(templates) = &widget.templates {
                for (size, path) in templates {
                    if !is_valid_widget_size(size) || !widget.sizes.contains(size) {
                        return Err(format!(
                            "Widget template uses an undeclared size {size}: {}",
                            widget.id
                        ));
                    }
                    validate_resource_path(path)?;
                    validate_resource_extension(path, ".html", "Widget template")?;
                }
            }
        }
    }

    let mut bound_credential_keys = std::collections::HashSet::new();
    let mut inbound_route_paths = std::collections::HashSet::new();
    if let Some(apis) = &manifest.apis {
        if apis.len() > 64 {
            return Err("Tapp apis accepts at most 64 entries".to_string());
        }
        for (name, api) in apis {
            if !valid_agent_name(name) {
                return Err(format!("Invalid Tapp API name: {name}"));
            }
            if api.cache_ttl > 86_400 {
                return Err(format!(
                    "Tapp API {name} cacheTtl must not exceed 86400 seconds"
                ));
            }
            if serde_json::to_string(api).is_ok_and(|encoded| encoded.contains("{{secrets.")) {
                return Err(format!(
                    "Tapp API {name} cannot reference host secret templates"
                ));
            }
            match api.api_type.as_str() {
                "http" => {
                    if api.endpoint.is_none() {
                        return Err(format!("HTTP Tapp API {name} requires endpoint"));
                    }
                    if api.builtin.is_some() {
                        return Err(format!("HTTP Tapp API {name} cannot declare builtin"));
                    }
                    if !manifest
                        .permissions
                        .iter()
                        .any(|permission| permission == "network:fetch")
                    {
                        return Err(format!("HTTP Tapp API {name} requires network:fetch"));
                    }
                    if let Some(binding) = &api.credential {
                        if !credential_keys.contains(binding.key.as_str()) {
                            return Err(format!(
                                "Tapp API {name} references undeclared credential: {}",
                                binding.key
                            ));
                        }
                        let endpoint = api.endpoint.as_deref().expect("checked endpoint");
                        let parsed = url::Url::parse(endpoint).map_err(|_| {
                            format!(
                                "Credential-bound Tapp API {name} requires a fixed absolute HTTPS endpoint"
                            )
                        })?;
                        if parsed.scheme() != "https"
                            || parsed.host_str().is_none()
                            || !parsed.username().is_empty()
                            || parsed.password().is_some()
                            || parsed
                                .host_str()
                                .is_some_and(|host| host.contains('{') || host.contains('}'))
                        {
                            return Err(format!(
                                "Credential-bound Tapp API {name} requires a fixed absolute HTTPS origin"
                            ));
                        }
                        let resolved = binding.resolve().map_err(|_| {
                            format!("Invalid credential binding for Tapp API {name}")
                        })?;
                        if binding
                            .prefix
                            .as_ref()
                            .is_some_and(|prefix| prefix.len() > MAX_CREDENTIAL_HEADER_PREFIX_LEN)
                        {
                            return Err(format!(
                                "Credential header prefix is too long for Tapp API {name}"
                            ));
                        }
                        match resolved.placement {
                            TappCredentialIn::Header => {
                                let header =
                                    crate::headers::parse_http_header_name(&resolved.field)
                                        .map_err(|_| {
                                            format!("Invalid credential header for Tapp API {name}")
                                        })?;
                                if FORBIDDEN_OUTBOUND_HEADERS.iter().any(|forbidden| {
                                    forbidden.eq_ignore_ascii_case(header.as_str())
                                }) {
                                    return Err(format!(
                                        "Forbidden credential header for Tapp API {name}"
                                    ));
                                }
                                if api.headers.as_ref().is_some_and(|headers| {
                                    headers.keys().any(|name| {
                                        name.eq_ignore_ascii_case(resolved.field.as_str())
                                    })
                                }) {
                                    return Err(format!(
                                        "Tapp API {name} declares the credential header twice"
                                    ));
                                }
                            }
                            TappCredentialIn::Query => {
                                if parsed
                                    .query_pairs()
                                    .any(|(name, _)| name.as_ref() == resolved.field)
                                {
                                    return Err(format!(
                                        "Tapp API {name} declares the credential query field twice"
                                    ));
                                }
                            }
                            TappCredentialIn::Form => {
                                if api.body_mode != TappHttpBodyMode::Form {
                                    return Err(format!(
                                        "Tapp API {name} form credentials require bodyMode form"
                                    ));
                                }
                                if !HTTP_BODY_METHODS.contains(&api.method.as_str()) {
                                    return Err(format!(
                                        "Tapp API {name} form credentials require one of: {}",
                                        HTTP_BODY_METHODS.join(", ")
                                    ));
                                }
                                let Some(body) =
                                    api.body.as_ref().and_then(serde_json::Value::as_object)
                                else {
                                    return Err(format!(
                                        "Tapp API {name} form credentials require a form object body"
                                    ));
                                };
                                if body.contains_key(&resolved.field) {
                                    return Err(format!(
                                        "Tapp API {name} declares the credential form field twice"
                                    ));
                                }
                            }
                            TappCredentialIn::Sign => {
                                let sign = resolved.sign.as_ref().expect("resolved sign");
                                if !sign.alg.is_implemented() {
                                    return Err(format!(
                                        "Tapp API {name} credential sign algorithm {} is not implemented",
                                        sign.alg.as_str()
                                    ));
                                }
                                if !HTTP_BODY_METHODS.contains(&api.method.as_str()) {
                                    return Err(format!(
                                        "Tapp API {name} signed credentials require one of: {}",
                                        HTTP_BODY_METHODS.join(", ")
                                    ));
                                }
                                if api.body_mode == TappHttpBodyMode::Raw {
                                    return Err(format!(
                                        "Tapp API {name} signed credentials cannot use bodyMode raw"
                                    ));
                                }
                                let Some(body) =
                                    api.body.as_ref().and_then(serde_json::Value::as_object)
                                else {
                                    return Err(format!(
                                        "Tapp API {name} signed credentials require a JSON or form object body"
                                    ));
                                };
                                if body.contains_key(&resolved.field) {
                                    return Err(format!(
                                        "Tapp API {name} must not declare the signature field in body"
                                    ));
                                }
                                for field_name in &sign.over {
                                    if sign.timestamp_field.as_deref() == Some(field_name.as_str())
                                    {
                                        continue;
                                    }
                                    let Some(declared) = body.get(field_name) else {
                                        return Err(format!(
                                            "Tapp API {name} sign.over field '{field_name}' is not declared in body"
                                        ));
                                    };
                                    if !declared.is_string()
                                        && !declared.is_number()
                                        && !declared.is_boolean()
                                        && !declared.is_null()
                                    {
                                        return Err(format!(
                                            "Tapp API {name} sign.over field '{field_name}' must be a scalar"
                                        ));
                                    }
                                }
                            }
                        }
                        bound_credential_keys.insert(binding.key.as_str());
                    }
                    match api.body_mode {
                        TappHttpBodyMode::Json => {}
                        TappHttpBodyMode::Raw => {
                            if !HTTP_BODY_METHODS.contains(&api.method.as_str()) {
                                return Err(format!(
                                    "HTTP Tapp API {name} bodyMode raw requires one of: {}",
                                    HTTP_BODY_METHODS.join(", ")
                                ));
                            }
                            if !matches!(api.body, Some(serde_json::Value::String(_))) {
                                return Err(format!(
                                    "HTTP Tapp API {name} raw body must be a string template"
                                ));
                            }
                        }
                        TappHttpBodyMode::Form => {
                            if !HTTP_BODY_METHODS.contains(&api.method.as_str()) {
                                return Err(format!(
                                    "HTTP Tapp API {name} bodyMode form requires one of: {}",
                                    HTTP_BODY_METHODS.join(", ")
                                ));
                            }
                            let Some(serde_json::Value::Object(fields)) = &api.body else {
                                return Err(format!(
                                    "HTTP Tapp API {name} form body must be an object"
                                ));
                            };
                            if fields.values().any(|value| {
                                matches!(
                                    value,
                                    serde_json::Value::Array(_) | serde_json::Value::Object(_)
                                )
                            }) {
                                return Err(format!(
                                    "HTTP Tapp API {name} form body fields must be scalar values"
                                ));
                            }
                        }
                    }
                }
                "builtin" => {
                    let Some(builtin) = api.builtin.as_deref() else {
                        return Err(format!("Builtin Tapp API {name} requires builtin"));
                    };
                    if !matches!(builtin, "geo" | "ai:chat" | "ai:generate") {
                        return Err(format!("Unknown builtin Tapp API: {builtin}"));
                    }
                    if api.endpoint.is_some()
                        || api.headers.is_some()
                        || api.body.is_some()
                        || api.body_mode != TappHttpBodyMode::Json
                        || api.spoof.is_some()
                        || api.inject.is_some()
                        || api.credential.is_some()
                    {
                        return Err(format!("Builtin Tapp API {name} contains HTTP-only fields"));
                    }
                    let required_permission = match builtin {
                        "ai:chat" => Some("ai:chat"),
                        "ai:generate" => Some("ai:generate"),
                        _ => None,
                    };
                    if required_permission.is_some_and(|required| {
                        !manifest
                            .permissions
                            .iter()
                            .any(|permission| permission == required)
                    }) {
                        return Err(format!(
                            "Builtin Tapp API {name} requires permission {}",
                            required_permission.expect("checked permission")
                        ));
                    }
                    let required_operation = match builtin {
                        "ai:chat" => Some(TappAiOperation::Chat),
                        "ai:generate" => Some(TappAiOperation::Generate),
                        _ => None,
                    };
                    if let Some(operation) = required_operation {
                        let ai = manifest.ai.as_ref().ok_or_else(|| {
                            format!(
                                "Builtin Tapp API {name} requires a protocolVersion 2 AI declaration"
                            )
                        })?;
                        if ai.protocol_version != 2 || !ai.operations.contains(&operation) {
                            return Err(format!(
                                "Builtin Tapp API {name} requires the matching AI operation"
                            ));
                        }
                        if !ai.output_formats.contains(&TappAiOutputFormat::Text) {
                            return Err(format!("Builtin Tapp API {name} requires AI text output"));
                        }
                    }
                }
                other => return Err(format!("Unknown Tapp API type: {other}")),
            }
            if !HTTP_METHODS.contains(&api.method.as_str()) {
                return Err(format!(
                    "Invalid HTTP method for Tapp API {name}: must be one of {}",
                    HTTP_METHODS.join(", ")
                ));
            }
            if let Some(inject) = &api.inject {
                if inject.len() > 32 {
                    return Err(format!("Tapp API {name} inject accepts at most 32 aliases"));
                }
                for (alias, template) in inject {
                    if !valid_agent_name(alias)
                        || API_INJECT_RESERVED_PREFIXES
                            .iter()
                            .any(|prefix| alias.starts_with(prefix))
                    {
                        return Err(format!(
                            "Invalid or reserved inject alias for Tapp API {name}: {alias}"
                        ));
                    }
                    if template.is_empty() || template.len() > 2_048 {
                        return Err(format!(
                            "Invalid inject template for Tapp API {name}: {alias}"
                        ));
                    }
                }
            }
            if let Some(route) = &api.route {
                if api.access != TappApiAccess::Public {
                    return Err(format!(
                        "Tapp API {name} inbound route requires access public"
                    ));
                }
                if api.api_type == "builtin"
                    && matches!(api.builtin.as_deref(), Some("ai:chat" | "ai:generate"))
                {
                    return Err(format!(
                        "Tapp API {name} inbound route cannot expose AI builtins"
                    ));
                }
                if !valid_inbound_route_path(&route.path) {
                    return Err(format!("Invalid inbound path for Tapp API {name}"));
                }
                if !inbound_route_paths.insert(route.path.as_str()) {
                    return Err(format!(
                        "Duplicate inbound path {} for Tapp API {name}",
                        route.path
                    ));
                }
                if route.methods.is_empty()
                    || route.methods.len() > ROUTE_METHODS.len()
                    || route
                        .methods
                        .iter()
                        .any(|method| !ROUTE_METHODS.contains(&method.as_str()))
                {
                    return Err(format!(
                        "Tapp API {name} inbound route methods must be GET and/or POST"
                    ));
                }
                let mut seen_methods = std::collections::HashSet::new();
                for method in &route.methods {
                    if !seen_methods.insert(method.as_str()) {
                        return Err(format!(
                            "Tapp API {name} inbound route declares method {method} twice"
                        ));
                    }
                }
                let verify = &route.verify;
                if !credential_keys.contains(verify.key.as_str()) {
                    return Err(format!(
                        "Tapp API {name} inbound route references undeclared credential: {}",
                        verify.key
                    ));
                }
                if verify.alg != TappCredentialSignAlg::HmacSha256Raw {
                    return Err(format!(
                        "Tapp API {name} inbound verify algorithm must be hmac-sha256-raw"
                    ));
                }
                if !valid_inbound_verify_header(&verify.header)
                    || !valid_inbound_verify_header(&verify.timestamp_header)
                    || !valid_inbound_verify_header(&verify.nonce_header)
                {
                    return Err(format!("Invalid inbound verify header for Tapp API {name}"));
                }
                let mut verify_headers = std::collections::HashSet::new();
                for header in [
                    verify.header.as_str(),
                    verify.timestamp_header.as_str(),
                    verify.nonce_header.as_str(),
                ] {
                    if !verify_headers.insert(header.to_ascii_lowercase()) {
                        return Err(format!(
                            "Tapp API {name} inbound verify headers must be distinct"
                        ));
                    }
                }
                if verify
                    .prefix
                    .as_ref()
                    .is_some_and(|prefix| prefix.len() > ROUTE_MAX_PREFIX_LEN)
                {
                    return Err(format!("Tapp API {name} inbound verify prefix is too long"));
                }
                let allows_get = route.methods.iter().any(|method| method == "GET");
                let allows_post = route.methods.iter().any(|method| method == "POST");
                match verify.over {
                    TappRouteVerifyOver::CanonicalQuery if !allows_get || allows_post => {
                        return Err(format!(
                            "Tapp API {name} canonical-query verify requires GET-only methods"
                        ));
                    }
                    TappRouteVerifyOver::RawBody if !allows_post || allows_get => {
                        return Err(format!(
                            "Tapp API {name} raw-body verify requires POST-only methods"
                        ));
                    }
                    _ => {}
                }
                if verify.max_skew_secs < ROUTE_MIN_MAX_SKEW_SECS
                    || verify.max_skew_secs > ROUTE_MAX_MAX_SKEW_SECS
                {
                    return Err(format!(
                        "Tapp API {name} inbound maxSkewSecs must be between {ROUTE_MIN_MAX_SKEW_SECS} and {ROUTE_MAX_MAX_SKEW_SECS}"
                    ));
                }
                bound_credential_keys.insert(verify.key.as_str());
            }
        }
    }
    for key in credential_keys {
        if !bound_credential_keys.contains(key) {
            return Err(format!(
                "Tapp credential '{key}' must be bound to at least one declared HTTP API or inbound route verify"
            ));
        }
    }

    if let Some(exchange) = &manifest.data_exchange {
        if exchange.exports.len() > MAX_DATA_EXCHANGE_DECLARATIONS
            || exchange.imports.len() > MAX_DATA_EXCHANGE_DECLARATIONS
        {
            return Err(format!(
                "Too many Data Exchange declarations (max {MAX_DATA_EXCHANGE_DECLARATIONS} per direction)"
            ));
        }

        let mut export_ids = std::collections::HashSet::new();
        for export in &exchange.exports {
            if !valid_data_exchange_id(&export.id) || !export_ids.insert(export.id.as_str()) {
                return Err(format!(
                    "Invalid or duplicate Data Exchange export id: {}",
                    export.id
                ));
            }
            if export.max_bytes == 0 || export.max_bytes > MAX_DATA_EXCHANGE_RESPONSE_BYTES {
                return Err(format!(
                    "Data Exchange export {} maxBytes must be between 1 and {MAX_DATA_EXCHANGE_RESPONSE_BYTES}",
                    export.id
                ));
            }
            if export
                .max_records
                .is_some_and(|limit| limit == 0 || limit > 10_000)
            {
                return Err(format!(
                    "Data Exchange export {} maxRecords must be between 1 and 10000",
                    export.id
                ));
            }
            if export
                .description
                .as_ref()
                .is_some_and(|description| description.len() > 500)
            {
                return Err(format!(
                    "Data Exchange export {} description is too long",
                    export.id
                ));
            }
            validate_inline_data_schema(&export.schema)?;
        }

        let mut imports = std::collections::HashSet::new();
        for import in &exchange.imports {
            validate_tapp_id(&import.tapp_id)?;
            if !valid_data_exchange_id(&import.export_id)
                || !imports.insert((import.tapp_id.as_str(), import.export_id.as_str()))
            {
                return Err(format!(
                    "Invalid or duplicate Data Exchange import: {} / {}",
                    import.tapp_id, import.export_id
                ));
            }
        }
    }

    if let Some(ai) = &manifest.ai {
        if ai.protocol_version != 2 {
            return Err("Tapp AI protocolVersion must be 2".to_string());
        }
        if ai.operations.is_empty() || ai.operations.len() > MAX_AI_OPERATIONS {
            return Err(format!(
                "Tapp AI operations must contain 1-{MAX_AI_OPERATIONS} entries"
            ));
        }
        if ai.output_formats.is_empty() || ai.output_formats.len() > 3 {
            return Err("Tapp AI outputFormats must contain 1-3 entries".to_string());
        }

        let mut operations = std::collections::HashSet::new();
        for operation in &ai.operations {
            if !operations.insert(*operation) {
                return Err("Tapp AI operations contains duplicates".to_string());
            }
            let permission = operation.permission();
            if !manifest.permissions.iter().any(|value| value == permission) {
                return Err(format!(
                    "Tapp AI operation requires manifest permission {permission}"
                ));
            }
        }

        let mut context_sources = std::collections::HashSet::new();
        if ai.context_sources.len() > 4
            || ai
                .context_sources
                .iter()
                .any(|source| !context_sources.insert(*source))
        {
            return Err(
                "Tapp AI contextSources contains duplicates or too many entries".to_string(),
            );
        }
        if context_sources.contains(&TappAiContextSource::Platform)
            && !manifest
                .permissions
                .iter()
                .any(|value| value == "platform:read")
        {
            return Err("Tapp AI platform context requires platform:read".to_string());
        }
        if context_sources.contains(&TappAiContextSource::Report)
            && !manifest
                .permissions
                .iter()
                .any(|value| value == "report:read")
        {
            return Err("Tapp AI report context requires report:read".to_string());
        }

        let mut output_formats = std::collections::HashSet::new();
        if ai
            .output_formats
            .iter()
            .any(|format| !output_formats.insert(*format))
        {
            return Err("Tapp AI outputFormats contains duplicates".to_string());
        }
        if operations.contains(&TappAiOperation::Image)
            && !output_formats.contains(&TappAiOutputFormat::Image)
        {
            return Err("Tapp AI image operation requires image output format".to_string());
        }
        if operations.contains(&TappAiOperation::Search)
            && !output_formats.contains(&TappAiOutputFormat::Json)
        {
            return Err("Tapp AI search operation requires json output format".to_string());
        }
    }

    if let Some(events) = &manifest.events {
        if events.publish.len() > 100 || events.subscribe.len() > 100 {
            return Err("Tapp events publish/subscribe accept at most 100 topics".to_string());
        }
        let publish_prefix = format!("tapp.{}.", manifest.id);
        let mut publish_topics = std::collections::HashSet::new();
        for topic in &events.publish {
            if !valid_event_topic(topic)
                || !topic.starts_with(&publish_prefix)
                || !publish_topics.insert(topic.as_str())
            {
                return Err(format!(
                    "Invalid or duplicate Tapp event publish topic: {topic}"
                ));
            }
        }
        let mut subscribe_topics = std::collections::HashSet::new();
        for topic in &events.subscribe {
            if !valid_event_topic(topic)
                || (!topic.starts_with("tapp.") && !topic.starts_with("system."))
                || !subscribe_topics.insert(topic.as_str())
            {
                return Err(format!(
                    "Invalid or duplicate Tapp event subscribe topic: {topic}"
                ));
            }
        }
        let declares =
            |permission: &str| manifest.permissions.iter().any(|value| value == permission);
        if !events.publish.is_empty() && !declares("event:publish") {
            return Err("Tapp event publish topics require event:publish".to_string());
        }
        if !events.subscribe.is_empty() && !declares("event:subscribe") {
            return Err("Tapp event subscribe topics require event:subscribe".to_string());
        }
    }

    if let Some(agent) = &manifest.agent {
        if agent.protocol_version != 2 {
            return Err("Tapp agent protocolVersion must be 2".to_string());
        }
        if agent.interactions.is_empty() || agent.interactions.len() > 32 {
            return Err("Tapp agent interactions must contain 1-32 entries".to_string());
        }
        let mut interaction_types = std::collections::HashSet::new();
        for interaction in &agent.interactions {
            if !valid_agent_name(&interaction.interaction_type)
                || !interaction_types.insert(interaction.interaction_type.as_str())
            {
                return Err(format!(
                    "Invalid or duplicate Agent interaction type: {}",
                    interaction.interaction_type
                ));
            }
            for schema in [
                interaction.input_schema.as_deref(),
                interaction.result_schema.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                validate_resource_path(schema)?;
                if !schema.ends_with(".json") {
                    return Err(format!("Agent schema must be a JSON resource: {schema}"));
                }
            }
        }
        if agent.intents.len() > 16 {
            return Err("Tapp agent intents accepts at most 16 entries".to_string());
        }
        let mut intents = std::collections::HashSet::new();
        for intent in &agent.intents {
            if !matches!(
                intent.as_str(),
                "ui.open" | "report.create" | "dataExchange.request"
            ) || !intents.insert(intent.as_str())
            {
                return Err(format!("Invalid or duplicate Agent intent: {intent}"));
            }
        }
    }

    validate_open_urls(manifest)?;
    Ok(())
}

pub fn validate_named_resource_keys<'a>(
    keys: impl IntoIterator<Item = &'a String>,
    kind: &str,
) -> Result<(), String> {
    for key in keys {
        if !is_safe_path_component(key) {
            return Err(format!("Invalid {kind}: {key}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn current() -> semver::Version {
        semver::Version::new(0, 4, 0)
    }

    fn manifest(permissions: &[&str]) -> TappManifest {
        serde_json::from_value(json!({
            "id": "com.example.permissions",
            "name": "Permissions test",
            "version": "1.0.0",
            "core": { "entry": "main.js" },
            "category": "utility",
            "permissions": permissions,
        }))
        .unwrap()
    }

    #[test]
    fn shipped_manifest_validator_uses_permission_catalog() {
        let err = validate_tapp_manifest(&manifest(&["storage"]), &current()).unwrap_err();
        assert!(err.contains("'storage'"), "{err}");
        assert!(err.contains("storage:read"), "{err}");
        assert!(TappPermission::from_str("storage").is_none());
        assert!(validate_tapp_manifest(&manifest(&["storage:read"]), &current()).is_ok());
    }

    #[test]
    fn named_resource_keys_require_safe_components() {
        assert!(
            validate_named_resource_keys([&"page".to_string(), &"schemas".to_string()], "dir")
                .is_ok()
        );
        assert!(validate_named_resource_keys([&"../x".to_string()], "dir").is_err());
    }

    #[test]
    fn validate_module_has_no_reqwest() {
        let src = include_str!("validate.rs");
        let production = src.split("#[cfg(test)]").next().expect("source");
        assert!(!production.contains("reqwest::"));
        assert!(!production.contains("tracing::"));
        assert!(!production.contains("env!"));
    }
}
