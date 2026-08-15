//! Pure DAG topology / ready-set / failure-strategy scheduling for recipes.
//!
//! Domain owns cycle validation, parallel-mode gates, ready-step selection,
//! completion/failure marking, and dynamic step injection. The executor async
//! loop keeps I/O; this module has no Axum/DB/network I/O.
//!
//! `mark_failed` returns whether dependents are blocked (Abort strategy).


use std::collections::{HashMap, HashSet};

use crate::services::agent::types::{FailureStrategy, RecipeStep};

/// DAG 调度器
#[derive(Debug)]
pub struct DagScheduler {
    /// step_id -> 步骤定义
    steps: HashMap<String, RecipeStep>,
    /// 已完成的步骤（成功或 Skip/Continue 策略的失败步骤）
    completed: HashSet<String>,
    /// 已失败的步骤（Abort 策略，阻塞依赖链）
    failed: HashSet<String>,
    /// 步骤执行顺序（用于无依赖时的默认顺序）
    order: Vec<String>,
}

impl DagScheduler {
    /// 从 RecipeSteps 构建 DAG
    ///
    /// 如果所有步骤的 `depends_on` 都为空，则按 `order` 字段顺序执行（兼容现有行为）。
    /// 如果有依赖关系，则构建 DAG 并并行执行。
    pub fn new(steps: &[RecipeStep]) -> Result<Self, String> {
        let mut step_map = HashMap::new();
        let mut order: Vec<String> = Vec::new();

        for step in steps {
            step_map.insert(step.id.clone(), step.clone());
            order.push(step.id.clone());
        }

        let scheduler = Self {
            steps: step_map,
            completed: HashSet::new(),
            failed: HashSet::new(),
            order,
        };

        // 校验无环（拓扑排序检测）
        scheduler.validate_no_cycles()?;

        Ok(scheduler)
    }

    /// 是否使用并行模式
    ///
    /// 当有 2 个以上步骤时启用并行模式：
    /// - 有显式 `depends_on` 时：按 DAG 拓扑排序，依赖满足的步骤并行执行
    /// - 全部 `depends_on` 为空时：所有步骤视为独立，整波并行执行
    ///
    /// 仅当唯一 1 个步骤时退化为顺序执行。
    pub fn is_parallel_mode(&self) -> bool {
        self.steps.len() > 1
    }

    /// 获取当前可执行的步骤（所有依赖已满足）
    pub fn get_ready_steps(&self) -> Vec<RecipeStep> {
        if !self.is_parallel_mode() {
            // 顺序模式：返回下一个未完成的步骤
            for id in &self.order {
                if !self.completed.contains(id) {
                    if let Some(step) = self.steps.get(id) {
                        return vec![step.clone()];
                    }
                }
            }
            return vec![];
        }

        // 并行模式：找出所有依赖已完成且未被失败阻塞的步骤
        self.steps
            .values()
            .filter(|step| {
                // 未完成且未失败
                !self.completed.contains(&step.id)
                    && !self.failed.contains(&step.id)
                    // 所有依赖已完成（成功或 Skip/Continue 策略）
                    && step
                        .depends_on
                        .iter()
                        .all(|dep| self.completed.contains(dep))
                    // 没有任何依赖被 Abort 策略阻塞
                    && !step.depends_on.iter().any(|dep| self.failed.contains(dep))
            })
            .cloned()
            .collect()
    }

    /// 动态注入新步骤（如 Skill 生成的子步骤）
    ///
    /// 新步骤的 `depends_on` 可引用已有步骤（含已完成的）或同批注入的步骤。
    /// 注入后重新校验无环。
    pub fn add_steps(&mut self, steps: &[RecipeStep]) -> Result<(), String> {
        for step in steps {
            self.steps.insert(step.id.clone(), step.clone());
            self.order.push(step.id.clone());
        }
        self.validate_no_cycles()
    }

    /// 是否还有未完成的步骤
    pub fn has_remaining(&self) -> bool {
        self.steps.len() > self.completed.len() + self.failed.len()
    }

    /// 标记步骤成功完成
    pub fn mark_completed(&mut self, step_id: &str) {
        self.completed.insert(step_id.to_string());
    }

    /// 标记步骤失败，根据 on_failure 策略决定是否阻塞依赖链
    ///
    /// - Skip/Continue: 视为"完成"（依赖步骤可继续执行）
    /// - Abort: 标记为失败（依赖步骤将被跳过）
    /// Mark step failed. Returns `true` if dependents are blocked (Abort).
    pub fn mark_failed(&mut self, step_id: &str, strategy: &FailureStrategy) -> bool {
        match strategy {
            FailureStrategy::Abort => {
                // 失败且阻塞，依赖步骤将不会被执行
                self.failed.insert(step_id.to_string());
                true
            }
            _ => {
                // Skip / UseDefault / Fallback：失败但不阻塞，视为已完成
                self.completed.insert(step_id.to_string());
                false
            }
        }
    }

    /// 拓扑排序检测环
    fn validate_no_cycles(&self) -> Result<(), String> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();

        // 初始化
        for id in self.steps.keys() {
            in_degree.insert(id.as_str(), 0);
        }

        // 构建入度
        for step in self.steps.values() {
            for dep in &step.depends_on {
                if !self.steps.contains_key(dep) {
                    return Err(format!(
                        "Step '{}' depends on non-existent step '{}'",
                        step.id, dep
                    ));
                }
                adj.entry(dep.as_str()).or_default().push(step.id.as_str());
                *in_degree.entry(step.id.as_str()).or_default() += 1;
            }
        }

        // Kahn's algorithm
        let mut queue: Vec<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();
        let mut visited = 0;

        while let Some(node) = queue.pop() {
            visited += 1;
            if let Some(neighbors) = adj.get(node) {
                for &next in neighbors {
                    if let Some(deg) = in_degree.get_mut(next) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(next);
                        }
                    }
                }
            }
        }

        if visited != self.steps.len() {
            return Err("Circular dependency detected in recipe steps".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::types::{FailureStrategy, RecipeStep};
    use std::collections::HashMap;

    fn make_step(id: &str, order: u32, depends_on: Vec<&str>) -> RecipeStep {
        RecipeStep {
            id: id.to_string(),
            order,
            capability_id: format!("test.{}", id),
            action: "test".to_string(),
            params: HashMap::new(),
            depends_on: depends_on.into_iter().map(String::from).collect(),
            on_failure: FailureStrategy::Abort,
            retry: None,
            timeout_ms: None,
            model_tier: None,
            generator: None,
        }
    }

    #[test]
    fn test_single_step_sequential() {
        let steps = vec![make_step("a", 0, vec![])];
        let scheduler = DagScheduler::new(&steps).unwrap();
        assert!(!scheduler.is_parallel_mode());

        let ready = scheduler.get_ready_steps();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "a");
    }

    #[test]
    fn test_independent_steps_parallel() {
        // 多个独立步骤（全部 depends_on 为空）应自动并行
        let steps = vec![
            make_step("a", 0, vec![]),
            make_step("b", 1, vec![]),
            make_step("c", 2, vec![]),
        ];
        let scheduler = DagScheduler::new(&steps).unwrap();
        assert!(scheduler.is_parallel_mode());

        let ready = scheduler.get_ready_steps();
        assert_eq!(
            ready.len(),
            3,
            "All independent steps should be ready simultaneously"
        );
    }

    #[test]
    fn test_parallel_mode() {
        let steps = vec![
            make_step("fetch_steam", 0, vec![]),
            make_step("fetch_bili", 1, vec![]),
            make_step("analyze", 2, vec!["fetch_steam", "fetch_bili"]),
        ];
        let mut scheduler = DagScheduler::new(&steps).unwrap();
        assert!(scheduler.is_parallel_mode());

        // Both fetch steps should be ready
        let ready = scheduler.get_ready_steps();
        assert_eq!(ready.len(), 2);

        // Complete both
        scheduler.mark_completed("fetch_steam");
        scheduler.mark_completed("fetch_bili");

        // Now analyze should be ready
        let ready = scheduler.get_ready_steps();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "analyze");
    }

    #[test]
    fn test_cycle_detection() {
        let steps = vec![
            make_step("a", 0, vec!["c"]),
            make_step("b", 1, vec!["a"]),
            make_step("c", 2, vec!["b"]),
        ];
        let result = DagScheduler::new(&steps);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Circular dependency"));
    }

    #[test]
    fn test_missing_dependency() {
        let steps = vec![make_step("a", 0, vec!["nonexistent"])];
        let result = DagScheduler::new(&steps);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("non-existent"));
    }

    #[test]
    fn test_failure_abort_blocks_dependents() {
        // fetch_steam → analyze depends on fetch_steam
        let steps = vec![
            make_step("fetch_steam", 0, vec![]),
            make_step("fetch_bili", 1, vec![]),
            make_step("analyze", 2, vec!["fetch_steam", "fetch_bili"]),
        ];
        let mut scheduler = DagScheduler::new(&steps).unwrap();

        // fetch_steam fails with Abort
        scheduler.mark_failed("fetch_steam", &FailureStrategy::Abort);
        // fetch_bili succeeds
        scheduler.mark_completed("fetch_bili");

        // analyze should NOT be ready (fetch_steam failed with Abort)
        let ready = scheduler.get_ready_steps();
        assert!(
            ready.is_empty(),
            "analyze should be blocked by failed dependency"
        );
    }

    #[test]
    fn test_failure_skip_allows_dependents() {
        let steps = vec![
            make_step("fetch_steam", 0, vec![]),
            make_step("fetch_bili", 1, vec![]),
            make_step("analyze", 2, vec!["fetch_steam", "fetch_bili"]),
        ];
        let mut scheduler = DagScheduler::new(&steps).unwrap();

        // fetch_steam fails with Skip (non-blocking)
        scheduler.mark_failed("fetch_steam", &FailureStrategy::Skip);
        // fetch_bili succeeds
        scheduler.mark_completed("fetch_bili");

        // analyze SHOULD be ready (Skip = treat as completed)
        let ready = scheduler.get_ready_steps();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "analyze");
    }

    #[test]
    fn test_add_steps_dynamic_injection() {
        // 模拟：初始只有 1 个 skill 步骤，执行后动态注入并行子步骤
        let skill_step = vec![make_step("skill_main", 0, vec![])];
        let mut scheduler = DagScheduler::new(&skill_step).unwrap();
        assert!(!scheduler.is_parallel_mode()); // 1 步骤 → 顺序

        // 标记 skill 步骤完成
        scheduler.mark_completed("skill_main");
        assert!(!scheduler.has_remaining());

        // 注入 3 组 prompt+image 动态步骤（模拟多变体并行）
        let dynamic_steps = vec![
            make_step("gen_prompt_1", 10, vec![]),
            make_step("gen_prompt_2", 11, vec![]),
            make_step("gen_prompt_3", 12, vec![]),
            make_step("gen_image_1", 20, vec!["gen_prompt_1"]),
            make_step("gen_image_2", 21, vec!["gen_prompt_2"]),
            make_step("gen_image_3", 22, vec!["gen_prompt_3"]),
        ];
        scheduler.add_steps(&dynamic_steps).unwrap();
        assert!(scheduler.is_parallel_mode()); // 7 步骤 → 并行
        assert!(scheduler.has_remaining());

        // Wave 1: 所有 prompt 步骤并行就绪
        let ready = scheduler.get_ready_steps();
        let mut ids: Vec<String> = ready.iter().map(|s| s.id.clone()).collect();
        ids.sort();
        assert_eq!(ids, vec!["gen_prompt_1", "gen_prompt_2", "gen_prompt_3"]);

        // 完成所有 prompt
        for id in &ids {
            scheduler.mark_completed(id);
        }

        // Wave 2: 所有 image 步骤并行就绪
        let ready = scheduler.get_ready_steps();
        let mut ids: Vec<String> = ready.iter().map(|s| s.id.clone()).collect();
        ids.sort();
        assert_eq!(ids, vec!["gen_image_1", "gen_image_2", "gen_image_3"]);

        // 完成所有 image
        for id in &ids {
            scheduler.mark_completed(id);
        }
        assert!(!scheduler.has_remaining());
    }
}
