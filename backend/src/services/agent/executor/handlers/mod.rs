//! 能力执行处理器
//!
//! 按能力类别分发执行逻辑

mod ai_process;
mod data_read;
mod data_write;
mod external;
mod model3d;
mod resource_create;
mod system_op;
mod ui_control;

use crate::services::agent::types::*;
use crate::services::analyzer::AiAnalyzer;
use sea_orm::DatabaseConnection;
use serde_json::Value;
use std::collections::HashMap;

/// Handler 执行上下文
pub struct HandlerContext<'a> {
    pub db: &'a DatabaseConnection,
    pub ai_analyzer: Option<&'a AiAnalyzer>,
    pub user_id: i32,
    /// Executor-owned task identity. Capability input is never trusted to
    /// choose which task an asynchronous Tapp result will resume.
    pub task_id: Option<String>,
    /// 执行上下文快照（包含对话历史、角色身份等）
    pub execution_context: Option<ExecutionContext>,
    pub autonomy_permission_cap: Option<Vec<String>>,
}

/// 根据能力类别分发执行
pub async fn execute_capability(
    capability_id: &str,
    action: &str,
    category: &CapabilityCategory,
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    match category {
        CapabilityCategory::DataRead => data_read::execute(capability_id, params, ctx).await,
        CapabilityCategory::DataWrite => data_write::execute(capability_id, params, ctx).await,
        CapabilityCategory::AiProcess => {
            ai_process::execute(capability_id, action, params, ctx).await
        }
        CapabilityCategory::ResourceCreate => {
            if capability_id.starts_with("model3d.") {
                model3d::execute(capability_id, params, ctx).await
            } else {
                resource_create::execute(capability_id, params, ctx).await
            }
        }
        CapabilityCategory::SystemOp => system_op::execute(capability_id, params, ctx).await,
        CapabilityCategory::ExternalIntegration => {
            external::execute(capability_id, params, ctx).await
        }
        CapabilityCategory::UiControl => ui_control::execute(capability_id, params, ctx).await,
    }
}

#[cfg(test)]
mod coverage {
    use super::*;
    use crate::services::agent::capability::CapabilityRegistry;
    use std::collections::{HashMap, HashSet};

    /// Match-arm IDs grouped by the category `execute_capability` dispatches on.
    /// MCP tools (`mcp.*`) are dynamic and are not in the static registry.
    fn handler_ids_by_category() -> HashMap<CapabilityCategory, HashSet<&'static str>> {
        let mut map = HashMap::new();
        map.insert(
            CapabilityCategory::DataRead,
            HashSet::from([
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
            ]),
        );
        map.insert(
            CapabilityCategory::DataWrite,
            HashSet::from([
                "platform.write",
                "platform.refresh",
                "storage.set",
                "tapp.storage",
                "brew.subscribe",
                "brew.mark",
                "content.write",
            ]),
        );
        map.insert(
            CapabilityCategory::AiProcess,
            HashSet::from([
                "ai.summarize",
                "ai.analyze",
                "ai.recommend",
                "ai.chat",
                "ai.webSearch",
                "ai.groundingSearch",
                "brewlia.annotate",
                "brewlia.podcast",
                "speech.tts",
                "smart.filter",
                "compare.content",
                "icon.recommend",
                "prompt.generate",
                "translate.text",
                "code.explain",
                "ai.image",
            ]),
        );
        map.insert(
            CapabilityCategory::ResourceCreate,
            HashSet::from([
                "tapp.generate",
                "tapp.install",
                "report.create",
                "reminder.create",
                "note.create",
                "bookmark.save",
                "model3d.status",
                "model3d.generate",
                "model3d.rig",
                "model3d.retarget",
            ]),
        );
        map.insert(
            CapabilityCategory::SystemOp,
            HashSet::from([
                "data.transform",
                "scheduler.create",
                "scheduler.trigger",
                "heartbeat.create",
                "heartbeat.update",
                "heartbeat.delete",
                "heartbeat.toggle",
                "system.metrics",
                "cache.status",
                "cache.clear",
                "rsshub.healthcheck",
                "image.cache",
                "export.data",
                "task.submit",
                "brew.schedule",
                "setup.status",
            ]),
        );
        map.insert(
            CapabilityCategory::ExternalIntegration,
            HashSet::from([
                "http.fetch",
                "hitokoto.get",
                "notion.query",
                "bilibili.user",
                "bilibili.video",
                "bangumi.user",
                "bangumi.collections",
                "steam.user",
                "steam.game",
                "proxy.image",
                "weather.get",
                "netease.song",
                "netease.playlist.detail",
                "web.scrape",
            ]),
        );
        map.insert(
            CapabilityCategory::UiControl,
            HashSet::from([
                "tapp.ui",
                "tapp.understand",
                "tapp.interact",
                "tapp.pageContent",
                "tapp.windows",
                "tapp.window.open",
                "tapp.window.close",
                "tapp.window.focus",
                "router.navigate",
                "router.state",
                "page.interact",
                "page.understand",
                "page.content",
                "music.control",
                "music.status",
                "music.playlist",
            ]),
        );
        map
    }

    #[test]
    fn every_registered_capability_has_a_handler() {
        let registry = CapabilityRegistry::new();
        let by_cat = handler_ids_by_category();
        let mut missing = Vec::new();
        let mut registered_by_cat: HashMap<CapabilityCategory, HashSet<String>> = HashMap::new();

        for cap in registry.get_all() {
            registered_by_cat
                .entry(cap.category.clone())
                .or_default()
                .insert(cap.id.clone());
            match by_cat.get(&cap.category) {
                Some(set) if set.contains(cap.id.as_str()) => {}
                Some(_) | None => missing.push(format!("{} ({:?})", cap.id, cap.category)),
            }
        }

        let mut extra = Vec::new();
        for (cat, ids) in &by_cat {
            let registered = registered_by_cat.get(cat).cloned().unwrap_or_default();
            for id in ids {
                if !registered.contains(*id) {
                    extra.push(format!("{id} ({cat:?})"));
                }
            }
        }

        assert!(
            missing.is_empty(),
            "registered capabilities with no handler match arm: {missing:?}"
        );
        assert!(
            extra.is_empty(),
            "handler match arms with no registry entry: {extra:?}"
        );
        assert_eq!(
            registry.get_all().len(),
            117,
            "update handler_ids_by_category when adding a capability"
        );
    }
}
