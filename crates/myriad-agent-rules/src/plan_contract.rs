//! 执行计划契约：引擎真正的调度语义，以及要告诉模型的同一句话。
//!
//! Planner 和 Skill 内部 DAG 是两次不同的模型调用，面对的却是同一个引擎。
//! 之前两边各手抄一份提示词，已经漂开了——`on_failure` 与 `timeout_ms` 只在
//! Planner 那份里有，步骤上限那个 8 在五处各写一遍（Planner 的 schema、
//! Planner 的提示词、DAG 提示词、DAG 的 `take(8)`、`recipe.rs` 里一个私有常量）。
//!
//! 这里放两边都必须说、而且必须说得一样的那几条。各自独有的规则留在各自那边。

use crate::image::{DEFAULT_IMAGE_HEIGHT, DEFAULT_IMAGE_WIDTH, IMAGE_DIM_MAX, IMAGE_DIM_MIN};

/// 一个计划里的步骤上限。引擎在 `validate_and_convert_steps` 处截断，
/// 所以提示词、JSON schema 和截断点必须是同一个数。
pub const MAX_PLAN_STEPS: usize = 8;

/// `depends_on` 的调度语义。并行是引擎的事，模型只需要把依赖写对。
pub const PLAN_DEPENDENCY_RULE: &str = "The engine schedules by `depends_on`: steps whose dependencies are met start in parallel immediately. You do not arrange parallelism yourself.\n\
- Independent, no data dependency → `depends_on: []`; they start together\n\
- Step B needs A's output or intel → B must `depends_on: [\"A\"]`. Several steps depending on the same A run in parallel after A finishes";

/// `xxxFrom` 取值约定。漏写 `depends_on` 是这条最常见的错法。
pub const PLAN_DATA_FLOW_RULE: &str = "To take a prior step's output, write `\"<field>From\": \"<step_id>\"` in params. The engine injects that output into the same-named param (`\"promptFrom\": \"gen_prompt\"` → inject gen_prompt's output into `prompt`).\n\
- Any `xxxFrom` must also appear in `depends_on`. Missing it runs the two steps in parallel and the reference is null\n\
- Prefer a concrete field: `\"step_id.field\"`, using names from the capability index `o` list. `ai.webSearch` `o` includes `results`, so write `\"dataFrom\": \"search.results\"`. Cite the whole step id only when you need the full output object\n\
- Never use `$$variable$$` or similar template placeholders. Param values are either concrete text or an `xxxFrom` reference";

/// 步骤上限。说清楚超出会被截掉，而不是只说「不要超」。
pub fn plan_step_cap_rule() -> String {
    format!("At most {MAX_PLAN_STEPS} steps; the engine truncates the rest.")
}

/// `ai.image` 的尺寸规则。数字取自实际生效的钳制常量，避免提示词自己漂。
pub fn plan_image_size_rule() -> String {
    format!(
        "`ai.image` size is `width` / `height` (integer pixels {IMAGE_DIM_MIN}–{IMAGE_DIM_MAX}; omit to default {DEFAULT_IMAGE_WIDTH}×{DEFAULT_IMAGE_HEIGHT}). These can sit next to `promptFrom`.\n\
- User gave numbers (\"512\", \"1024x768\", \"1920×1080\") → use those numbers\n\
- Portrait / phone wallpaper / 竖图 / 肖像 → 768×1024 (or 768×1344)\n\
- Landscape / desktop wallpaper / 横图 / 风景 → 1024×768 (or 1344×768)\n\
- Square / avatar / icon, or no size mentioned → omit, use the default\n\
- Do not put width/height in the prompt text; put them in `ai.image` params"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 提示词里的数字必须是真正生效的那几个，不能各写各的。
    #[test]
    fn image_rule_quotes_the_enforced_limits() {
        let rule = plan_image_size_rule();
        assert!(rule.contains(&IMAGE_DIM_MIN.to_string()));
        assert!(rule.contains(&IMAGE_DIM_MAX.to_string()));
        assert!(rule.contains(&format!("{DEFAULT_IMAGE_WIDTH}×{DEFAULT_IMAGE_HEIGHT}")));
    }

    #[test]
    fn step_cap_rule_quotes_the_enforced_cap() {
        assert!(plan_step_cap_rule().contains(&MAX_PLAN_STEPS.to_string()));
    }

    /// 两条依赖规则要互相咬住：数据流那条必须提醒补 `depends_on`，
    /// 否则模型会写出并行执行、引用取到 null 的计划。
    #[test]
    fn data_flow_rule_points_back_at_depends_on() {
        assert!(PLAN_DATA_FLOW_RULE.contains("depends_on"));
        assert!(PLAN_DEPENDENCY_RULE.contains("depends_on"));
    }
}
