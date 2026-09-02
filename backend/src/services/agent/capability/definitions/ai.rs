//! AI 处理能力定义

use crate::services::agent::types::*;
use serde_json::json;

use super::super::CapabilityRegistry;

pub fn register(registry: &mut CapabilityRegistry) {
    // AI 内容总结
    registry.register(Capability {
        id: "ai.summarize".to_string(),
        name: "AI 内容总结".to_string(),
        description: "使用 AI 对内容进行智能总结".to_string(),
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
        // `execute_ai_summarize` 只返回摘要正文和回显的 style；声明过的 `keyPoints`
        // 从未被产出，留在这里只会让 Planner 去引用一个取不到的字段。
        output_schema: json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string", "description": "摘要正文" },
                "style": { "type": "string", "description": "回显的摘要风格" }
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
        name: "AI 数据分析".to_string(),
        description: "使用 AI 进行深度数据分析".to_string(),
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
                "analysis": { "type": "string", "description": "AI 分析正文" },
                "type": { "type": "string", "description": "回显的 analysisType" }
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
        name: "AI 推荐".to_string(),
        description: "基于用户数据进行智能推荐".to_string(),
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
                "recommendations": {
                    "type": ["array", "string"],
                    "description": "推荐列表；AI 未返回合法 JSON 数组时退化为原始文本"
                },
                "count": { "type": "integer", "description": "请求的推荐条数" }
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
        name: "AI 图片生成".to_string(),
        description: "使用 AI 生成图片；可选 width/height（像素 256–2048，默认 1024）指定分辨率"
            .to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "prompt": { "type": "string", "description": "图片生成提示词" },
                "width": {
                    "type": "integer",
                    "minimum": 256,
                    "maximum": 2048,
                    "default": 1024,
                    "description": "宽度像素；竖图建议 768，横图建议 1024/1344，省略默认 1024"
                },
                "height": {
                    "type": "integer",
                    "minimum": 256,
                    "maximum": 2048,
                    "default": 1024,
                    "description": "高度像素；竖图建议 1024/1344，横图建议 768，省略默认 1024"
                },
                "style": { "type": "string" }
            },
            "required": ["prompt"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "imageUrl": { "type": "string" },
                "width": { "type": "integer" },
                "height": { "type": "integer" }
            }
        }),
        required_permissions: vec!["ai:image".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(90000),
        ..Default::default()
    });

    // AI 对话
    registry.register(Capability {
        id: "ai.chat".to_string(),
        name: "AI 对话".to_string(),
        description: "与 AI 进行自由对话".to_string(),
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
                "reply": { "type": "string" }
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
        name: "AI 联网搜索".to_string(),
        description: "通过 AI 联网搜索获取实时信息（如 RSS 源、API 文档等）".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![
            IntentAction::Query,
            IntentAction::Summarize,
            IntentAction::Analyze,
            IntentAction::Compare,
        ],
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "搜索查询内容" },
                "searchType": { 
                    "type": "string", 
                    "enum": ["rss_source", "api_docs", "general"],
                    "default": "general",
                    "description": "搜索类型：rss_source 搜索 RSS 源，api_docs 搜索 API 文档，general 通用搜索"
                },
                "resultFormat": { 
                    "type": "string", 
                    "enum": ["url", "json", "text"],
                    "default": "json",
                    "description": "结果格式：url 返回链接列表，json 返回结构化数据，text 返回纯文本"
                },
                "maxResults": { 
                    "type": "integer", 
                    "default": 5,
                    "description": "最大返回结果数"
                },
                "source": { 
                    "type": "string",
                    "description": "搜索来源提示"
                },
                "searchPrompt": {
                    "type": "string",
                    "description": "自定义搜索提示词"
                }
            },
            "required": ["query"]
        }),
        // 与 `execute_gemini_grounding_search_wrapper` 的实际返回一致。原先声明的
        // `source` / `searchPrompt` 从未被产出，而真正有用的 `aiSummary` /
        // `totalResults` 反倒没被声明——Planner 因此看不到它们。
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "query": { "type": "string", "description": "回显的查询" },
                "searchType": { "type": "string", "description": "回显的搜索类型" },
                "aiSummary": { "type": "string", "description": "AI 对搜索结果的综述" },
                "results": { "type": "array", "description": "搜索结果列表" },
                "totalResults": { "type": "integer", "description": "结果条数" }
            }
        }),
        required_permissions: vec!["ai:search".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(8000),
        ..Default::default()
    });

    // AI Grounding 搜索（ai.webSearch 的别名，用于强调事实性搜索）
    registry.register(Capability {
        id: "ai.groundingSearch".to_string(),
        name: "AI Grounding 搜索".to_string(),
        description: "通过 AI 联网搜索验证事实、获取实时信息".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![
            IntentAction::Query,
            IntentAction::Summarize,
            IntentAction::Analyze,
        ],
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "搜索查询内容" },
                "searchType": {
                    "type": "string",
                    "enum": ["rss_source", "api_docs", "general"],
                    "default": "general"
                },
                "resultFormat": {
                    "type": "string",
                    "enum": ["url", "json", "text"],
                    "default": "json"
                },
                "maxResults": { "type": "integer", "default": 5 },
                "searchPrompt": { "type": "string", "description": "自定义搜索提示词" }
            },
            "required": ["query"]
        }),
        // 与 ai.webSearch 共用 `execute_gemini_grounding_search_wrapper`，返回同一形状。
        output_schema: json!({
            "type": "object",
            "properties": {
                "success": { "type": "boolean" },
                "query": { "type": "string", "description": "回显的查询" },
                "searchType": { "type": "string", "description": "回显的搜索类型" },
                "aiSummary": { "type": "string", "description": "AI 对搜索结果的综述" },
                "results": { "type": "array", "description": "搜索结果列表" },
                "totalResults": { "type": "integer", "description": "结果条数" }
            }
        }),
        required_permissions: vec!["ai:search".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(8000),
        ..Default::default()
    });

    // AI 文章注释
    registry.register(Capability {
        id: "brewlia.annotate".to_string(),
        name: "AI 文章注释".to_string(),
        description: "为文章生成 AI 智能注释和解读".to_string(),
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
                "fromCache": { "type": "boolean" }
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
        name: "AI 播客生成".to_string(),
        description: "将文章转换为对话式播客文稿".to_string(),
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
                "duration": { "type": "number" }
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
        name: "智能内容过滤".to_string(),
        description: "对原始数据进行智能分类和过滤".to_string(),
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
        required_permissions: vec!["filter:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(2000),
        ..Default::default()
    });

    // 内容比较
    registry.register(Capability {
        id: "compare.content".to_string(),
        name: "内容比较".to_string(),
        description: "比较不同时间点的平台数据变化".to_string(),
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
        required_permissions: vec!["compare:read".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(3000),
        ..Default::default()
    });

    // 提示词生成
    registry.register(Capability {
        id: "prompt.generate".to_string(),
        name: "提示词生成".to_string(),
        description: "为图片生成提供优化的提示词。在 description 中传入详细描述（角色名、出处、外貌特征、场景和风格等），或用 descriptionFrom 引用前序步骤的输出".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "主题/标题" },
                "summary": { "type": "string", "description": "简要说明" },
                "description": { "type": "string", "description": "详细描述：角色全名、来源作品、外貌特征（发型发色、瞳色、服装配饰等）、场景、姿势、画风等。可用 descriptionFrom 引用前序步骤" },
                "category": { "type": "string" },
                "style": { "type": "string", "description": "画风偏好，例如 anime, photorealistic, watercolor 等" }
            }
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "prompt": { "type": "string" },
                "negativePrompt": { "type": "string" }
            }
        }),
        required_permissions: vec!["prompt:write".to_string()],
        requires_ai: true,
        estimated_duration_ms: Some(15000),
        ..Default::default()
    });

    // 文本翻译
    registry.register(Capability {
        id: "translate.text".to_string(),
        name: "文本翻译".to_string(),
        description: "使用 AI 翻译文本内容，支持中英日韩等多语言".to_string(),
        category: CapabilityCategory::AiProcess,
        supported_actions: vec![IntentAction::Create],
        input_schema: json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "targetLang": { "type": "string", "enum": ["zh-CN", "zh-TW", "en", "ja", "ko"], "default": "zh-CN" },
                "sourceLang": { "type": "string" }
            },
            "required": ["text"]
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "translated": { "type": "string" },
                "targetLang": { "type": "string" }
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
        name: "代码解释".to_string(),
        description: "使用 AI 解释代码功能和逻辑".to_string(),
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
        name: "图标推荐".to_string(),
        description: "根据平台名称推荐合适的图标".to_string(),
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
                "iconType": { "type": "string" },
                "iconName": { "type": "string" },
                "colorSuggestion": { "type": "string" }
            }
        }),
        required_permissions: vec!["icon:read".to_string()],
        requires_ai: false,
        estimated_duration_ms: Some(100),
        ..Default::default()
    });
}
