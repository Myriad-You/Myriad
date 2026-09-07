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
pub const PLAN_DEPENDENCY_RULE: &str = "引擎按 `depends_on` 调度：依赖已满足的步骤立即并行启动，并行不需要你安排。\n\
- 互相独立、没有数据依赖 → `depends_on: []`，它们会同时开始\n\
- 步骤 B 要用步骤 A 的输出或情报 → B 必须 `depends_on: [\"A\"]`；多个步骤依赖同一个 A，A 完成后它们一起并行";

/// `xxxFrom` 取值约定。漏写 `depends_on` 是这条最常见的错法。
pub const PLAN_DATA_FLOW_RULE: &str = "后续步骤取前序输出，在 params 里写 `\"<字段名>From\": \"<step_id>\"`，引擎解析后注入同名参数（`\"promptFrom\": \"gen_prompt\"` → 把 gen_prompt 的输出注入 `prompt`）。\n\
- 用了 `xxxFrom` 就必须把那个步骤写进 `depends_on`。漏了会让两步并行、引用取到 null\n\
- 优先引用具体字段：`\"step_id.字段名\"`，字段名取自能力索引的 `o` 列表。`ai.webSearch` 的 `o` 含 `results`，就写 `\"dataFrom\": \"search.results\"`；引用整个步骤 ID 只在需要完整输出对象时用\n\
- 严禁 `$$variable$$` 之类的模板占位符。params 的值要么是具体文本，要么用 `xxxFrom` 引用";

/// 步骤上限。说清楚超出会被截掉，而不是只说「不要超」。
pub fn plan_step_cap_rule() -> String {
    format!("最多 {MAX_PLAN_STEPS} 个步骤，超出的会被引擎截掉。")
}

/// `ai.image` 的尺寸规则。数字取自实际生效的钳制常量，避免提示词自己漂。
pub fn plan_image_size_rule() -> String {
    format!(
        "`ai.image` 的尺寸由 `width` / `height` 决定（整数像素 {IMAGE_DIM_MIN}–{IMAGE_DIM_MAX}，省略则默认 {DEFAULT_IMAGE_WIDTH}×{DEFAULT_IMAGE_HEIGHT}），可以和 `promptFrom` 同时给。\n\
- 用户给了数字（\"512\"、\"1024x768\"、\"1920×1080\"）→ 按数字填\n\
- 竖图 / 手机壁纸 / 肖像 → 768×1024（或 768×1344）\n\
- 横图 / 桌面壁纸 / 风景 → 1024×768（或 1344×768）\n\
- 方图 / 头像 / 图标，或没提尺寸 → 省略，走默认\n\
- 不要把宽高写进 prompt 文本，写在 `ai.image` 的 params 里"
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
