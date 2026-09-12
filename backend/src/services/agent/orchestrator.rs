//! Recipe role analysis for identity injection.
//!
//! Groups steps by Agent role, reports `can_parallelize`, and loads role
//! identity contexts. Does not execute or schedule groups.

use std::collections::HashMap;

use super::identity::get_role_identity;
use super::routing::{get_router, AgentRole, TaskAssignment};
use super::types::Recipe;

/// Multi-Agent Orchestrator
pub struct Orchestrator;

impl Orchestrator {
    /// 分析 Recipe 的角色分布
    pub fn analyze_recipe(recipe: &Recipe) -> (TaskAssignment, usize, bool) {
        let router = get_router();

        // 收集能力 ID 和角色映射
        let capability_ids: Vec<String> = recipe
            .steps
            .iter()
            .map(|s| s.capability_id.clone())
            .collect();
        let assignment = router.summarize_assignment(&capability_ids);

        // 按角色分组步骤
        let mut role_groups: HashMap<AgentRole, Vec<usize>> = HashMap::new();
        for (idx, step) in recipe.steps.iter().enumerate() {
            let role = router.route_capability(&step.capability_id);
            role_groups.entry(role).or_default().push(idx);
        }

        // 分析跨角色依赖
        let group_dependencies: Vec<Vec<AgentRole>> = role_groups
            .into_values()
            .map(|step_indices| Self::find_cross_role_deps(recipe, &step_indices))
            .collect();
        let role_group_count = group_dependencies.len();
        let can_parallelize = role_group_count >= 2
            && group_dependencies
                .iter()
                .any(|dependencies| dependencies.is_empty());

        (assignment, role_group_count, can_parallelize)
    }

    /// 检查步骤的跨角色依赖
    fn find_cross_role_deps(recipe: &Recipe, step_indices: &[usize]) -> Vec<AgentRole> {
        let router = get_router();
        let mut deps = Vec::new();

        for &idx in step_indices {
            if let Some(step) = recipe.steps.get(idx) {
                // 检查 depends_on 引用的步骤是否属于其他角色
                for dep_id in &step.depends_on {
                    if let Some(dep_step) = recipe.steps.iter().find(|s| s.id == *dep_id) {
                        let dep_role = router.route_capability(&dep_step.capability_id);
                        let my_role = router.route_capability(&step.capability_id);
                        if dep_role != my_role && !deps.contains(&dep_role) {
                            deps.push(dep_role);
                        }
                    }
                }
            }
        }

        deps
    }

    /// 获取参与角色的 identity 上下文（注入到 AI 步骤的系统 prompt）
    pub async fn get_role_contexts(recipe: &Recipe) -> HashMap<AgentRole, String> {
        let router = get_router();
        let mut contexts = HashMap::new();

        let mut roles_seen = Vec::new();
        for step in &recipe.steps {
            let role = router.route_capability(&step.capability_id);
            if !roles_seen.contains(&role) {
                roles_seen.push(role);
            }
        }

        for role in roles_seen {
            if let Some(identity) = get_role_identity(role).await {
                contexts.insert(role, identity);
            }
        }

        contexts
    }
}
