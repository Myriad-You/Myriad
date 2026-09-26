use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
#[cfg(test)]
use std::collections::HashMap;

use super::agent_header::*;
use super::types::*;
use super::{capability, executor, memory};

/// 一次办事结束后交给记忆抽取的材料。
pub(crate) struct MemoryRecordParams<'a> {
    pub(crate) db: &'a DatabaseConnection,
    pub(crate) user_id: i32,
    pub(crate) user_input: &'a str,
    pub(crate) recipe: &'a Recipe,
    pub(crate) success: bool,
    /// 实际步骤执行结果（用于丰富记忆提取的上下文）
    pub(crate) step_results: Option<&'a std::collections::HashMap<String, StepResult>>,
}

/// 办事结束后的记忆抽取，写入统一记忆表。
///
/// 抽取是一次完整的 Standard 往返，`tokio::spawn` 到响应路径之外。归属需要
/// 显式带进去：detached task 的 task-local 是空的，不带就会从成本账里消失。
/// 用量计量器**不**带——它属于本回合的配额预留，这份工作在预留结算之后才跑完。
pub(crate) async fn record_execution_memory(params: MemoryRecordParams<'_>) {
    let step_outputs: Vec<(String, Value)> = params
        .recipe
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let result = params
                .step_results
                .and_then(|results| results.get(&step.id));
            let mut value = json!({
                "action": step.action,
                "capability": step.capability_id,
                "success": result.map(|r| r.success).unwrap_or(params.success),
            });
            if let Some(summary) = result
                .and_then(|r| r.output.as_ref())
                .map(memory::work_memory::summarize_value_for_memory)
            {
                value["output_summary"] = Value::String(summary);
            }
            if let Some(error) = result.and_then(|r| r.error.as_ref()) {
                value["error"] = Value::String(error.clone());
            }
            (format!("step_{}", index + 1), value)
        })
        .collect();
    let run = memory::work_memory::WorkRun {
        user_id: params.user_id,
        user_input: params.user_input.to_string(),
        conversation: Vec::new(),
        step_outputs,
        capabilities: params
            .recipe
            .steps
            .iter()
            .map(|step| step.capability_id.clone())
            .collect(),
        success: params.success,
    };
    let db = params.db.clone();
    let attribution = crate::services::ai_cost_ledger::current_ai_attribution();
    tokio::spawn(async move {
        let learn = memory::work_memory::learn_from_run(&db, run);
        match attribution {
            Some(attribution) => {
                crate::services::ai_cost_ledger::with_ai_ledger_attribution(
                    crate::services::ai_cost_ledger::AiLedgerAttribution {
                        operation: "agent.memory".to_string(),
                        ..attribution
                    },
                    learn,
                )
                .await
            }
            None => learn.await,
        }
    });
}

/// 一次 Agent 回合的 AI 预算：先按估算预留，结束后按实际消耗结算。
///
/// Admin（含代站长运行的 `SYSTEM_USER_ID` 心跳）在 `limits_for_role` 里是 unlimited，
/// 预留是空操作，所以这条门只对普通用户和访客生效。
pub(crate) struct AgentTurnBudget {
    reservation: crate::services::ai_quota::AiQuotaReservation,
    meter: crate::services::ai_cost_ledger::AiUsageMeter,
    attribution: crate::services::ai_cost_ledger::AiLedgerAttribution,
}

/// 一次回合的预留额度。
///
/// 回合的真实开销要到跑完才知道（步骤数、是否 replan 都不确定），所以这里只预留
/// 一个够判断「预算是不是已经见底」的基数：`AGENT_TURN_TOKEN_ESTIMATE` = 10_000。
/// 低估不会漏账——`settle_ai_quota` 会按实际用量补差，超出的部分记在这一回合，
/// 由下一回合的预留检查拦下。
const AGENT_TURN_TOKEN_ESTIMATE: usize = 10_000;

/// 这次预留属于哪种回合。
///
/// 冷却是用来给**新需求**之间留间隔的。确认 / 恢复不是新需求——它们是 Agent 自己
/// 提出的问题的回答，紧接着上一轮，用户点得多快就来得多快。默认配置
/// （`user_ai_cooldown_seconds = 5` / `guest_ai_cooldown_seconds = 10`）下按新回合
/// 判定，「规划 → 确认」几乎必然撞上 `AI_COOLDOWN_ACTIVE`：用户刚批准的活反而干不了。
///
/// 调用次数和 token 检查两种都照收——续跑确实要花 AI；只有冷却门对续跑放行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentTurnKind {
    /// 用户发起的新回合（`process` / 预设执行）。
    Fresh,
    /// 上一回合的延续（确认后执行、WaitingForInput 恢复）。
    Continuation,
}

impl AgentTurnKind {
    fn reserve_options(self) -> crate::services::ai_quota::AiQuotaReserveOptions {
        crate::services::ai_quota::AiQuotaReserveOptions {
            skip_cooldown: self == Self::Continuation,
        }
    }
}

impl AgentTurnBudget {
    /// 预留本回合额度；额度不足直接返回错误，不进入执行。
    pub(crate) async fn reserve(
        db: &sea_orm::DatabaseConnection,
        user_id: i32,
        operation: &str,
        task_id: String,
        kind: AgentTurnKind,
    ) -> Result<Self, String> {
        let role = crate::services::permission_service::role_from_user_id(
            user_id,
            user_is_current_admin(db, user_id).await?,
        );
        let reservation = crate::services::ai_quota::reserve_ai_quota_with_options(
            db,
            role,
            user_id,
            user_id,
            crate::services::ai_quota::SITE_AGENT_SCOPE,
            AGENT_TURN_TOKEN_ESTIMATE,
            None,
            kind.reserve_options(),
        )
        .await
        .map_err(|error| {
            tracing::info!(
                user_id,
                code = error.code(),
                kind = ?kind,
                "[Agent] Turn rejected by AI quota"
            );
            error.to_string()
        })?;

        Ok(Self {
            reservation,
            meter: crate::services::ai_cost_ledger::AiUsageMeter::new(),
            attribution: crate::services::ai_cost_ledger::AiLedgerAttribution {
                subject_id: user_id,
                owner_id: user_id,
                source: "agent".into(),
                operation: operation.into(),
                tapp_id: None,
                task_id,
            },
        })
    }

    /// 在预算作用域内运行整个回合。
    ///
    /// 同时装上归属和计量：归属覆盖 Planner 调用，计量跨越 executor 内层重新设置的归属，
    /// 保证统计的是整个回合。
    pub(crate) async fn scope<F, T>(&self, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        crate::services::ai_cost_ledger::with_ai_ledger_attribution(
            self.attribution.clone(),
            crate::services::ai_cost_ledger::with_ai_usage_meter(self.meter.clone(), fut),
        )
        .await
    }

    /// 按实际消耗结算预留。
    pub(crate) async fn settle(self, db: &sea_orm::DatabaseConnection) {
        let spent = self.meter.total_tokens();
        if let Err(error) =
            crate::services::ai_quota::settle_ai_quota(db, &self.reservation, spent as usize).await
        {
            // 结算失败只影响计量精度，不该让已经完成的回合失败。
            tracing::warn!(
                %error,
                spent_tokens = spent,
                "[Agent] Failed to settle AI quota for this turn"
            );
        }
    }

    /// 预留 → 在作用域内跑 `body` → 结算，一次做完（用户发起的新回合）。
    ///
    /// 确认 / 恢复采用**重新预留**而不是把首轮的预留挂着：确认之间隔着一次用户
    /// 往返，可能是几分钟，长时间占着额度只会让并发用户互相饿死。代价是一次
    /// 「规划 + 确认后执行」记两次调用，这在语义上也说得通——它确实是两次请求。
    /// 但那第二次是续跑，不该再过冷却门，见 [`Self::run_continuation`]。
    ///
    /// `body` 用 `Pin<Box<...>>` 接收：装箱让 rustc 类型布局查询在指针处终止
    /// （`queries overflow the depth limit`；增量缓存会让本地 `cargo check` 假通过）。
    /// 用 `AssertUnwindSafe` + `catch_unwind` 包一层，让 body panic 时预留也能被
    /// 结算掉，否则预留的 tokens 会一直挂到当天配额重置。
    pub(crate) async fn run<T>(
        db: &sea_orm::DatabaseConnection,
        user_id: i32,
        operation: &str,
        task_id: String,
        body: std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, String>> + Send + '_>>,
    ) -> Result<T, String> {
        Self::run_with_kind(db, user_id, operation, task_id, AgentTurnKind::Fresh, body).await
    }

    /// 同 [`Self::run`]，但按**续跑**预留：不再过冷却门。
    ///
    /// 用于确认后执行与 WaitingForInput 恢复——这两条路上的「等待」是我们让用户等
    /// 的，冷却再拦一道只会把刚批准的操作挡在门外。
    ///
    /// 调用方要负责把不跑 AI 的分支（取消、越权、不存在、已过期、参数还没齐）留在
    /// 预留**之外**：那些路径一次模型都不调，不该记一次调用、也不该把 10k tokens
    /// 挂到结算才退。
    pub(crate) async fn run_continuation<T>(
        db: &sea_orm::DatabaseConnection,
        user_id: i32,
        operation: &str,
        task_id: String,
        body: std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, String>> + Send + '_>>,
    ) -> Result<T, String> {
        Self::run_with_kind(
            db,
            user_id,
            operation,
            task_id,
            AgentTurnKind::Continuation,
            body,
        )
        .await
    }

    async fn run_with_kind<T>(
        db: &sea_orm::DatabaseConnection,
        user_id: i32,
        operation: &str,
        task_id: String,
        kind: AgentTurnKind,
        body: std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, String>> + Send + '_>>,
    ) -> Result<T, String> {
        use futures::FutureExt;

        let budget = Self::reserve(db, user_id, operation, task_id, kind).await?;
        let outcome = budget
            .scope(std::panic::AssertUnwindSafe(body).catch_unwind())
            .await;
        budget.settle(db).await;

        match outcome {
            Ok(result) => result,
            Err(panic) => {
                // 结算已经完成，这里只把 panic 继续抛出去。
                std::panic::resume_unwind(panic)
            }
        }
    }
}

/// Discovery list filtered by the same granted permissions the planner uses (`TappPermissionService::check`).
pub async fn get_capabilities_summary_for_user(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> serde_json::Value {
    // Informational listing: an unreadable role (already logged) shows the
    // non-admin summary, whose grants are computed separately below.
    if user_is_current_admin(db, user_id).await == Ok(true) {
        capability::get_capability_summary_filtered(None).await
    } else {
        let granted = get_user_permissions(db, user_id).await;
        capability::get_capability_summary_filtered(Some(&granted)).await
    }
}

/// Query the current role ([`crate::services::principal::is_current_admin`]).
/// Agent work can execute long after a token was issued, so a hard-coded
/// "first user is admin" rule is unsafe. The heartbeat (user 0) is not a
/// person: it acts for the site owner, so it is an administrator exactly while
/// the owner is one, and nothing before setup. A failed read is an error;
/// callers decide, it is never reported as "not an admin".
pub async fn user_is_current_admin(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Result<bool, String> {
    let subject = if user_id == SYSTEM_USER_ID {
        match heartbeat_delegate(db).await? {
            Some(owner) => owner,
            None => return Ok(false),
        }
    } else {
        user_id
    };
    crate::services::principal::is_current_admin(db, subject)
        .await
        .map_err(|error| {
            tracing::warn!(user_id, %error, "[Agent] Failed to read current user role");
            "Could not verify current administrator status".to_string()
        })
}

/// The account the heartbeat acts for: the site owner. Heartbeat tasks are
/// written by the owner, so they run with the owner's granted permissions and
/// narrow with them. `None` before setup.
pub(crate) async fn heartbeat_delegate(
    db: &sea_orm::DatabaseConnection,
) -> Result<Option<i32>, String> {
    crate::services::principal::site_owner_id(db)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "[Agent] Failed to resolve the site owner for the heartbeat");
            "Could not resolve the site owner".to_string()
        })
}

/// 非管理员 Agent 能力候选全集；之后按当前授予权限过滤。
///
/// 空的自治授权请求用它做默认范围：当前授予 ∩ 这个集合。管理员专属权限
/// 不进默认自治范围；显式列出且当前已授予时仍然允许。
pub(crate) fn max_user_agent_permissions() -> std::collections::HashSet<String> {
    // 共享订阅库管理、报告生成和系统管理不进入非管理员候选集。
    [
        "platform:read",
        "steam:read",
        "bilibili:read",
        "bangumi:read",
        "github:read",
        "netease:read",
        "ai:analyze",
        "ai:chat",
        "ai:search",
        "ai:image",
        "phantasi:read", // 读共享库；标记已读/收藏走 phantasi:write
        "report:read",
        "tapp:read",
        "system:read",
        "http:fetch",
        "web:scrape",
        "tapp:write",
        "tapp:interact",
        "phantasi:write",
        "weather:read",
        "metadata:read",
        "proxy:read",
        "scheduler:read",
        "scheduler:write",
        "3d:generate",
        "ai:generate",
        "speech:tts",
        "music:control",
        "music:read",
        "storage:write",
        "content:write",
        "reminder:write",
        "note:write",
        "bookmark:write",
        "mcp:execute",
        "notion:read",
        "rsshub:read",
        "profile:read",
        "random:read",
        "search:read",
        "router:read",
        "router:write",
        "ui:read",
        "ui:interact",
        "page:read",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// 校验当前用户是否允许使用 Agent（页面可见性 + Tapp `ai:chat`）
///
/// 返回 Ok(is_admin)；禁用时返回 403 语义错误字符串
pub async fn ensure_agent_usage_allowed(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Result<bool, String> {
    use crate::services::permission_service::{TappPermission, TappPermissionService, UserRole};

    let is_admin = user_is_current_admin(db, user_id).await?;
    if is_admin || user_id == SYSTEM_USER_ID {
        return Ok(true);
    }

    let visibility = crate::services::module_visibility::agent_module_visibility(db).await?;
    if visibility == "admin" {
        return Err("Agent is admin only".to_string());
    }

    // 授予权限：TappPermissionService::check(..., AiChat)
    let config = crate::services::config_service::ConfigService::load_permission_config_on(db)
        .await
        .map_err(|_| "Could not verify current Agent permission policy".to_string())?;
    if !TappPermissionService::check(&config, UserRole::User, TappPermission::AiChat) {
        return Err("Agent chat is not enabled for this account".to_string());
    }
    Ok(false)
}

/// Agent 权限串 → Tapp 权限。未映射则非管理员拒绝；`host_agent_permission`（system/router/ui/page）例外。
///
/// 非管理员能力 = 候选全集 ∩ 角色授予（`TappPermissionService::check`；下放只影响 elevated）
///
/// `mcp:execute` 不映射到 NetworkFetch。Capability 注册写 `required_permissions: ["mcp:execute"]`。
/// 无映射 → 非管理员拒；管理员走 `get_user_permissions` 里单独 insert 的那一条。
fn agent_perm_to_tapp(perm: &str) -> Option<crate::services::permission_service::TappPermission> {
    use crate::services::permission_service::TappPermission;
    match perm {
        // AI（elevated，须在 Tapp 权限管理中下放）
        "ai:chat" => Some(TappPermission::AiChat),
        "ai:analyze" => Some(TappPermission::AiAnalyze),
        "ai:image" => Some(TappPermission::AiImage),
        "3d:generate" => Some(TappPermission::ThreeDGenerate),
        "ai:search" => Some(TappPermission::AiSearch),
        "ai:generate" => Some(TappPermission::AiGenerate),
        // 读（basic）
        "phantasi:read" => Some(TappPermission::PhantasiRead),
        "phantasi:write" => Some(TappPermission::PhantasiWrite),
        "report:read" => Some(TappPermission::ReportRead),
        "platform:read" | "steam:read" | "bilibili:read" | "bangumi:read" | "github:read"
        | "netease:read" | "weather:read" | "metadata:read" => Some(TappPermission::PlatformRead),
        "tapp:read" => Some(TappPermission::TappListRead),
        // 写 / 出站 / 媒体（Tapp 映射）
        "phantasi:manage" => Some(TappPermission::PhantasiManage),
        "report:write" => Some(TappPermission::ReportWrite),
        "http:fetch" | "web:scrape" | "proxy:read" => Some(TappPermission::NetworkFetch),
        "scheduler:read" | "scheduler:write" => Some(TappPermission::SchedulerRegister),
        // 个人 Tapp 写：用 storage:write 表达「可持久化自己的内容」，非 manage 全站
        "tapp:write" | "tapp:interact" | "storage:write" | "content:write" | "reminder:write"
        | "note:write" | "bookmark:write" => Some(TappPermission::StorageWrite),
        "speech:tts" => Some(TappPermission::SpeechTts),
        "music:control" => Some(TappPermission::MediaControl),
        "music:read" => Some(TappPermission::MediaRead),
        "notion:read" => Some(TappPermission::NetworkFetch),
        "profile:read" | "random:read" | "search:read" | "rsshub:read" => {
            Some(TappPermission::PlatformRead)
        }
        "platform:write" => Some(TappPermission::PlatformWrite),
        "phantasi:admin" => Some(TappPermission::PhantasiManage),
        // system:read / 宿主 UI 无 Tapp 对应，见 retain 特例
        _ => None,
    }
}

/// Agent 权限串没有 Tapp 对应：登录用户只要进了候选集就可以用。
///
/// 宿主 SPA 导航/读页/点页面是 Agent 作为用户操作本站，不是 Tapp 沙箱能力。
fn host_agent_permission(perm: &str) -> bool {
    matches!(
        perm,
        "system:read" | "router:read" | "router:write" | "ui:read" | "ui:interact" | "page:read"
    )
}

/// Whether the Agent grant set would let `scheduler.create` extra Tapp checks pass.
///
/// Basic Tapp entries (`storage:read`, `ui:notification`) succeed at execute for
/// any logged-in user, so they are treated as covered. Privileged / elevated
/// extras must appear in the grant set (same mapping as `agent_perm_to_tapp`).
pub(crate) fn granted_covers_tapp_permission(
    granted: &std::collections::HashSet<String>,
    permission: crate::services::permission_service::TappPermission,
) -> bool {
    use crate::services::permission_service::TappPermission;
    match permission {
        TappPermission::SchedulerRegister => {
            granted.contains("scheduler:write") || granted.contains("scheduler:read")
        }
        TappPermission::PlatformWrite => granted.contains("platform:write"),
        TappPermission::StorageWrite => [
            "storage:write",
            "tapp:write",
            "tapp:interact",
            "content:write",
            "reminder:write",
            "note:write",
            "bookmark:write",
        ]
        .iter()
        .any(|perm| granted.contains(*perm)),
        TappPermission::StorageRead | TappPermission::UiNotification => true,
        TappPermission::AiGenerate => granted.contains("ai:generate"),
        TappPermission::AiSearch => granted.contains("ai:search"),
        TappPermission::NetworkFetch => ["http:fetch", "web:scrape", "proxy:read", "notion:read"]
            .iter()
            .any(|perm| granted.contains(*perm)),
        _ => false,
    }
}

/// Plan-time mirror of `scheduler.create`'s extra Tapp checks on `backendActions`.
pub(crate) fn scheduler_create_actions_within_grants(
    params: &std::collections::HashMap<String, serde_json::Value>,
    granted: &std::collections::HashSet<String>,
) -> Result<(), String> {
    use crate::services::agent::system_op_pure::extract_raw_backend_actions;
    use crate::services::tapp_scheduler::{
        backend_action_permissions_of, normalize_backend_actions_parsed,
    };

    let Some(raw) = extract_raw_backend_actions(params) else {
        return Ok(());
    };
    let (_normalized, wrappers) = normalize_backend_actions_parsed(Some(raw))?;
    for permission in backend_action_permissions_of(&wrappers) {
        if !granted_covers_tapp_permission(granted, permission) {
            return Err(format!(
                "capability 'scheduler.create' is not available for scheduled action: {}",
                permission.as_str()
            ));
        }
    }
    Ok(())
}

/// Execute-time translation of `scheduler.create`'s Tapp permissions into the
/// Agent grant vocabulary. Do not pass `permission.as_str()` to
/// `authorize_capability`: those strings are not in the Agent grant set.
pub(crate) fn scheduler_create_tapp_permissions_within_grants(
    granted: &std::collections::HashSet<String>,
    permissions: &[crate::services::permission_service::TappPermission],
) -> Result<(), String> {
    for permission in permissions {
        if !granted_covers_tapp_permission(granted, *permission) {
            return Err(format!(
                "capability 'scheduler.create' is not available for scheduled action: {}",
                permission.as_str()
            ));
        }
    }
    Ok(())
}

/// 获取用户在 Agent 系统中的授予权限。
///
/// - 管理员（含站长是管理员时的心跳）：全部能力权限
/// - 其他：候选全集 ∩ `TappPermissionService::check`（角色下放后的授予权限，不是安装批准）
pub async fn get_user_permissions(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> std::collections::HashSet<String> {
    use crate::services::permission_service::{TappPermissionService, UserRole};
    use std::collections::HashSet;

    // 系统用户或管理员：全部权限。角色读不出与下方可见性读不出一样按最小权限。
    let is_admin = match user_is_current_admin(db, user_id).await {
        Ok(is_admin) => is_admin,
        Err(_) => return HashSet::new(),
    };
    if is_admin {
        let registry = capability::get_registry();
        let mut permissions: HashSet<String> = registry
            .get_all()
            .iter()
            .flat_map(|cap| cap.required_permissions.iter().cloned())
            .collect();
        permissions.insert("mcp:execute".to_string());
        return permissions;
    }

    let visibility = match crate::services::module_visibility::agent_module_visibility(db).await {
        Ok(visibility) => visibility,
        Err(error) => {
            tracing::warn!(%error, "Could not verify Agent module visibility");
            return HashSet::new();
        }
    };
    // 可见性 admin-only 时，非管理员无任何 agent 能力
    if visibility == "admin" {
        return HashSet::new();
    }

    let mut perms = max_user_agent_permissions();

    // 候选集 ∩ 角色授予（TappPermissionService::check）
    let config =
        match crate::services::config_service::ConfigService::load_permission_config_on(db).await {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!(%error, "Could not verify current Agent permission policy");
                return HashSet::new();
            }
        };
    perms.retain(|p| {
        if host_agent_permission(p) {
            return true;
        }
        match agent_perm_to_tapp(p) {
            Some(tp) => TappPermissionService::check(&config, UserRole::User, tp),
            // 未映射权限：非管理员拒绝（避免旁路）
            None => false,
        }
    });

    perms
}

/// 初始化任务存储的数据库连接
///
/// 应在应用启动时调用，以支持任务持久化和恢复
pub async fn init_task_store(db: DatabaseConnection) {
    executor::init_task_store_db(db).await;
}

// 预执行参数收集 / 写回

#[cfg(test)]
mod tests {
    use super::*;

    /// Guests are answered without the database; a real account whose role
    /// cannot be read is an error, not "not an admin". The heartbeat reads the
    /// owner's role, so it is not assumed to be an administrator.
    #[tokio::test]
    async fn current_admin_role_read_failure_is_an_error() {
        let db = sea_orm::DatabaseConnection::default();
        assert_eq!(user_is_current_admin(&db, -4).await, Ok(false));
        crate::middleware::auth::invalidate_auth_cache_local(910_401);
        assert!(user_is_current_admin(&db, 910_401).await.is_err());
        assert!(get_user_permissions(&db, 910_401).await.is_empty());
    }

    #[test]
    fn only_continuations_skip_the_cooldown_gate() {
        // Confirm / resume arrive as fast as a human can click. Reserving them
        // as fresh turns means the default 5s (user) / 10s (guest) cooldown
        // rejects the work the user just approved.
        assert!(
            AgentTurnKind::Continuation.reserve_options().skip_cooldown,
            "confirm / resume must not re-apply cooldown"
        );
        assert!(
            !AgentTurnKind::Fresh.reserve_options().skip_cooldown,
            "a new user turn is exactly what cooldown is for"
        );
    }

    /// 非管理员开着「网络请求」也拿不到 MCP。
    ///
    /// 这个开关在权限页写的是「允许声明式出站请求，以及加载远端图片/音视频
    /// 资源」。站长按这句话打开它，不应该同时把本站接入的每一个 MCP 工具
    /// 交给普通用户——那是文件系统、内网 API 这一类东西，不是取个网页。
    #[test]
    fn the_network_fetch_switch_does_not_hand_out_mcp() {
        use crate::config::DynamicConfig;
        use crate::services::permission_service::{
            TappPermission, TappPermissionService, UserRole,
        };

        let mut config = DynamicConfig::default();
        config.user_perm_network_fetch = true;
        assert!(TappPermissionService::check(
            &config,
            UserRole::User,
            TappPermission::NetworkFetch
        ));

        // 开关开着，出站类权限确实放行……
        for perm in ["http:fetch", "web:scrape", "proxy:read", "notion:read"] {
            let mapped = agent_perm_to_tapp(perm).expect("mapped");
            assert!(
                TappPermissionService::check(&config, UserRole::User, mapped),
                "{perm} 应当跟着「网络请求」开关走"
            );
        }

        // ……但 MCP 不在其中，而且它也不是宿主 UI 那一档的特例。
        assert_eq!(agent_perm_to_tapp("mcp:execute"), None);
        assert!(!host_agent_permission("mcp:execute"));

        // 反向映射得跟着一起改，否则文档里那句「same mapping」就成了假话。
        let mcp_only: std::collections::HashSet<String> =
            ["mcp:execute".to_string()].into_iter().collect();
        assert!(!granted_covers_tapp_permission(
            &mcp_only,
            TappPermission::NetworkFetch
        ));
    }

    #[test]
    fn max_user_agent_permission_candidates() {
        let elevated = max_user_agent_permissions();
        assert!(elevated.contains("http:fetch"));
        assert!(elevated.contains("web:scrape"));
        assert!(elevated.contains("tapp:write"));
        assert!(elevated.contains("phantasi:write"));
        assert!(elevated.contains("scheduler:read"));
        assert!(elevated.contains("3d:generate"));
        assert!(elevated.contains("ai:generate"));
        assert!(elevated.contains("speech:tts"));
        assert!(elevated.contains("music:control"));
        assert!(elevated.contains("router:write"));
        assert!(elevated.contains("mcp:execute"));
        assert!(!elevated.contains("phantasi:manage"));
        assert!(!elevated.contains("report:write"));
        assert!(!elevated.contains("system:admin"));
    }

    #[test]
    fn agent_perm_to_tapp_force_alignment_map() {
        use crate::services::permission_service::TappPermission;
        assert_eq!(agent_perm_to_tapp("ai:chat"), Some(TappPermission::AiChat));
        assert_eq!(
            agent_perm_to_tapp("3d:generate"),
            Some(TappPermission::ThreeDGenerate)
        );
        assert_eq!(
            agent_perm_to_tapp("ai:search"),
            Some(TappPermission::AiSearch)
        );
        assert_eq!(
            agent_perm_to_tapp("http:fetch"),
            Some(TappPermission::NetworkFetch)
        );
        assert_eq!(
            agent_perm_to_tapp("phantasi:read"),
            Some(TappPermission::PhantasiRead)
        );
        assert_eq!(
            agent_perm_to_tapp("phantasi:write"),
            Some(TappPermission::PhantasiWrite)
        );
        assert_eq!(
            agent_perm_to_tapp("phantasi:manage"),
            Some(TappPermission::PhantasiManage)
        );
        assert_eq!(
            agent_perm_to_tapp("report:write"),
            Some(TappPermission::ReportWrite)
        );
        assert_eq!(agent_perm_to_tapp("system:read"), None); // 特例：不经 Tapp
        assert_eq!(agent_perm_to_tapp("router:write"), None);
        assert_eq!(
            agent_perm_to_tapp("speech:tts"),
            Some(TappPermission::SpeechTts)
        );
        assert_eq!(
            agent_perm_to_tapp("music:control"),
            Some(TappPermission::MediaControl)
        );
        assert_eq!(
            agent_perm_to_tapp("content:write"),
            Some(TappPermission::StorageWrite)
        );
        // MCP 工具不跟着「网络请求」开关走：没有映射就等于非管理员拒绝。
        assert_eq!(agent_perm_to_tapp("mcp:execute"), None);
        assert_eq!(
            agent_perm_to_tapp("notion:read"),
            Some(TappPermission::NetworkFetch)
        );
        assert_eq!(agent_perm_to_tapp("unknown:perm"), None);
        assert!(host_agent_permission("router:write"));
        assert!(!host_agent_permission("speech:tts"));
        use std::collections::HashSet;
        let elevated = max_user_agent_permissions();
        assert!(granted_covers_tapp_permission(
            &elevated,
            TappPermission::NetworkFetch
        ));
        assert!(granted_covers_tapp_permission(
            &elevated,
            TappPermission::StorageWrite
        ));
        assert!(!granted_covers_tapp_permission(
            &elevated,
            TappPermission::PlatformWrite
        ));
        assert!(granted_covers_tapp_permission(
            &elevated,
            TappPermission::StorageRead
        ));

        let mut fetch_only = HashSet::new();
        fetch_only.insert("http:fetch".to_string());
        let mut fetch_params = std::collections::HashMap::new();
        fetch_params.insert(
            "backendActions".to_string(),
            json!([{ "type": "fetch", "url": "https://example.com" }]),
        );
        assert!(scheduler_create_actions_within_grants(&fetch_params, &fetch_only).is_ok());
        assert!(scheduler_create_actions_within_grants(&fetch_params, &HashSet::new()).is_err());
        let mut sync_params = std::collections::HashMap::new();
        sync_params.insert(
            "backendActions".to_string(),
            json!([{ "type": "platform.sync", "platform": "steam" }]),
        );
        assert!(scheduler_create_actions_within_grants(&sync_params, &elevated).is_err());
    }

    #[test]
    fn autonomy_scheduler_create_translates_tapp_permissions() {
        use crate::services::agent::consciousness::missing_permission;
        use crate::services::agent::system_op_pure::extract_raw_backend_actions;
        use crate::services::permission_service::TappPermission;
        use crate::services::tapp_scheduler::{
            backend_action_permissions_of, normalize_backend_actions_parsed,
        };
        use std::collections::HashSet;

        fn required_of(params: &HashMap<String, Value>) -> Vec<TappPermission> {
            let mut required = vec![TappPermission::SchedulerRegister];
            if let Some(raw) = extract_raw_backend_actions(params) {
                let (_normalized, wrappers) =
                    normalize_backend_actions_parsed(Some(raw)).expect("actions");
                required.extend(backend_action_permissions_of(&wrappers));
            }
            required
        }

        let mut basic = HashMap::new();
        basic.insert(
            "backendActions".into(),
            json!([
                { "action": "storage.get", "key": "note" },
                { "action": "notification.queue", "message": "ok" }
            ]),
        );
        let mut fetch = HashMap::new();
        fetch.insert(
            "backendActions".into(),
            json!([{ "type": "fetch", "url": "https://example.com" }]),
        );
        let granted: HashSet<String> = ["scheduler:write".to_string()].into_iter().collect();
        let granted_list: Vec<String> = granted.iter().cloned().collect();

        let basic_required = required_of(&basic);
        // The old execute path compared Tapp strings (`scheduler:register`) to
        // the Agent grant set and always refused. The translated check allows
        // a scheduler:write ceiling that only schedules basic actions.
        let tapp_strings: Vec<String> = basic_required
            .iter()
            .map(|permission| permission.as_str().to_string())
            .collect();
        assert!(
            missing_permission(&granted_list, "scheduler.create", &tapp_strings).is_some(),
            "Tapp strings must not be treated as Agent grants"
        );
        assert!(
            scheduler_create_tapp_permissions_within_grants(&granted, &basic_required).is_ok(),
            "scheduler:write plus basic actions must be allowed"
        );

        let fetch_required = required_of(&fetch);
        let refused = scheduler_create_tapp_permissions_within_grants(&granted, &fetch_required);
        assert!(
            refused
                .as_ref()
                .is_err_and(|error| error.contains("network:fetch")),
            "fetch without a network grant must be refused, got {refused:?}"
        );

        for params in [&basic, &fetch] {
            let required = required_of(params);
            assert_eq!(
                scheduler_create_tapp_permissions_within_grants(&granted, &required).is_ok(),
                scheduler_create_actions_within_grants(params, &granted).is_ok(),
                "execution and plan must agree for {params:?}"
            );
        }
    }

    #[test]
    fn saved_recipe_validation_rejects_dependency_cycles() {
        let mut recipe = Recipe::new("cycle", "cycle", ExecutionType::Instant);
        let step = |id: &str, dependency: &str, order| RecipeStep {
            id: id.to_string(),
            order,
            capability_id: "ai.summarize".to_string(),
            action: id.to_string(),
            params: HashMap::new(),
            depends_on: vec![dependency.to_string()],
            on_failure: FailureStrategy::Abort,
            retry: None,
            timeout_ms: None,
            model_tier: None,
            generator: None,
        };
        recipe.steps = vec![step("a", "b", 0), step("b", "a", 1)];

        let error = Agent::validate_saved_recipe(&recipe).expect_err("cycle must be rejected");
        assert!(error.contains("dependency cycle"));
    }

    #[test]
    fn heartbeat_outcome_rejects_blocked_and_interactive_responses() {
        let response = |response_type, data| AgentResponse {
            response_type,
            message: "result".to_string(),
            data,
            data_display: None,
            suggestions: vec![],
            task: None,
            frontend_action: None,
            performance: None,
        };

        assert!(response(AgentResponseType::Answer, None).is_successful_outcome());
        assert!(
            !response(AgentResponseType::Answer, Some(json!({"blocked": true})))
                .is_successful_outcome()
        );
        assert!(!response(AgentResponseType::Clarification, None).is_successful_outcome());

        for status in [
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::WaitingForInput,
            TaskStatus::Paused,
            TaskStatus::Failed,
            TaskStatus::Cancelled,
        ] {
            let recipe = Recipe::new("heartbeat", "heartbeat", ExecutionType::Instant);
            let mut task = TaskState::new(&recipe);
            task.status = status;
            let mut non_terminal = response(AgentResponseType::TaskCompleted, None);
            non_terminal.task = Some(task);
            assert!(!non_terminal.is_successful_outcome());
        }

        let recipe = Recipe::new("heartbeat", "heartbeat", ExecutionType::Instant);
        let mut task = TaskState::new(&recipe);
        task.status = TaskStatus::Completed;
        let mut completed = response(AgentResponseType::TaskCompleted, None);
        completed.task = Some(task.clone());
        assert!(completed.is_successful_outcome());

        let mut completed_with_failed_step = response(AgentResponseType::TaskCompleted, None);
        let mut failed_step_task = task;
        failed_step_task.step_results.insert(
            "blocked_dynamic_step".to_string(),
            StepResult {
                step_id: "blocked_dynamic_step".to_string(),
                success: false,
                output: None,
                error: Some("blocked".to_string()),
                duration_ms: 0,
                retry_count: 0,
            },
        );
        completed_with_failed_step.task = Some(failed_step_task);
        assert!(!completed_with_failed_step.is_successful_outcome());
    }

    #[test]
    fn task_state_task_id_matches_recipe_id_for_sse_cancel() {
        let recipe = Recipe::new("t", "do something", ExecutionType::Instant);
        let task = TaskState::new(&recipe);
        assert_eq!(
            task.task_id, recipe.id,
            "TaskCreated SSE uses recipe.id; cancel/steer must hit the same id"
        );
        assert_eq!(task.recipe_id, recipe.id);
    }
}
