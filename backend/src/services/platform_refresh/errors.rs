//! User-facing fetch error copy (humanize / warning / resolve).

use serde_json::Value;
use std::collections::HashMap;

/// 记录某平台抓取错误（主/副接口均可）；同平台多次失败会拼接，避免覆盖。
pub(super) fn note_fetch_error(
    errors: &mut HashMap<String, String>,
    platform: &str,
    stage: &str,
    error: impl ToString,
) {
    let detail = {
        let raw = error.to_string();
        if stage.is_empty() {
            raw
        } else {
            format!("{stage}: {raw}")
        }
    };
    errors
        .entry(platform.to_string())
        .and_modify(|existing| {
            if !existing.contains(&detail) {
                existing.push_str("; ");
                existing.push_str(&detail);
            }
        })
        .or_insert(detail);
}

fn platform_label(platform: &str, locale: &str) -> String {
    if platform.eq_ignore_ascii_case("netease") {
        return pick_msg(locale, "网易云音乐", "网易雲音楽", "NetEase Cloud Music").to_string();
    }
    match platform {
        "github" => "GitHub",
        "bilibili" => "Bilibili",
        "steam" => "Steam",
        "bangumi" => "Bangumi",
        "x" => "X",
        "discord" => "Discord",
        "mal" => "MyAnimeList",
        "xbox" => "Xbox",
        "psn" => "PSN",
        "youtube" => "YouTube",
        other => other,
    }
    .to_string()
}

fn pick_msg<'a>(locale: &str, zh: &'a str, ja: &'a str, en: &'a str) -> &'a str {
    let tag = locale.to_ascii_lowercase();
    if tag.starts_with("ja") {
        ja
    } else if tag.starts_with("en") {
        en
    } else {
        zh
    }
}

/// 将底层抓取错误转成面向用户的说明（含 X 402、通用鉴权/限流等）。
pub fn humanize_platform_fetch_error_for(platform: &str, error: &str, locale: &str) -> String {
    let lower = error.to_ascii_lowercase();
    let label = platform_label(platform, locale);

    // X 按量计费额度
    if platform.eq_ignore_ascii_case("x")
        && (lower.contains("402")
            || lower.contains("credits depleted")
            || lower.contains("payment required")
            || lower.contains("creditsdepleted"))
    {
        return pick_msg(
            locale,
            "X API 额度已耗尽（HTTP 402）。请到 developer.x.com 充值后再刷新。",
            "X API のクレジットが不足しています（HTTP 402）。developer.x.com でチャージしてから再取得してください。",
            "X API credits are depleted (HTTP 402). Top up at developer.x.com, then refresh.",
        )
        .to_string();
    }

    if lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("quota")
    {
        return format!(
            "{}{}",
            label,
            pick_msg(
                locale,
                " 请求过于频繁或额度不足。请稍后再试，或检查 API 配额。",
                " のリクエストが多すぎるか、枠が足りません。しばらくしてから再試行するか、API 枠を確認してください。",
                " is rate-limited or out of quota. Try again later, or check the API quota.",
            )
        );
    }

    if lower.contains("401")
        || lower.contains("unauthorized")
        || lower.contains("invalid token")
        || lower.contains("bad credentials")
        || lower.contains("invalid_grant")
    {
        return format!(
            "{}{}",
            label,
            pick_msg(
                locale,
                " 鉴权失败。请检查 Token / API Key / Cookie 是否有效或已过期。",
                " の認証に失敗しました。Token / API Key / Cookie が有効か確認してください。",
                " authentication failed. Check that the token, API key, or cookie is still valid.",
            )
        );
    }

    if lower.contains("403") || lower.contains("forbidden") || lower.contains("access denied") {
        return format!(
            "{}{}",
            label,
            pick_msg(
                locale,
                " 拒绝访问。常见原因：资料未公开、权限不足、或接口风控。",
                " へのアクセスが拒否されました。公開設定・権限・アクセス制限を確認してください。",
                " denied access. The profile may be private, the token scope too narrow, or the request blocked.",
            )
        );
    }

    if lower.contains("404") || lower.contains("not found") {
        return format!(
            "{}{}",
            label,
            pick_msg(
                locale,
                " 未找到目标。请确认用户名 / ID 配置正确。",
                " の対象が見つかりません。ユーザー名 / ID を確認してください。",
                " could not find that account. Check the username or ID.",
            )
        );
    }

    if lower.contains("timeout") || lower.contains("timed out") || lower.contains("connect") {
        return format!(
            "{}{}",
            label,
            pick_msg(
                locale,
                " 网络超时。请稍后重试。",
                " の通信がタイムアウトしました。しばらくしてから再試行してください。",
                " timed out. Please try again.",
            )
        );
    }

    format!(
        "{}{}",
        label,
        pick_msg(
            locale,
            " 抓取失败。请稍后重试。",
            " の取得に失敗しました。しばらくしてから再試行してください。",
            " could not be fetched. Please try again.",
        )
    )
}

/// 综合「远程错误」与「数据是否为空」给出最终用户提示。
/// - 远程失败且无可用数据 → 优先展示真实错误（如 402）
/// - 远程部分失败但仍有可用数据 → 提示失败点，并说明仍有可用数据
/// - 无远程错误但数据空 → 原有 platform_data_warning
pub fn resolve_platform_fetch_message(
    platform: &str,
    data: Option<&Value>,
    remote_error: Option<&str>,
) -> Option<String> {
    resolve_platform_fetch_message_for(platform, data, remote_error, "zh-CN")
}

pub fn resolve_platform_fetch_message_for(
    platform: &str,
    data: Option<&Value>,
    remote_error: Option<&str>,
    locale: &str,
) -> Option<String> {
    let has_usable = platform_data_warning_for(platform, data, locale).is_none();

    match (remote_error, has_usable) {
        (Some(err), false) => Some(humanize_platform_fetch_error_for(platform, err, locale)),
        (Some(err), true) => Some(format!(
            "{}{}",
            humanize_platform_fetch_error_for(platform, err, locale),
            pick_msg(
                locale,
                "（仍有部分可用数据，请查看详情后重试失败项）",
                "（一部のデータは残っています。失敗した項目を確認して再試行してください）",
                " (some data is still available — retry the failed parts)",
            )
        )),
        (None, false) => platform_data_warning_for(platform, data, locale),
        (None, true) => None,
    }
}

/// 空载荷时的用户提示（默认 zh-CN）。

pub fn platform_data_warning(platform: &str, data: Option<&Value>) -> Option<String> {
    platform_data_warning_for(platform, data, "zh-CN")
}

pub fn platform_data_warning_for(
    platform: &str,
    data: Option<&Value>,
    locale: &str,
) -> Option<String> {
    let Some(data) = data.filter(|v| !v.is_null()) else {
        return Some(format!(
            "{}{}",
            platform_label(platform, locale),
            pick_msg(
                locale,
                " 未返回任何数据。请确认该平台已启用且账号/令牌配置正确。",
                " からデータを取得できませんでした。有効化とアカウント設定を確認してください。",
                " returned no data. Check that the platform is enabled and the account is configured.",
            )
        ));
    };

    // 各平台核心数组为空时给出针对性提示
    let is_empty_array = |key: &str| {
        data.get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
    };

    match platform {
        "bangumi" => is_empty_array("collections").then(|| {
            pick_msg(
                locale,
                "Bangumi 收藏为空。可能是收藏设为私密、用户名/访问令牌不正确，或该账号确实没有收藏。",
                "Bangumi のコレクションが空です。非公開設定、ユーザー名/トークン、または未登録の可能性があります。",
                "Bangumi collections are empty. The list may be private, the username/token wrong, or the account has no collections.",
            )
            .to_string()
        }),
        "mal" => {
            let anime_empty = is_empty_array("anime_list");
            let manga_empty = is_empty_array("manga_list");
            (anime_empty && manga_empty).then(|| {
                pick_msg(
                    locale,
                    "MyAnimeList 列表为空。请确认用户名正确；公开列表模式需将列表设为公开，或配置可选 Client ID 使用官方 API。",
                    "MyAnimeList のリストが空です。ユーザー名と公開設定、または公式 API の Client ID を確認してください。",
                    "MyAnimeList lists are empty. Check the username; public-list mode needs a public list, or configure the optional Client ID.",
                )
                .to_string()
            })
        }
        "steam" => is_empty_array("games").then(|| {
            pick_msg(
                locale,
                "Steam 未返回游戏数据。请确认 API Key、SteamID 正确且个人资料设为公开。",
                "Steam からゲームデータを取得できませんでした。API Key、SteamID、プロフィール公開設定を確認してください。",
                "Steam returned no games. Check the API key, SteamID, and that the profile is public.",
            )
            .to_string()
        }),
        "bilibili" => {
            let no_user = data
                .get("user")
                .or_else(|| data.get("user_info"))
                .filter(|v| !v.is_null())
                .is_none();
            let no_content = is_empty_array("favorites") && is_empty_array("bangumi");
            if no_user && no_content {
                Some(
                    pick_msg(
                        locale,
                        "Bilibili 未返回用户与内容数据。请确认 UID 正确；用户接口受风控时请稍后重试。",
                        "Bilibili からユーザーとコンテンツを取得できませんでした。UID を確認し、制限中なら後で再試行してください。",
                        "Bilibili returned no user or content data. Check the UID; retry later if the API is rate-limiting.",
                    )
                    .to_string(),
                )
            } else if no_user {
                Some(
                    pick_msg(
                        locale,
                        "Bilibili 用户信息未取到（追番/收藏可能仍有数据）。常见原因：接口风控；请重新刷新。",
                        "Bilibili のユーザー情報を取得できませんでした（視聴/お気に入りは残っている場合があります）。制限中なら再取得してください。",
                        "Bilibili user info is missing (shows/favorites may still be present). This is often rate-limiting — refresh again.",
                    )
                    .to_string(),
                )
            } else {
                None
            }
        }
        "github" => data
            .get("user")
            .filter(|v| !v.is_null())
            .is_none()
            .then(|| {
                pick_msg(
                    locale,
                    "GitHub 未返回用户数据。请检查用户名与令牌。",
                    "GitHub からユーザーデータを取得できませんでした。ユーザー名とトークンを確認してください。",
                    "GitHub returned no user data. Check the username and token.",
                )
                .to_string()
            }),
        "x" => data
            .get("user")
            .filter(|v| !v.is_null())
            .is_none()
            .then(|| {
                pick_msg(
                    locale,
                    "X 未返回用户数据。请检查用户名、Bearer Token 以及 API 套餐权限。",
                    "X からユーザーデータを取得できませんでした。ユーザー名、Bearer Token、API プランを確認してください。",
                    "X returned no user data. Check the username, bearer token, and API plan.",
                )
                .to_string()
            }),
        "discord" => data
            .get("user")
            .filter(|v| !v.is_null())
            .is_none()
            .then(|| {
                pick_msg(
                    locale,
                    "Discord 未返回用户数据。请检查 Access Token 是否有效，且 scope 含 identify / guilds / connections。",
                    "Discord からユーザーデータを取得できませんでした。Access Token と identify / guilds / connections スコープを確認してください。",
                    "Discord returned no user data. Check the access token and that it includes identify / guilds / connections.",
                )
                .to_string()
            }),
        "xbox" => data
            .pointer("/achievements/titles")
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true)
            .then(|| {
                pick_msg(
                    locale,
                    "Xbox 未返回成就数据。请确认 Gamertag、OpenXBL API Key 正确且资料设为公开。",
                    "Xbox から実績データを取得できませんでした。Gamertag、OpenXBL API Key、公開設定を確認してください。",
                    "Xbox returned no achievements. Check the gamertag, OpenXBL API key, and that the profile is public.",
                )
                .to_string()
            }),
        "psn" => is_empty_array("trophy_titles").then(|| {
            pick_msg(
                locale,
                "PSN 未返回奖杯数据。请确认 Online ID、NPSSO 有效且奖杯设为公开。",
                "PSN からトロフィーを取得できませんでした。Online ID、NPSSO、公開設定を確認してください。",
                "PSN returned no trophies. Check the Online ID, NPSSO, and that trophies are public.",
            )
            .to_string()
        }),
        // YouTube: channel present with 0 videos is a valid empty public channel —
        // never treat as fetch failure. Only warn when channel object is missing.
        "youtube" => data
            .get("channel")
            .filter(|v| !v.is_null() && v.get("id").is_some())
            .is_none()
            .then(|| {
                pick_msg(
                    locale,
                    "YouTube 未返回频道数据。请确认 API Key 有效，且 Channel ID / @handle 正确。",
                    "YouTube からチャンネルデータを取得できませんでした。API Key と Channel ID / @handle を確認してください。",
                    "YouTube returned no channel data. Check the API key and Channel ID / @handle.",
                )
                .to_string()
            }),
        "netease" => data
            .get("profile")
            .filter(|v| !v.is_null())
            .is_none()
            .then(|| {
                pick_msg(
                    locale,
                    "网易云未返回用户资料。请确认用户 ID 正确；接口受风控时请稍后重试。",
                    "网易雲からプロフィールを取得できませんでした。ユーザー ID を確認し、制限中なら後で再試行してください。",
                    "NetEase returned no profile. Check the user ID; retry later if the API is rate-limiting.",
                )
                .to_string()
            }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn humanize_x_402_credits_depleted() {
        let msg = humanize_platform_fetch_error_for(
            "x",
            "X API error (402 Payment Required): credits depleted",
            "zh-CN",
        );
        assert!(msg.contains("额度"), "{msg}");
        assert!(msg.contains("402") || msg.contains("Credits"), "{msg}");
    }

    #[test]
    fn resolve_prefers_remote_error_when_empty() {
        let msg = resolve_platform_fetch_message(
            "x",
            None,
            Some("X API error (402 Payment Required): credits depleted"),
        )
        .unwrap();
        assert!(msg.contains("额度"), "{msg}");
        assert!(!msg.contains("未返回任何数据"), "{msg}");
    }

    #[test]
    fn resolve_keeps_usable_note_when_data_present() {
        let data = json!({"user": {"id": "1", "username": "hitomi"}, "tweets": []});
        let msg = resolve_platform_fetch_message(
            "x",
            Some(&data),
            Some("X API error (402 Payment Required): credits depleted"),
        )
        .unwrap();
        assert!(msg.contains("可用数据"), "{msg}");
    }

    #[test]
    fn humanize_rate_limit_and_auth_generic() {
        let r = humanize_platform_fetch_error_for("steam", "HTTP 429 Too Many Requests", "zh-CN");
        assert!(r.contains("频繁") || r.contains("配额"), "{r}");
        assert!(!r.contains("HTTP 429"), "{r}");
        let a = humanize_platform_fetch_error_for(
            "github",
            "401 Unauthorized: Bad credentials",
            "zh-CN",
        );
        assert!(a.contains("鉴权"), "{a}");
        let en = humanize_platform_fetch_error_for("steam", "HTTP 429 Too Many Requests", "en-US");
        assert!(en.contains("rate-limited") || en.contains("quota"), "{en}");
        assert!(!en.contains("频繁"), "{en}");
    }

    #[test]
    fn note_fetch_error_appends_stages() {
        let mut map = std::collections::HashMap::new();
        note_fetch_error(&mut map, "steam", "user", "boom");
        note_fetch_error(&mut map, "steam", "games", "nope");
        let v = map.get("steam").unwrap();
        assert!(v.contains("user: boom"), "{v}");
        assert!(v.contains("games: nope"), "{v}");
    }

    #[test]
    fn platform_data_warning_none_when_payload_present() {
        let data = json!({"games": [{"appid": 1}], "user": {"name": "x"}});
        assert!(platform_data_warning("steam", Some(&data)).is_none());
    }

    #[test]
    fn platform_data_warning_when_missing_or_null() {
        assert!(platform_data_warning("steam", None)
            .unwrap()
            .contains("未返回"));
        assert!(platform_data_warning("steam", Some(&Value::Null))
            .unwrap()
            .contains("未返回"));
    }

    #[test]
    fn platform_data_warning_steam_empty_games() {
        let data = json!({"games": []});
        let w = platform_data_warning("steam", Some(&data)).unwrap();
        assert!(w.contains("Steam"), "{w}");
    }

    #[test]
    fn platform_data_warning_bangumi_empty_collections() {
        let data = json!({"collections": []});
        let w = platform_data_warning("bangumi", Some(&data)).unwrap();
        assert!(w.contains("Bangumi"), "{w}");
    }

    #[test]
    fn platform_data_warning_mal_empty_lists() {
        let data = json!({"anime_list": [], "manga_list": []});
        let w = platform_data_warning("mal", Some(&data)).unwrap();
        assert!(w.contains("MyAnimeList"), "{w}");
    }
}
