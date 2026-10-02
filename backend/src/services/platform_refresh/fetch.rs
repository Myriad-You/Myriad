//! Dispatcher: credential gate + per-platform fetch arms.

use super::arms_core;
use super::arms_extended;
use super::cache::{load_platform_data_cache, save_platform_data_cache};
use super::clean::clean_platform_data;
use super::errors::{platform_data_warning, resolve_platform_fetch_message};
use crate::config::DynamicConfig;
use crate::services::fetcher::PlatformFetcher;
use crate::services::metadata_service::MetadataService;
use crate::services::platform_id::PlatformId;
use crate::services::site_owner::site_owner_user_id;
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use std::collections::HashMap;

/// 一次抓取的结果：合并后的平台数据 + 各平台远程错误（保留旧缓存时也会记录）。
#[derive(Debug, Clone)]
pub struct FreshPlatformData {
    pub data: Value,
    /// platform id → `{stage}: {error}` (concatenated stages), not a raw remote body.
    pub errors: HashMap<String, String>,
}

pub(super) struct FetchCtx<'a> {
    pub fetcher: &'a PlatformFetcher,
    pub config: &'a DynamicConfig,
    pub all_data: &'a mut Value,
    pub fetch_errors: &'a mut HashMap<String, String>,
    pub metadata_service: &'a MetadataService,
    pub user_id: i32,
    pub db: &'a DatabaseConnection,
}

/// Platforms whose fetch arm would run. Fetch/refresh needs configured credentials
/// ([`PlatformId::credentials_present`]); it does not read `*_enabled` (that flag
/// also gates report generation, public cards, Agent connection, Steam presence).
pub fn configured_platform_ids(config: &DynamicConfig) -> Vec<&'static str> {
    refresh_plan(config, None)
        .into_iter()
        .map(PlatformId::slug)
        .collect()
}

/// 刷新单个平台数据（抓取函数自己落盘，这里只把结果翻译成调度器要的 Ok / Err）
pub async fn refresh_platform_for_scheduler(
    db: &DatabaseConnection,
    platform: &str,
) -> Result<Value, String> {
    let outcome = fetch_fresh_platform_data(db, Some(platform))
        .await
        .map_err(|error| error.to_string())?;
    let remote_err = outcome.errors.get(platform).map(String::as_str);
    if let Some(msg) =
        resolve_platform_fetch_message(platform, outcome.data.get(platform), remote_err)
    {
        // 无可用数据（platform_data_warning Some）时返回 Err；有可用数据则 warn 并 Ok
        if platform_data_warning(platform, outcome.data.get(platform)).is_some() {
            return Err(msg);
        }
        tracing::warn!(
            "Platform {} refresh warning (stale kept): {}",
            platform,
            msg
        );
    }
    Ok(outcome.data.get(platform).cloned().unwrap_or(Value::Null))
}

/// 本次要抓的平台：`target` 指定的那一个，或全部；都只取凭据齐备的。
fn refresh_plan(config: &DynamicConfig, target: Option<&str>) -> Vec<PlatformId> {
    PlatformId::ALL
        .into_iter()
        .filter(|id| target.is_none_or(|t| t == id.slug()))
        .filter(|id| id.credentials_present(config))
        .collect()
}

/// 合并底：磁盘上的旧缓存。全量刷新就是逐个平台跑单平台流程，每个平台都从
/// 这里出发，抓取臂只覆盖成功的子键，失败的子请求（如 GitHub user 超时）保留
/// 旧值，不会把整份平台数据换成残缺对象。
fn merge_base(previous: Option<Value>) -> Value {
    previous
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

/// 抓取臂跑完之后：清洗整棵树，返回计划内有数据（要落盘、要过滤）的平台。
fn finish_merge(data: &mut Value, plan: &[PlatformId]) -> Vec<&'static str> {
    // 部分平台树 allowlist/truncate（非 5W1H）
    clean_platform_data(data);
    plan.iter()
        .map(|id| id.slug())
        .filter(|slug| data.get(slug).is_some_and(|v| !v.is_null()))
        .collect()
}

/// 要落盘的原始数据：只含本次计划内有数据的平台。
fn refreshed_subset(data: &Value, refreshed: &[&str]) -> Value {
    Value::Object(
        refreshed
            .iter()
            .filter_map(|slug| data.get(slug).map(|v| (slug.to_string(), v.clone())))
            .collect(),
    )
}

async fn run_arm(id: PlatformId, ctx: &mut FetchCtx<'_>) {
    match id {
        // GitHub（含仓库信息）
        PlatformId::Github => arms_core::fetch_github(ctx).await,
        PlatformId::Bilibili => arms_core::fetch_bilibili(ctx).await,
        // Steam（只保留游玩时间>=3小时的游戏）
        PlatformId::Steam => arms_core::fetch_steam(ctx).await,
        // YouTube（公开频道；API key + channel id/handle，无 OAuth）
        PlatformId::Youtube => arms_extended::fetch_youtube(ctx).await,
        PlatformId::Netease => arms_core::fetch_netease(ctx).await,
        // Bangumi 收藏数据
        PlatformId::Bangumi => arms_core::fetch_bangumi(ctx).await,
        PlatformId::X => arms_extended::fetch_x(ctx).await,
        // Discord（用户 OAuth：画像 + 服务器 + 连接）
        PlatformId::Discord => arms_extended::fetch_discord(ctx).await,
        // MyAnimeList（双模式：有 client_id 走官方 API，否则公开 load.json）
        PlatformId::Mal => arms_extended::fetch_mal(ctx).await,
        // Xbox（成就向：Gamerscore + 各游戏成就进度）
        PlatformId::Xbox => arms_extended::fetch_xbox(ctx).await,
        // PSN（奖杯向：奖杯等级 + 各游戏奖杯完成度）
        PlatformId::Psn => arms_extended::fetch_psn(ctx).await,
    }
}

/// 抓取并落盘（always remote）。`target_platform` Some = 只抓这一个平台，None = 全部
/// 凭据齐备的平台；两者走同一条流程：旧缓存作合并底 → 抓取 → 清洗 → 保存原始数据
/// → 逐平台智能过滤。返回的 `data` 是合并后的整棵树。
pub async fn fetch_fresh_platform_data(
    db: &DatabaseConnection,
    target_platform: Option<&str>,
) -> Result<FreshPlatformData, Box<dyn std::error::Error>> {
    tracing::info!(
        "Starting fetch platform data (target: {:?})...",
        target_platform
    );

    let fetcher = PlatformFetcher::new().await;
    let user_id = site_owner_user_id(db)
        .await
        .map_err(std::io::Error::other)?;

    // 获取动态配置
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    let plan = refresh_plan(&config, target_platform);
    let mut all_data = merge_base(load_platform_data_cache().map(|c| c.data));
    let mut fetch_errors: HashMap<String, String> = HashMap::new();

    // 创建元数据服务
    let metadata_service = MetadataService::new(db.clone());

    {
        let mut ctx = FetchCtx {
            fetcher: &fetcher,
            config: &config,
            all_data: &mut all_data,
            fetch_errors: &mut fetch_errors,
            metadata_service: &metadata_service,
            user_id,
            db,
        };
        for &id in &plan {
            run_arm(id, &mut ctx).await;
        }
    }
    drop(config);
    let refreshed = finish_merge(&mut all_data, &plan);

    for id in &plan {
        if !refreshed.contains(&id.slug()) {
            tracing::warn!("No data for platform {} after fetch; nothing saved", id);
        }
    }
    if let Err(e) = save_platform_data_cache(&refreshed_subset(&all_data, &refreshed)) {
        tracing::error!("Failed to save platform cache: {}", e);
    }
    for platform in &refreshed {
        if let Err(e) = crate::services::smart_filter::SmartFilter::process_and_save_single(
            platform,
            &all_data[*platform],
        ) {
            tracing::error!(
                "Failed to update smart filter cache for {}: {}",
                platform,
                e
            );
        }
    }

    // 平台画像可能换了（站长在 B站换了头像）——重算站长快照，让 /api/auth/me 等
    // 单查询出口也能跟着变；平台画像藏在 platform_metadata 的 JSON 里，SQL 阶梯够不到。
    crate::services::avatar::refresh_avatar_snapshot(db, user_id).await;

    Ok(FreshPlatformData {
        data: all_data,
        errors: fetch_errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand-in for the fetch arms: GitHub user endpoint times out, repos
    /// succeed; Steam fails outright. Arms only assign the sub-keys that succeeded.
    fn partial_failure_arm(id: PlatformId, data: &mut Value) {
        if id == PlatformId::Github {
            data["github"]["repos"] = json!([{ "name": "new" }]);
        }
    }

    /// Same steps as `fetch_fresh_platform_data`, minus the network.
    fn refresh(previous: Option<Value>, plan: &[PlatformId]) -> (Value, Vec<&'static str>) {
        let mut data = merge_base(previous);
        for &id in plan {
            partial_failure_arm(id, &mut data);
        }
        let refreshed = finish_merge(&mut data, plan);
        (data, refreshed)
    }

    fn previous_cache() -> Value {
        json!({
            "github": {
                "user": { "login": "me", "avatar_url": "https://a/me.png", "bio": "hi" },
                "repos": [{ "name": "old" }],
            },
            "steam": { "user": { "personaname": "me" }, "games": [] },
        })
    }

    #[test]
    fn full_and_single_refresh_merge_the_same_way_on_partial_failure() {
        let (full, full_refreshed) = refresh(
            Some(previous_cache()),
            &[PlatformId::Github, PlatformId::Steam],
        );
        let (single, single_refreshed) = refresh(Some(previous_cache()), &[PlatformId::Github]);

        assert_eq!(full["github"], single["github"]);
        assert_eq!(full["github"]["user"]["avatar_url"], "https://a/me.png");
        assert_eq!(full["github"]["user"]["bio"], "hi");
        assert_eq!(full["github"]["repos"][0]["name"], "new");
        assert_eq!(full["steam"], previous_cache()["steam"]);

        assert_eq!(full_refreshed, vec!["github", "steam"]);
        assert_eq!(single_refreshed, vec!["github"]);
        let full_saved = refreshed_subset(&full, &full_refreshed);
        let single_saved = refreshed_subset(&single, &single_refreshed);
        assert_eq!(full_saved["github"], single_saved["github"]);
        assert!(single_saved.get("steam").is_none());
    }

    #[test]
    fn platform_without_previous_or_fresh_data_is_not_saved() {
        for previous in [None, Some(json!([])), Some(json!({}))] {
            let (data, refreshed) = refresh(previous, &[PlatformId::Github, PlatformId::Steam]);
            assert_eq!(refreshed, vec!["github"]);
            assert!(data["github"].get("user").is_none());
            let saved = refreshed_subset(&data, &refreshed);
            assert_eq!(saved.as_object().map(|o| o.len()), Some(1));
        }
    }

    /// The fetch path runs the same steps as `refresh` above: one merge base
    /// for every plan, no empty-object branch for full refresh.
    #[test]
    fn fetch_uses_the_shared_merge_steps() {
        let body = include_str!("fetch.rs")
            .split("pub async fn fetch_fresh_platform_data")
            .nth(1)
            .and_then(|rest| rest.split("#[cfg(test)]").next())
            .expect("fetch_fresh_platform_data");
        assert!(body.contains("merge_base(load_platform_data_cache()"));
        assert!(body.contains("finish_merge(&mut all_data, &plan)"));
        assert!(body.contains("refreshed_subset(&all_data, &refreshed)"));
        assert!(!body.contains("target_platform.is_some()"));
    }

    #[test]
    fn refresh_plan_is_target_or_all_with_credentials() {
        let mut config = DynamicConfig::default();
        config.github_username = Some("me".into());
        config.bilibili_uid = Some("1".into());
        assert_eq!(
            refresh_plan(&config, None),
            vec![PlatformId::Github, PlatformId::Bilibili]
        );
        assert_eq!(
            refresh_plan(&config, Some("github")),
            vec![PlatformId::Github]
        );
        assert!(refresh_plan(&config, Some("steam")).is_empty());
        assert!(refresh_plan(&config, Some("GitHub")).is_empty());
    }
}
