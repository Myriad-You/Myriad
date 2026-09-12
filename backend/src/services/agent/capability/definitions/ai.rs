//! AI 处理能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // AI 内容总结
    registry.register(Capability {
        id: "ai.summarize".to_string(),
        name: "Summarize".to_string(),
        description: "Summarize content with AI.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Summarize],
        input_schema: json!({
            "type": "object",
            "properties": {
                "content": { "type": "string" },
                "items": { "type": "array" },
                "style": { "type": "string", "enum": ["brief", "detailed", "bullet"] },
                "maxLength": { "type": "integer" }
            }
        }),
        // `execute_ai_summarize` 只返回摘要正文和回显的 style。
        output_schema: json!({
            "type": "object",
            "properties": {
                "format": { "type": "string" },
                "value": {
                    "type": "object",
                    "properties": {
                        "summary": { "type": "string", "description": "Summary body" },
                        "style": { "type": "string", "description": "Echoed summary style" }
                    }
                },
                "contextProvenance": { "type": "array" }
            }
        }),
        required_permissions: vec!["ai:analyze".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // AI 数据分析
    registry.register(Capability {
        id: "ai.analyze".to_string(),
        name: "Analyze".to_string(),
        description: "Analyze data in depth with AI.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {
                "data": { "type": "any" },
                "analysisType": { "type": "string", "enum": ["trend", "sentiment", "categorize", "custom"] },
                "instruction": { "type": "string" }
            },
            "required": ["data"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "format": { "type": "string" },
                "value": {
                    "type": "object",
                    "properties": {
                        "analysis": { "type": "string", "description": "Analysis body" },
                        "type": { "type": "string", "description": "Echoed analysisType" }
                    }
                },
                "contextProvenance": { "type": "array" }
            }
        }),
        required_permissions: vec!["ai:analyze".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(15000),
        ..Default::default()
    });

    // AI 推荐
    registry.register(Capability {
        id: "ai.recommend".to_string(),
        name: "Recommend".to_string(),
        description: "Recommend items from your data.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Recommend],
        input_schema: json!({
            "type": "object",
            "properties": {
                "context": { "type": "object" },
                "preferences": { "type": "object" },
                "count": { "type": "integer", "default": 5 }
            }
        }),
        // `execute_ai_recommend` 在 AI 没能给出合法 JSON 数组时会退化成原始文本，
        // 所以 `recommendations` 两种形状都合法。`reasoning` 从未被产出过。
        output_schema: json!({
            "type": "object",
            "properties": {
                "format": { "type": "string" },
                "value": {
                    "type": "object",
                    "properties": {
                        "recommendations": {
                            "type": ["array", "string"],
                            "description": "Recommendations; falls back to raw text if the model did not return a JSON array"
                        },
                        "count": { "type": "integer", "description": "Requested recommendation count" }
                    }
                },
                "contextProvenance": { "type": "array" }
            }
        }),
        required_permissions: vec!["ai:analyze".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(4000),
        ..Default::default()
    });

    // AI 图片生成
    registry.register(Capability {
        id: "ai.image".to_string(),
        name: "Generate image".to_string(),
        description: "Generate an image. Optional width and height (256–2048, default 1024)."
            .to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "prompt": { "type": "string", "description": "Image prompt" },
                "width": {
                    "type": "integer",
                    "minimum": 256,
                    "maximum": 2048,
                    "default": 1024,
                    "description": "Width in pixels; portrait 768, landscape 1024/1344; omit for 1024"
                },
                "height": {
                    "type": "integer",
                    "minimum": 256,
                    "maximum": 2048,
                    "default": 1024,
                    "description": "Height in pixels; portrait 1024/1344, landscape 768; omit for 1024"
                },
                "style": { "type": "string" }
            },
            "required": ["prompt"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "format": { "type": "string" },
                "value": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string" },
                        "width": { "type": "integer" },
                        "height": { "type": "integer" }
                    }
                },
                "contextProvenance": { "type": "array" }
            }
        }),
        required_permissions: vec!["ai:image".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(300_000),
        ..Default::default()
    });

    // AI 对话
    registry.register(Capability {
        id: "ai.chat".to_string(),
        name: "Chat".to_string(),
        description: "Have a free-form conversation with AI.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Query],
        input_schema: json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" },
                "context": { "type": "array" },
                "systemPrompt": { "type": "string" }
            },
            "required": ["message"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "format": { "type": "string" },
                "value": { "type": "string", "description": "Reply body" },
                "contextProvenance": { "type": "array" }
            }
        }),
        required_permissions: vec!["ai:chat".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // AI 联网搜索
    registry.register(Capability {
        id: "ai.webSearch".to_string(),
        name: "Web search".to_string(),
        description: "Search the web for live information.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![
            IntentAction::Query,
            IntentAction::Summarize,
            IntentAction::Analyze,
            IntentAction::Compare,
        ],
        input_schema: crate::services::agent::search_output::capability_input_schema(),
        output_schema: crate::services::agent::search_output::capability_output_schema(),
        required_permissions: vec!["ai:search".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(8000),
        ..Default::default()
    });

    // AI Grounding 搜索（独立 registry id，不是 `ai.webSearch` 别名；无 Compare）
    registry.register(Capability {
        id: "ai.groundingSearch".to_string(),
        name: "Grounded search".to_string(),
        description: "Search the web to check facts and get live information.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![
            IntentAction::Query,
            IntentAction::Summarize,
            IntentAction::Analyze,
        ],
        input_schema: crate::services::agent::search_output::capability_input_schema(),
        output_schema: crate::services::agent::search_output::capability_output_schema(),
        required_permissions: vec!["ai:search".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(8000),
        ..Default::default()
    });

    // AI 文章注释
    registry.register(Capability {
        id: "brewlia.annotate".to_string(),
        name: "Annotate article".to_string(),
        description: "Add AI notes and reading help to an article.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {
                "itemId": { "type": "integer" },
                "regenerate": { "type": "boolean", "default": false }
            },
            "required": ["itemId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "annotations": { "type": "array" },
                "fromCache": { "type": "boolean" },
                "itemId": { "type": "integer", "description": "Echoed article id" }
            }
        }),
        required_permissions: vec!["ai:analyze".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(5000),
        ..Default::default()
    });

    // AI 播客生成
    registry.register(Capability {
        id: "brewlia.podcast".to_string(),
        name: "Generate podcast".to_string(),
        description: "Turn an article into a spoken-dialogue script.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "itemId": { "type": "integer" },
                "style": { "type": "string", "enum": ["casual", "professional", "educational"] }
            },
            "required": ["itemId"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "script": { "type": "string" },
                "duration": { "type": "number" },
                "style": { "type": "string", "description": "Echoed podcast style" },
                "itemId": { "type": "integer", "description": "Echoed article id" }
            }
        }),
        required_permissions: vec!["ai:generate".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(8000),
        ..Default::default()
    });

    // 智能内容过滤
    registry.register(Capability {
        id: "smart.filter".to_string(),
        name: "Filter content".to_string(),
        description: "Classify and filter raw data.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string" },
                "options": { "type": "object" }
            },
            "required": ["platform"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string" },
                "status": { "type": "string" },
                "data": { "type": "object" },
                "analysis": { "type": "string" }
            }
        }),
        required_permissions: vec!["platform:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(2000),
        ..Default::default()
    });

    // 内容比较
    registry.register(Capability {
        id: "compare.content".to_string(),
        name: "Compare content".to_string(),
        description: "Compare platform data across time.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string" },
                "startDate": { "type": "string", "format": "date" },
                "endDate": { "type": "string", "format": "date" }
            },
            "required": ["platform"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "platform": { "type": "string" },
                "period": { "type": "object" },
                "analysis": { "type": "string" }
            }
        }),
        required_permissions: vec!["platform:read".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // 提示词生成
    registry.register(Capability {
        id: "prompt.generate".to_string(),
        name: "Generate prompt".to_string(),
        description: "Write a better image prompt from a description.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Topic / title" },
                "summary": { "type": "string", "description": "Short summary" },
                "description": { "type": "string", "description": "Detailed description: character full name, source work, looks (hair, eyes, outfit), scene, pose, style. Use descriptionFrom to cite a prior step" },
                "category": { "type": "string" },
                "style": { "type": "string", "description": "Style preference, e.g. anime, photorealistic, watercolor" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "prompt": { "type": "string" },
                "negativePrompt": { "type": "string" },
                "title": { "type": "string", "description": "Echoed title; empty string if the caller omitted it" }
            }
        }),
        required_permissions: vec!["ai:generate".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(15000),
        ..Default::default()
    });

    // 文本翻译
    registry.register(Capability {
        id: "translate.text".to_string(),
        name: "Translate".to_string(),
        description: "Translate text.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "targetLang": { "type": "string", "enum": ["zh-CN", "zh-TW", "en", "ja", "ko"], "default": "en-US" },
                "sourceLang": { "type": "string" }
            },
            "required": ["text"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "originalText": { "type": "string", "description": "Echoed source text" },
                "translated": { "type": "string" },
                "targetLang": { "type": "string" },
                "sourceLang": {
                    "type": ["string", "null"],
                    "description": "Caller-supplied source language; null if omitted"
                }
            }
        }),
        required_permissions: vec!["ai:analyze".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // 代码解释
    registry.register(Capability {
        id: "code.explain".to_string(),
        name: "Explain code".to_string(),
        description: "Explain what code does.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Analyze],
        input_schema: json!({
            "type": "object",
            "properties": {
                "code": { "type": "string" },
                "language": { "type": "string" }
            },
            "required": ["code"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "Truncated code echo" },
                "language": {
                    "type": ["string", "null"],
                    "description": "Caller-supplied language; null if omitted"
                },
                "explanation": { "type": "string" },
                "complexity": { "type": "string" }
            }
        }),
        required_permissions: vec!["ai:analyze".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(5000),
        ..Default::default()
    });

    // 图标推荐
    registry.register(Capability {
        id: "icon.recommend".to_string(),
        name: "Recommend icon".to_string(),
        description: "Recommend an icon for a platform name.".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Recommend],
        input_schema: json!({
            "type": "object",
            "properties": {
                "platformName": { "type": "string" }
            },
            "required": ["platformName"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "platformName": { "type": "string", "description": "Echoed platform name" },
                "iconType": { "type": "string" },
                "iconName": { "type": "string" },
                "colorSuggestion": { "type": "string" }
            }
        }),
        required_permissions: vec!["platform:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });
}
