use super::super::HandlerContext;
use super::brew::{
    execute_brew_article, execute_brew_items, execute_brew_read, execute_brew_sources,
    execute_brew_stats,
};
use super::brew_generate::execute_brew_generate_reading_list;
use super::catalog::{
    execute_context_reference, execute_database_query, execute_heartbeat_list,
    execute_metadata_history, execute_platform_connection, execute_profile_summary,
    execute_random_content, execute_report_list, execute_rsshub_instances, execute_scheduler_list,
    execute_search_global, execute_stats_overview, execute_tapp_list, execute_task_status,
};
use super::config_time_auth::{execute_auth_status, execute_config_get, execute_time_info};
use super::extras_platform::{
    execute_bilibili_bangumi, execute_github_repos, execute_netease_playlist,
    execute_netease_search_playlist, execute_steam_wishlist,
};
use super::pages::{execute_brew_page_content, execute_tapp_page_content};
use super::permission::{execute_permission_check, execute_tapp_widget};
use super::platform::{execute_platform_read, execute_platform_stats};
use super::rsshub::execute_brew_discover;
use super::search::execute_fuzzy_search;
use serde_json::Value;
use std::collections::HashMap;

pub async fn execute(
    capability_id: &str,
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    match capability_id {
        "platform.read" => execute_platform_read(params).await,
        "platform.stats" => execute_platform_stats(params).await,
        "brew.read" => execute_brew_read(params, ctx).await,
        "brew.sources" => execute_brew_sources(params, ctx).await,
        "brew.items" => execute_brew_items(params, ctx).await,
        "brew.article" => execute_brew_article(params, ctx).await,
        "brew.stats" => execute_brew_stats(params, ctx).await,
        "brew.discover" => execute_brew_discover(params, ctx).await,
        "brew.page" => execute_brew_page_content(params, ctx).await,
        "brew.generateReadingList" => execute_brew_generate_reading_list(params, ctx).await,
        "tapp.page" => execute_tapp_page_content(params, ctx).await,
        "search.fuzzy" => execute_fuzzy_search(params, ctx).await,
        "config.get" => execute_config_get(params).await,
        "time.info" => execute_time_info(params).await,
        "auth.status" => execute_auth_status(ctx).await,
        // 音乐平台
        "netease.playlist" => execute_netease_playlist(params).await,
        "netease.searchPlaylist" => execute_netease_search_playlist(params, ctx).await,
        // GitHub
        "github.repos" => execute_github_repos(params).await,
        // 追加能力
        "bilibili.bangumi" => execute_bilibili_bangumi(params).await,
        "steam.wishlist" => execute_steam_wishlist(params).await,
        "tapp.widget" => execute_tapp_widget(params, ctx).await,
        "permission.check" => execute_permission_check(params, ctx).await,
        "platform.connection" => execute_platform_connection(params).await,
        "stats.overview" => execute_stats_overview(params).await,
        "profile.summary" => execute_profile_summary(params).await,
        "search.global" => execute_search_global(params).await,
        "task.status" => execute_task_status(params, ctx).await,
        "metadata.history" => execute_metadata_history(params).await,
        "tapp.list" => execute_tapp_list(params, ctx).await,
        "scheduler.list" => execute_scheduler_list(params, ctx).await,
        "heartbeat.list" => execute_heartbeat_list(params, ctx).await,
        "rsshub.instances" => execute_rsshub_instances(params, ctx).await,
        "context.reference" => execute_context_reference(params).await,
        // 补充的能力
        "database.anime" | "database.game" | "database.artist" => {
            execute_database_query(capability_id, params).await
        }
        "random.content" => execute_random_content(params).await,
        "report.list" => execute_report_list(params, ctx).await,
        _ => Err(format!("Unknown data_read capability: {}", capability_id)),
    }
}

#[cfg(test)]
mod dispatcher_contract_tests {
    /// Split must keep the same capability ids on the shipped `execute` match.
    #[test]
    fn execute_match_still_covers_pre_split_capability_ids() {
        let src = include_str!("execute.rs");
        let match_body = src
            .split("match capability_id {")
            .nth(1)
            .and_then(|rest| rest.split("\n    }").next())
            .expect("execute match");
        for id in [
            "platform.read",
            "platform.stats",
            "brew.read",
            "brew.sources",
            "brew.items",
            "brew.article",
            "brew.stats",
            "brew.discover",
            "brew.page",
            "brew.generateReadingList",
            "tapp.page",
            "search.fuzzy",
            "config.get",
            "time.info",
            "auth.status",
            "netease.playlist",
            "netease.searchPlaylist",
            "github.repos",
            "bilibili.bangumi",
            "steam.wishlist",
            "tapp.widget",
            "permission.check",
            "platform.connection",
            "stats.overview",
            "profile.summary",
            "search.global",
            "task.status",
            "metadata.history",
            "tapp.list",
            "scheduler.list",
            "heartbeat.list",
            "rsshub.instances",
            "context.reference",
            "database.anime",
            "database.game",
            "database.artist",
            "random.content",
            "report.list",
        ] {
            assert!(
                match_body.contains(&format!("\"{id}\"")),
                "dispatcher dropped {id}"
            );
        }
    }
}
