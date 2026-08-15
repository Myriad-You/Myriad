use chrono::Utc;
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};
use std::collections::HashMap;

use super::agent_header::*;
use super::types::*;
use super::{capability, executor, mcp, memory, skill, types};

/// 记录执行记忆的通用参数
pub(crate) struct MemoryRecordParams<'a> {
    pub(crate) user_id: i32,
    pub(crate) user_input: &'a str,
    pub(crate) recipe: &'a Recipe,
    pub(crate) planner_steps_len: usize,
    pub(crate) success: bool,
    pub(crate) error_msg: Option<&'a str>,
    /// 日志前缀（区分来源: "", "saved:", "confirmed:"）
    pub(crate) log_prefix: &'a str,
    /// 是否记录会话归档（仅完整 process 流程需要）
    pub(crate) conversation_context: Option<&'a [ConversationMessage]>,
    /// 实际步骤执行结果（用于丰富记忆提取的上下文）
    pub(crate) step_results: Option<&'a std::collections::HashMap<String, StepResult>>,
}

/// 统一的执行后记忆记录
///
/// 提取自 process / process_with_progress / execute_saved_recipe /
/// execute_simple_query_v2 / process_confirmation 中的重复逻辑。
pub(crate) async fn record_execution_memory(params: MemoryRecordParams<'_>) {
    let Some(mem) = memory::get_memory() else {
        return;
    };

    let ok = params.success;
    let step_caps: Vec<String> = params
        .recipe
        .steps
        .iter()
        .map(|s| s.capability_id.clone())
        .collect();

    // 1. AI 提取多维度记忆
    let exec_results: std::collections::HashMap<String, serde_json::Value> = params
        .recipe
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let step_key = format!("step_{}", i + 1);
            // 从实际 step_results 中查找对应输出摘要
            let output_summary = params
                .step_results
                .and_then(|sr| sr.get(&s.id))
                .and_then(|r| r.output.as_ref())
                .map(memory::summarize_value_for_memory);
            let step_success = params
                .step_results
                .and_then(|sr| sr.get(&s.id))
                .map(|r| r.success)
                .unwrap_or(ok);
            let mut val = serde_json::json!({
                "action": s.action,
                "capability": s.capability_id,
                "success": step_success,
            });
            if let Some(summary) = output_summary {
                val["output_summary"] = serde_json::Value::String(summary);
            }
            if let Some(err) = params
                .step_results
                .and_then(|sr| sr.get(&s.id))
                .and_then(|r| r.error.as_ref())
            {
                val["error"] = serde_json::Value::String(err.clone());
            }
            (step_key, val)
        })
        .collect();
    // 将会话上下文转换为 Value 格式供记忆提取 AI 使用
    let conversation_values: Option<Vec<serde_json::Value>> =
        params.conversation_context.map(|msgs| {
            msgs.iter()
                .filter_map(|m| serde_json::to_value(m).ok())
                .collect()
        });
    // AI 提取整轮记忆是一次完整的 Standard 往返，此前同步 await 在响应路径上——
    // 用户每次请求都要为一件自己看不见的后台整理白等一个来回。旁边的 Skill 自动
    // 创建早就是 spawn 的。
    //
    // 归属需要显式带进去：detached task 的 task-local 是空的，不带就会从成本账里
    // 消失。用量计量器**不**带——它属于本回合的配额预留，而这份工作在预留结算之后
    // 才跑完；后台整理也不该记在用户的额度上。
    {
        let mem = mem.clone();
        let attribution = crate::services::ai_cost_ledger::current_ai_attribution();
        let user_input = params.user_input.to_string();
        let exec_results = exec_results.clone();
        let step_caps_for_extraction = step_caps.clone();
        let user_id = params.user_id;
        tokio::spawn(async move {
            let extract = async {
                mem.extract_memories_from_execution(
                    &user_input,
                    conversation_values.as_deref(),
                    &exec_results,
                    ok,
                    &step_caps_for_extraction,
                    user_id,
                )
                .await;
            };
            match attribution {
                Some(attribution) => {
                    crate::services::ai_cost_ledger::with_ai_ledger_attribution(
                        crate::services::ai_cost_ledger::AiLedgerAttribution {
                            operation: "agent.memory".to_string(),
                            ..attribution
                        },
                        extract,
                    )
                    .await
                }
                None => extract.await,
            }
        });
    }

    // 2. 失败教训
    if !ok {
        let error = params.error_msg.unwrap_or("unknown");
        let lesson = format!(
            "{}执行失败教训：{} → 步骤 [{}] 失败: {}",
            params.log_prefix,
            params.user_input.chars().take(40).collect::<String>(),
            step_caps.join(", "),
            error
        );
        mem.remember_full(
            &lesson,
            memory::MemoryType::ExecutionLesson,
            memory::MemoryTier::MediumTerm,
            0.8,
            Vec::new(),
            step_caps.clone(),
            params.user_id,
        )
        .await;
    }

    // 3. 日志
    let first_cap = params
        .recipe
        .steps
        .first()
        .map(|s| s.capability_id.as_str())
        .unwrap_or("?");
    let summary = format!(
        "{}{} | {} 步 ({}, ...) → {}",
        params.log_prefix,
        params.user_input.chars().take(30).collect::<String>(),
        params.planner_steps_len,
        first_cap,
        if ok { "✓" } else { "✗" }
    );
    mem.log_daily(params.user_id, &summary).await;

    // 4. 会话摘要归档
    if let Some(history) = params.conversation_context {
        if history.len() >= 4 {
            let session_summary = format!(
                "会话主题：{} | 执行了 {} 步骤 | 结果：{}",
                params.user_input.chars().take(50).collect::<String>(),
                params.planner_steps_len,
                if ok { "成功" } else { "失败" }
            );
            mem.consolidate_session(&session_summary, params.user_id)
                .await;
        }
    }

    // 5. 提升 + 清理
    mem.promote_memories().await;
    mem.cleanup_short_term().await;
}

/// 一次 Agent 回合的 AI 预算：先按估算预留，结束后按实际消耗结算。
///
/// 此前 Agent 路径只有 `with_ai_ledger_attribution` 的**事后记账**，没有任何
/// 上限。一次回合会打出规划、每个数据步骤的动态分析、各 AI 步骤、结果汇总、
/// 记忆提取，失败还要 replan 重跑——跑飞时没有任何东西会拦，只会在账单里被看到。
/// Tapp AI 任务早就走 `reserve_ai_quota` 全套，Agent 只是没接上。
///
/// Admin（含 `SYSTEM_USER_ID` 的定时任务）在 `limits_for_role` 里是 unlimited，
/// 预留是空操作，所以这条门只对普通用户和访客生效。
pub(crate) struct AgentTurnBudget {
    reservation: crate::services::ai_quota::AiQuotaReservation,
    meter: crate::services::ai_cost_ledger::AiUsageMeter,
    attribution: crate::services::ai_cost_ledger::AiLedgerAttribution,
}

/// 一次回合的预留额度。
///
/// 回合的真实开销要到跑完才知道（步骤数、是否 replan 都不确定），所以这里只预留
/// 一个够判断「预算是不是已经见底」的基数：Planner 的 system prompt 本身就约
/// 30k 字符 ≈ 7.6k tokens，再留一点余量给用户 prompt 和至少一个下游 AI 步骤。
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
        let role = crate::services::tapp_context::role_for_subject(
            user_id,
            user_is_current_admin(db, user_id).await,
        );
        let reservation = crate::services::ai_quota::reserve_ai_quota_with_options(
            db,
            role,
            user_id,
            user_id,
            AGENT_LEDGER_TAPP_ID,
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
                tapp_id: AGENT_LEDGER_TAPP_ID.into(),
                task_id,
            },
        })
    }

    /// 在预算作用域内运行整个回合。
    ///
    /// 同时装上归属和计量：归属让 Planner 的调用第一次被记进成本账（此前它在
    /// 任何 attribution 作用域之外，executor 里那层只覆盖执行阶段），计量则跨越
    /// executor 内层重新设置的归属，保证统计的是整个回合。
    ///
    /// **调用方必须传入 `Box::pin(...)` 的回合体。** `process` /
    /// `process_with_progress` 的状态机本来就极大，再套两层 task-local 作用域后，
    /// 等着它们的 API handler 在计算类型布局时会超过 rustc 的递归上限
    /// （`queries overflow the depth limit`，深度 +130）。装箱让布局查询在指针处
    /// 终止；一次回合多一次堆分配，相对一次模型调用可以忽略。
    ///
    /// 注意这个错误只在**全新编译**时出现——增量缓存会让本地 `cargo check` 假通过。
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
    /// 所有会消耗 AI 的入口都该走这里。此前只有 `process` /
    /// `process_with_progress` 有预留，于是「规划后要确认」的流程是：首轮预留、
    /// 规划、返回确认、**结算**——随后确认接口把整条 recipe 跑完，全程无预留。
    /// 额度耗尽的用户只要点一次确认，仍然能把昂贵的活干完；预设执行更是从未
    /// 碰到过这道门。
    ///
    /// 确认 / 恢复采用**重新预留**而不是把首轮的预留挂着：确认之间隔着一次用户
    /// 往返，可能是几分钟，长时间占着额度只会让并发用户互相饿死。代价是一次
    /// 「规划 + 确认后执行」记两次调用，这在语义上也说得通——它确实是两次请求。
    /// 但那第二次是续跑，不该再过冷却门，见 [`Self::run_continuation`]。
    ///
    /// `body` 用 `Pin<Box<...>>` 接收：见 [`Self::scope`] 关于类型布局递归的说明。
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
                // 结算已经完成，这里只把 panic 继续抛出去，保持原有崩溃语义。
                std::panic::resume_unwind(panic)
            }
        }
    }
}

/// Agent 在配额与成本账里的 bucket key（与 `AiLedgerAttribution.tapp_id` 一致）
pub(crate) const AGENT_LEDGER_TAPP_ID: &str = "__agent__";

/// 获取系统能力摘要
pub async fn get_capabilities_summary() -> serde_json::Value {
    capability::get_capability_summary().await
}

/// Discovery list filtered by admin (hides system:admin caps for non-admin).
pub async fn get_capabilities_summary_for_user(is_admin: bool) -> serde_json::Value {
    capability::get_capability_summary_filtered(is_admin).await
}

/// Query the current database role. Agent recipes can execute long after a
/// token was issued, so a hard-coded "first user is admin" rule is unsafe.
pub async fn user_is_current_admin(db: &sea_orm::DatabaseConnection, user_id: i32) -> bool {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    if user_id == 0 {
        return true;
    }

    match db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT is_admin FROM users WHERE id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await
    {
        Ok(Some(row)) => row.try_get::<bool>("", "is_admin").unwrap_or(false),
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(
                user_id,
                %error,
                "[Agent] Failed to refresh current user role; using least privilege"
            );
            false
        }
    }
}

/// Agent 能力预设（与 Tapp 权限页「开关模板」对应；运行时以 Tapp 开关为准）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // 预设档位名 / 单测映射
pub enum AgentUsageMode {
    /// 禁用（Agent 相关 elevated 全关）
    None,
    /// 仅 AI 对话/分析
    Chat,
    /// 标准（平台/共享 Brew 只读 + AI；无出站）
    Standard,
    /// 扩展（标准 + 出站抓取 + 调度 + 个人 Tapp 写）
    Elevated,
}

impl AgentUsageMode {
    #[allow(dead_code)]
    pub fn parse(s: &str) -> Self {
        match s {
            "chat" => Self::Chat,
            "standard" => Self::Standard,
            "elevated" => Self::Elevated,
            _ => Self::None,
        }
    }
}

/// 某预设对应的 Agent 权限串（不含 brew:write / report:write）
fn permissions_for_usage_mode(mode: AgentUsageMode) -> std::collections::HashSet<String> {
    use std::collections::HashSet;
    let mut perms = HashSet::new();
    match mode {
        AgentUsageMode::None => {}
        AgentUsageMode::Chat => {
            for p in &["ai:chat", "ai:analyze", "system:read"] {
                perms.insert((*p).to_string());
            }
        }
        // 共享订阅库：普通用户永不授予 brew:write（加/改/删源仅管理员）
        AgentUsageMode::Standard | AgentUsageMode::Elevated => {
            for p in &[
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
                "brew:read", // 读共享库 + 个人已读/收藏（brew.mark）
                "report:read",
                "tapp:read",
                "system:read",
            ] {
                perms.insert((*p).to_string());
            }
            if mode == AgentUsageMode::Elevated {
                // 扩展：出站 + 调度 + 仅自己的 Tapp；报告生成仅管理员
                for p in &[
                    "http:fetch",
                    "web:scrape",
                    "tapp:write",
                    "weather:read",
                    "metadata:read",
                    "proxy:read",
                    "scheduler:read",
                    "scheduler:write",
                ] {
                    perms.insert((*p).to_string());
                }
            }
        }
    }
    perms
}

/// 非管理员 Agent 能力候选全集（再经 Tapp 开关过滤）
fn max_user_agent_permissions() -> std::collections::HashSet<String> {
    permissions_for_usage_mode(AgentUsageMode::Elevated)
}

/// 校验当前用户是否允许使用 Agent（页面可见性 + Tapp `ai:chat`）
///
/// 返回 Ok(is_admin)；禁用时返回 403 语义错误字符串
pub async fn ensure_agent_usage_allowed(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> Result<bool, String> {
    use crate::services::permission_service::{TappPermission, TappPermissionService, UserRole};

    let is_admin = user_is_current_admin(db, user_id).await;
    if is_admin || user_id == SYSTEM_USER_ID {
        return Ok(true);
    }

    let visibility = crate::services::module_visibility::agent_module_visibility(db).await;
    if visibility == "admin" {
        return Err("Agent 仅管理员可用".to_string());
    }

    // 能力真相源：Tapp 权限（设置页预设模板会批量开关这些项）
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    if !TappPermissionService::check(&config, UserRole::User, TappPermission::AiChat) {
        return Err(
            "Agent 未对普通用户开放 AI 对话（请在「Tapp 权限管理」中下放 ai:chat 或选用助手预设）"
                .to_string(),
        );
    }
    Ok(false)
}

/// Agent 权限串 → Tapp 权限（强制对齐；未映射的权限非管理员一律拒绝，仅 `system:read` 例外）
///
/// 非管理员能力 = 候选全集 ∩ Tapp 下放开关（与权限页预设模板同一真相源）
fn agent_perm_to_tapp(perm: &str) -> Option<crate::services::permission_service::TappPermission> {
    use crate::services::permission_service::TappPermission;
    match perm {
        // AI（elevated，须在 Tapp 权限管理中下放）
        "ai:chat" => Some(TappPermission::AiChat),
        "ai:analyze" => Some(TappPermission::AiAnalyze),
        "ai:image" => Some(TappPermission::AiImage),
        "ai:search" | "ai:generate" => Some(TappPermission::AiGenerate),
        // 读（basic，默认全员）
        "brew:read" => Some(TappPermission::BrewRead),
        "report:read" => Some(TappPermission::ReportRead),
        "platform:read" | "steam:read" | "bilibili:read" | "bangumi:read" | "github:read"
        | "netease:read" | "weather:read" | "metadata:read" => Some(TappPermission::PlatformRead),
        "tapp:read" => Some(TappPermission::TappListRead),
        // 写 / 出站（elevated 或 privileged）
        "brew:write" => Some(TappPermission::BrewWrite),
        "report:write" => Some(TappPermission::ReportWrite),
        "http:fetch" | "web:scrape" | "proxy:read" => Some(TappPermission::NetworkFetch),
        "scheduler:read" | "scheduler:write" => Some(TappPermission::SchedulerRegister),
        // 个人 Tapp 写：用 storage（basic）表达「可持久化自己的内容」，非 manage 全站
        "tapp:write" => Some(TappPermission::Storage),
        // system:read 无 Tapp 对应，见 retain 特例
        _ => None,
    }
}

/// 获取用户在 Agent 系统中的权限集
///
/// - 管理员 / 系统用户：全部能力权限
/// - 其他：候选全集 ∩ Tapp 权限检查（强制对齐；无独立 agentUsage 天花板）
pub async fn get_user_permissions(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
) -> std::collections::HashSet<String> {
    use crate::services::permission_service::{TappPermissionService, UserRole};
    use std::collections::HashSet;

    // 系统用户或管理员：全部权限
    if user_is_current_admin(db, user_id).await {
        let registry = capability::get_registry().await;
        let mut permissions: HashSet<String> = registry
            .get_all()
            .iter()
            .flat_map(|cap| cap.required_permissions.iter().cloned())
            .collect();
        permissions.insert("mcp:execute".to_string());
        return permissions;
    }

    let visibility = crate::services::module_visibility::agent_module_visibility(db).await;
    // 可见性 admin-only 时，非管理员无任何 agent 能力
    if visibility == "admin" {
        return HashSet::new();
    }

    let mut perms = max_user_agent_permissions();

    // 强制与 Tapp 对齐
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    perms.retain(|p| {
        if p == "system:read" {
            return true; // Agent 内部只读元信息，无 Tapp 对应
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

/// 清理过期的确认请求
pub async fn cleanup_expired_confirmations() {
    let now = Utc::now();
    let mut store = PENDING_CONFIRMATIONS.write().await;

    let expired: Vec<String> = store
        .iter()
        .filter(|(_, v)| v.request.expires_at < now)
        .map(|(k, _)| k.clone())
        .collect();

    for id in expired {
        tracing::debug!(confirmation_id = %id, "[Agent] Cleaning up expired confirmation");
        store.remove(&id);
    }
}

/// 将字段名转换为用户友好的标题
/// 子任务：AI 生成会话标题（在 tokio::spawn 中调用，与 executor 并行）
pub(crate) async fn generate_session_title_ai(user_input: &str, reasoning: Option<&str>) -> String {
    use crate::config::ModelTier;
    use crate::services::ai::create_ai_analyzer_for_tier;

    let truncated: String = user_input.chars().take(300).collect();

    if let Some(analyzer) = create_ai_analyzer_for_tier(ModelTier::Standard).await {
        let context = if let Some(r) = reasoning {
            format!(
                "User message: {}\nAgent understanding: {}",
                truncated,
                r.chars().take(200).collect::<String>()
            )
        } else {
            truncated.clone()
        };
        let prompt = format!(
            "Based on the following conversation context, generate a concise session title (5-15 characters, in the same language as the user). \
             Return ONLY the title text, no quotes, no explanation.\n\n{}",
            context
        );
        match analyzer.analyze(&prompt).await {
            Ok(raw) => {
                let cleaned = raw
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\u{300c}')
                    .trim_matches('\u{300d}')
                    .trim();
                if cleaned.is_empty() || cleaned.len() > 100 {
                    fallback_title_text(&truncated)
                } else {
                    cleaned.to_string()
                }
            }
            Err(e) => {
                tracing::warn!("[Agent] AI title generation failed: {}", e);
                fallback_title_text(&truncated)
            }
        }
    } else {
        fallback_title_text(&truncated)
    }
}

/// 标题降级：截取用户输入前 50 字符
fn fallback_title_text(input: &str) -> String {
    let count = input.chars().count();
    if count > 50 {
        format!("{}...", input.chars().take(47).collect::<String>())
    } else if count > 0 {
        input.to_string()
    } else {
        "New conversation".to_string()
    }
}

pub(crate) fn humanize_field_name(field: &str) -> String {
    // 常见字段名映射
    let mappings: &[(&str, &str)] = &[
        ("id", "ID"),
        ("title", "标题"),
        ("name", "名称"),
        ("content", "内容"),
        ("description", "描述"),
        ("desc", "描述"),
        ("summary", "摘要"),
        ("time", "时间"),
        ("date", "日期"),
        ("created_at", "创建时间"),
        ("updated_at", "更新时间"),
        ("author", "作者"),
        ("platform", "平台"),
        ("status", "状态"),
        ("progress", "进度"),
        ("count", "数量"),
        ("price", "价格"),
        ("url", "链接"),
        ("image", "图片"),
        ("cover", "封面"),
        ("thumbnail", "缩略图"),
        ("views", "浏览量"),
        ("likes", "点赞数"),
        ("comments", "评论数"),
        ("duration", "时长"),
        ("category", "分类"),
        ("tags", "标签"),
    ];

    // 查找映射
    for (key, label) in mappings {
        if field.to_lowercase() == *key || field.to_lowercase().ends_with(&format!("_{}", key)) {
            return label.to_string();
        }
    }

    // 默认处理：将 snake_case 或 camelCase 转换为空格分隔
    let mut result = String::new();
    for (i, c) in field.chars().enumerate() {
        if c == '_' {
            result.push(' ');
        } else if c.is_uppercase() && i > 0 {
            result.push(' ');
            result.push(c);
        } else if i == 0 {
            result.push(c.to_ascii_uppercase());
        } else {
            result.push(c);
        }
    }
    result
}

// 预执行参数收集 / 写回

/// 缺失的必需参数
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MissingRequiredParam {
    pub(crate) step_id: String,
    pub(crate) param_name: String,
    pub(crate) description: String,
}

/// 结构化 question_id，resume 时据此写回 Recipe.step.params
pub(crate) fn pre_param_question_id(step_id: &str, param_name: &str) -> String {
    format!("pre_param:{}:{}", step_id, param_name)
}

/// 解析 `pre_param:{step_id}:{param_name}`；兼容旧格式 `pre_param_{param_name}`
pub fn parse_pre_param_question_id(question_id: &str) -> Option<(String, String)> {
    if let Some(rest) = question_id.strip_prefix("pre_param:") {
        let mut parts = rest.splitn(2, ':');
        let step_id = parts.next()?.to_string();
        let param_name = parts.next()?.to_string();
        if step_id.is_empty() || param_name.is_empty() {
            return None;
        }
        return Some((step_id, param_name));
    }
    // 兼容旧版 pre_param_{name}（无 step 映射，调用方用第一个匹配步骤）
    if let Some(param_name) = question_id.strip_prefix("pre_param_") {
        if !param_name.is_empty() && param_name != "s" {
            return Some((String::new(), param_name.to_string()));
        }
    }
    None
}

/// 将用户回答写回 Recipe 对应步骤参数
pub fn apply_pre_param_answer_to_recipe(
    recipe: &mut Recipe,
    question_id: &str,
    answer: &str,
) -> bool {
    let Some((step_id, param_name)) = parse_pre_param_question_id(question_id) else {
        return false;
    };
    let value = serde_json::Value::String(answer.trim().to_string());
    if step_id.is_empty() {
        // 旧格式：写入第一个缺少该参数的步骤，否则第一个步骤
        let idx = recipe
            .steps
            .iter()
            .position(|s| !s.params.contains_key(&param_name))
            .or(if recipe.steps.is_empty() {
                None
            } else {
                Some(0)
            });
        if let Some(i) = idx {
            recipe.steps[i].params.insert(param_name, value);
            return true;
        }
        return false;
    }
    if let Some(step) = recipe.steps.iter_mut().find(|s| s.id == step_id) {
        step.params.insert(param_name, value);
        return true;
    }
    false
}

fn step_has_param_value(step: &types::RecipeStep, param_name: &str) -> bool {
    let has_value = step
        .params
        .get(param_name)
        .map(|v| !v.is_null() && v.as_str().is_none_or(|s| !s.is_empty()))
        .unwrap_or(false);
    let has_from = step.params.contains_key(&format!("{}From", param_name));
    has_value || has_from
}

fn push_missing_from_schema(
    missing: &mut Vec<MissingRequiredParam>,
    step: &types::RecipeStep,
    schema: &Value,
) {
    let Some(required) = schema.get("required").and_then(|v| v.as_array()) else {
        return;
    };
    let properties = schema.get("properties");
    for req_val in required {
        let Some(param_name) = req_val.as_str() else {
            continue;
        };
        if step_has_param_value(step, param_name) {
            continue;
        }
        let description = properties
            .and_then(|p| p.get(param_name))
            .and_then(|p| p.get("description"))
            .and_then(|d| d.as_str())
            .unwrap_or(param_name)
            .to_string();
        missing.push(MissingRequiredParam {
            step_id: step.id.clone(),
            param_name: param_name.to_string(),
            description,
        });
    }
}

/// 收集 Recipe 中缺失的必需参数（静态 registry + 动态 MCP schema + Skill）
pub(crate) async fn collect_missing_required_params(recipe: &Recipe) -> Vec<MissingRequiredParam> {
    let registry = capability::get_registry().await;
    let skill_registry = skill::get_skill_registry();
    let mcp_schemas = load_mcp_tool_schemas().await;
    let mut missing = Vec::new();

    for step in &recipe.steps {
        if let Some(skill_id) = step.capability_id.strip_prefix("skill:") {
            if let Some(skill_reg) = skill_registry {
                if let Some(sk) = skill_reg.get(skill_id).await {
                    for param_name in &sk.parameters {
                        if !step_has_param_value(step, param_name) {
                            missing.push(MissingRequiredParam {
                                step_id: step.id.clone(),
                                param_name: param_name.to_string(),
                                description: param_name.replace('_', " "),
                            });
                        }
                    }
                }
            }
            continue;
        }

        if let Some(cap) = registry.get(&step.capability_id) {
            push_missing_from_schema(&mut missing, step, &cap.input_schema);
            continue;
        }

        // 动态 MCP 工具：不在静态 registry 中，从 MCP manager 读 schema
        if step.capability_id.starts_with("mcp.") {
            if let Some(schema) = mcp_schemas.get(&step.capability_id) {
                push_missing_from_schema(&mut missing, step, schema);
            }
        }
    }

    missing
}

/// capability_id (`mcp.{server}.{tool}`) → input_schema
async fn load_mcp_tool_schemas() -> HashMap<String, Value> {
    let mut map = HashMap::new();
    let Some(manager) = mcp::get_mcp_manager() else {
        return map;
    };
    for (server_id, tool) in manager.list_tools().await {
        let cap_id = format!("mcp.{}.{}", server_id, tool.name);
        map.insert(cap_id, tool.input_schema);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(cap: &str, risk: RiskLevel) -> PendingConfirmation {
        PendingConfirmation {
            step_id: "s1".to_string(),
            capability_id: cap.to_string(),
            capability_name: cap.to_string(),
            description: String::new(),
            risk_level: risk,
            confirmation_message: String::new(),
            impact: Vec::new(),
        }
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

    #[test]
    fn session_id_is_parsed_from_lane_key() {
        assert_eq!(
            session_id_from_lane_key("user:42:session:ses_abc"),
            Some("ses_abc".to_string())
        );
        assert_eq!(session_id_from_lane_key("user:42"), None);
        assert_eq!(session_id_from_lane_key("user:42:session:"), None);
    }

    #[test]
    fn test_system_gate_normal_user_needs_confirmation() {
        let steps = vec![pending("cache.clear", RiskLevel::High)];
        assert!(
            Agent::system_sensitive_gate(1, &steps).is_none(),
            "普通用户应走正常确认流程"
        );
    }

    #[test]
    fn test_system_gate_auto_confirms_low_and_medium() {
        for risk in [RiskLevel::Low, RiskLevel::Medium] {
            let steps = vec![pending("storage.set", risk)];
            match Agent::system_sensitive_gate(SYSTEM_USER_ID, &steps) {
                Some(Ok(())) => {}
                other => panic!(
                    "系统任务应自动确认 {risk:?}，got {:?}",
                    other.map(|r| r.is_ok())
                ),
            }
        }
    }

    #[test]
    fn test_system_gate_blocks_high() {
        let steps = vec![pending("cache.clear", RiskLevel::High)];
        match Agent::system_sensitive_gate(SYSTEM_USER_ID, &steps) {
            Some(Err(resp)) => {
                assert!(resp.message.contains("cache.clear"), "{}", resp.message);
            }
            other => panic!("High 应被拒绝，got {:?}", other.map(|r| r.is_ok())),
        }
    }

    #[test]
    fn test_system_gate_blocks_critical() {
        let steps = vec![
            pending("storage.set", RiskLevel::Low),
            pending("system.shutdown", RiskLevel::Critical),
        ];
        match Agent::system_sensitive_gate(SYSTEM_USER_ID, &steps) {
            Some(Err(resp)) => {
                assert!(resp.message.contains("system.shutdown"), "{}", resp.message);
            }
            other => panic!("Critical 应被拒绝，got {:?}", other.map(|r| r.is_ok())),
        }
    }

    #[test]
    fn agent_usage_mode_parse_and_permission_sets() {
        assert_eq!(AgentUsageMode::parse("none"), AgentUsageMode::None);
        assert_eq!(AgentUsageMode::parse("chat"), AgentUsageMode::Chat);
        assert_eq!(AgentUsageMode::parse("standard"), AgentUsageMode::Standard);
        assert_eq!(AgentUsageMode::parse("elevated"), AgentUsageMode::Elevated);
        assert_eq!(AgentUsageMode::parse("bogus"), AgentUsageMode::None);

        let none = permissions_for_usage_mode(AgentUsageMode::None);
        assert!(none.is_empty());

        let chat = permissions_for_usage_mode(AgentUsageMode::Chat);
        assert!(chat.contains("ai:chat"));
        assert!(!chat.contains("brew:write"));
        assert!(!chat.contains("http:fetch"));

        let standard = permissions_for_usage_mode(AgentUsageMode::Standard);
        assert!(standard.contains("brew:read"));
        assert!(!standard.contains("brew:write"));
        assert!(standard.contains("ai:chat"));
        assert!(!standard.contains("http:fetch"));

        let elevated = permissions_for_usage_mode(AgentUsageMode::Elevated);
        assert!(elevated.contains("http:fetch"));
        assert!(elevated.contains("web:scrape"));
        assert!(elevated.contains("tapp:write"));
        assert!(elevated.contains("scheduler:read"));
        assert!(!elevated.contains("brew:write"));
        assert!(!elevated.contains("report:write"));

        let max = max_user_agent_permissions();
        assert_eq!(max, elevated);
    }

    #[test]
    fn agent_perm_to_tapp_force_alignment_map() {
        use crate::services::permission_service::TappPermission;
        assert_eq!(agent_perm_to_tapp("ai:chat"), Some(TappPermission::AiChat));
        assert_eq!(
            agent_perm_to_tapp("ai:search"),
            Some(TappPermission::AiGenerate)
        );
        assert_eq!(
            agent_perm_to_tapp("http:fetch"),
            Some(TappPermission::NetworkFetch)
        );
        assert_eq!(
            agent_perm_to_tapp("brew:read"),
            Some(TappPermission::BrewRead)
        );
        assert_eq!(
            agent_perm_to_tapp("report:write"),
            Some(TappPermission::ReportWrite)
        );
        assert_eq!(agent_perm_to_tapp("system:read"), None); // 特例：不经 Tapp
        assert_eq!(agent_perm_to_tapp("unknown:perm"), None);
    }

    #[test]
    fn saved_recipe_validation_rejects_dependency_cycles() {
        let mut recipe = Recipe::new("cycle", "cycle", ExecutionType::Instant);
        recipe.steps = vec![
            AiRecipeStep {
                id: "a".to_string(),
                capability_id: "ai.summarize".to_string(),
                action: "a".to_string(),
                params: HashMap::new(),
                depends_on: vec!["b".to_string()],
                on_failure: "abort".to_string(),
                retry: None,
                timeout_ms: None,
            }
            .into_recipe_step(0, None),
            AiRecipeStep {
                id: "b".to_string(),
                capability_id: "ai.summarize".to_string(),
                action: "b".to_string(),
                params: HashMap::new(),
                depends_on: vec!["a".to_string()],
                on_failure: "abort".to_string(),
                retry: None,
                timeout_ms: None,
            }
            .into_recipe_step(1, None),
        ];

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
            confirmation: None,
            frontend_action: None,
        };

        assert!(response(AgentResponseType::Answer, None).is_successful_outcome());
        assert!(
            !response(AgentResponseType::Answer, Some(json!({"blocked": true})))
                .is_successful_outcome()
        );
        assert!(!response(AgentResponseType::ConfirmationRequired, None).is_successful_outcome());
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

    #[test]
    fn pre_param_question_id_roundtrip() {
        let qid = pre_param_question_id("step_1", "tappId");
        assert_eq!(qid, "pre_param:step_1:tappId");
        assert_eq!(
            parse_pre_param_question_id(&qid),
            Some(("step_1".into(), "tappId".into()))
        );
        assert_eq!(
            parse_pre_param_question_id("pre_param_url"),
            Some(("".into(), "url".into()))
        );
        assert!(parse_pre_param_question_id("other").is_none());
    }

    fn sample_step(id: &str, cap: &str) -> types::RecipeStep {
        types::RecipeStep {
            id: id.into(),
            order: 0,
            capability_id: cap.into(),
            action: "act".into(),
            params: HashMap::new(),
            depends_on: vec![],
            on_failure: types::FailureStrategy::Abort,
            retry: None,
            timeout_ms: None,
            model_tier: None,
            generator: None,
        }
    }

    #[test]
    fn apply_pre_param_writes_back_to_recipe_step() {
        let mut recipe = Recipe::new("t", "open tapp", ExecutionType::Instant);
        recipe.steps.push(sample_step("step_1", "tapp.interact"));

        assert!(apply_pre_param_answer_to_recipe(
            &mut recipe,
            "pre_param:step_1:tappId",
            " my-tapp "
        ));
        assert_eq!(
            recipe.steps[0]
                .params
                .get("tappId")
                .and_then(|v| v.as_str()),
            Some("my-tapp")
        );
    }

    #[test]
    fn step_has_param_respects_from_refs() {
        let mut step = sample_step("s", "ai.summarize");
        assert!(!step_has_param_value(&step, "content"));
        step.params.insert("contentFrom".into(), json!("step_0"));
        assert!(step_has_param_value(&step, "content"));
    }
}
