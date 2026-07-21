//! 结果评估与智能升级模块
//!
//! 提供结果评估器（失败模式检测、满意度评分），
//! 由 Planner.replan() 消费评估结果来驱动升级决策。
//!
//! ## 模块状态
//!
//! - `evaluator` — 活跃：ResultEvaluator 被 Agent 的 should_escalate / build_escalation_hint 使用
//! - `capability_mapping` / `strategy` — 已移除：v3 Planner 直接通过 AI 决定升级路径，
//!   静态映射表不再被消费。如未来需要恢复，可从 git 历史中找回。

mod evaluator;

// 核心导出：ResultEvaluator 供 Agent.should_escalate / build_escalation_hint 使用
pub use evaluator::{EvaluationContext, ResultEvaluator};
