//! 能力系统工具函数
//!
//! 包含能力名称映射、步骤描述、风险评估等辅助函数

use super::super::types::{RecipeStep, RiskLevel};
use serde_json::{json, Value};
use std::collections::HashMap;

/// 获取能力的友好名称
pub fn get_capability_friendly_name(capability_id: &str) -> String {
    match capability_id {
        "platform.read" => "获取平台数据".to_string(),
        "platform.bilibili" => "获取 B 站数据".to_string(),
        "platform.steam" => "获取 Steam 数据".to_string(),
        "platform.github" => "获取 GitHub 数据".to_string(),
        "platform.netease" => "获取网易云数据".to_string(),
        "platform.mal" => "获取 MyAnimeList 数据".to_string(),
        "ai.summarize" => "AI 总结".to_string(),
        "ai.analyze" => "AI 分析".to_string(),
        "ai.chat" => "AI 对话".to_string(),
        "ai.webSearch" => "网络搜索".to_string(),
        "brew.discover" => "发现 RSS 源".to_string(),
        "brew.subscribe" => "订阅 RSS 源".to_string(),
        "brew.read" => "获取订阅内容".to_string(),
        "brew.items" => "获取文章列表".to_string(),
        "brew.article" => "获取文章内容（仅在已知ID或URL的情况下使用）".to_string(),
        "brew.sources" => "获取订阅源".to_string(),
        "brew.stats" => "阅读统计".to_string(),
        "tapp.list" => "获取 Tapp 列表".to_string(),
        "tapp.page" => "获取 Tapp 页面".to_string(),
        "ai.image" => "生成图片".to_string(),
        "prompt.generate" => "生成提示词".to_string(),
        "compare.content" => "内容对比".to_string(),
        "speech.tts" => "文字转语音".to_string(),
        "search.global" => "全局搜索".to_string(),
        "report.generate" => "生成报告".to_string(),
        _ => {
            // Skill 能力：从 ID 提取可读名称
            if let Some(skill_id) = capability_id.strip_prefix("skill:") {
                let clean = skill_id.strip_prefix("_auto_").unwrap_or(skill_id);
                // 将连字符和下划线替换为空格
                let name: String = clean
                    .chars()
                    .map(|c| if c == '-' || c == '_' { ' ' } else { c })
                    .collect();
                let trimmed = name.trim();
                if !trimmed.is_empty() {
                    return format!("执行技能: {}", truncate_str(trimmed, 20));
                }
                return "执行技能".to_string();
            }
            // MCP 工具：提取工具名
            if let Some(rest) = capability_id.strip_prefix("mcp.") {
                // mcp.server_id.tool_name → 取最后一段
                if let Some(tool_name) = rest.rsplit('.').next() {
                    return format!("调用工具: {}", tool_name);
                }
                return "调用外部工具".to_string();
            }
            // 尝试提取友好名称
            if let Some(name) = capability_id.split('.').next_back() {
                // 将 camelCase 转换为空格分隔
                let mut result = String::new();
                for (i, c) in name.chars().enumerate() {
                    if i > 0 && c.is_uppercase() {
                        result.push(' ');
                    }
                    result.push(c);
                }
                result
            } else {
                capability_id.to_string()
            }
        }
    }
}

/// 获取步骤的详细描述（带上下文参数）
/// 返回类似 "搜索知乎日报"、"订阅知乎日报" 的描述
pub fn get_step_description(step: &RecipeStep) -> String {
    // Skill 和 MCP 步骤：使用 planner 提供的 action 描述（已是人类可读的）
    if step.capability_id.starts_with("skill:") || step.capability_id.starts_with("mcp.") {
        let action = step.action.trim();
        if !action.is_empty() {
            return truncate_str(action, 30);
        }
        // action 为空时，回退到 capability 友好名称
        return get_capability_friendly_name(&step.capability_id);
    }

    let params = &step.params;

    // 尝试从参数中提取关键信息
    let query = params.get("query").and_then(|v| v.as_str());
    let name = params.get("name").and_then(|v| v.as_str());
    let url = params.get("url").and_then(|v| v.as_str());
    let platform = params.get("platform").and_then(|v| v.as_str());
    let topic = params.get("topic").and_then(|v| v.as_str());
    let page_type = params.get("pageType").and_then(|v| v.as_str());

    // 获取主要目标名称
    let target = query
        .or(name)
        .or(topic)
        .map(|s| s.to_string())
        .unwrap_or_default();

    // 获取页面类型的友好名称
    let page_name = page_type.map(|pt| match pt {
        "brew" => "订阅",
        "tapp" => "应用",
        "report" => "报告",
        "dashboard" => "仪表盘",
        _ => "当前页面",
    });

    match step.capability_id.as_str() {
        // RSS/Brew 相关
        "brew.discover" => {
            if !target.is_empty() {
                format!("搜索 {}", target)
            } else if let Some(u) = url {
                format!("发现 RSS 源: {}", truncate_str(u, 30))
            } else {
                "发现 RSS 源".to_string()
            }
        }
        "brew.subscribe" => {
            if !target.is_empty() {
                format!("订阅 {}", target)
            } else if let Some(u) = url {
                format!("订阅: {}", truncate_str(u, 30))
            } else {
                "订阅 RSS 源".to_string()
            }
        }
        "brew.read" | "brew.list" => {
            if let Some(pn) = page_name {
                format!("获取{}内容", pn)
            } else if !target.is_empty() {
                format!("获取 {} 的内容", target)
            } else {
                "获取订阅内容".to_string()
            }
        }
        "brew.items" => {
            if !target.is_empty() {
                format!("获取 {} 的文章列表", target)
            } else {
                "获取文章列表".to_string()
            }
        }
        "brew.article" => {
            if !target.is_empty() {
                format!("获取文章: {}", truncate_str(&target, 30))
            } else {
                "获取文章内容".to_string()
            }
        }

        // AI 相关
        "ai.webSearch" => {
            if !target.is_empty() {
                format!("搜索: {}", truncate_str(&target, 20))
            } else {
                "网络搜索".to_string()
            }
        }
        "ai.summarize" => {
            if let Some(pn) = page_name {
                format!("总结{}内容", pn)
            } else if !target.is_empty() {
                format!("总结: {}", truncate_str(&target, 20))
            } else {
                "AI 总结".to_string()
            }
        }
        "ai.analyze" => {
            if let Some(pn) = page_name {
                format!("分析{}内容", pn)
            } else if !target.is_empty() {
                format!("分析: {}", truncate_str(&target, 20))
            } else {
                "AI 分析".to_string()
            }
        }
        "ai.chat" => "AI 对话".to_string(),
        "ai.image" => {
            let desc = params
                .get("description")
                .and_then(|v| v.as_str())
                .or(params.get("prompt").and_then(|v| v.as_str()));
            if let Some(d) = desc {
                format!("生成图片: {}", truncate_str(d, 20))
            } else if !target.is_empty() {
                format!("生成图片: {}", truncate_str(&target, 20))
            } else {
                "生成图片".to_string()
            }
        }
        "prompt.generate" => {
            let desc = params.get("description").and_then(|v| v.as_str());
            if let Some(d) = desc {
                format!("生成提示词: {}", truncate_str(d, 20))
            } else if !target.is_empty() {
                format!("生成提示词: {}", truncate_str(&target, 20))
            } else {
                "生成提示词".to_string()
            }
        }
        "compare.content" => {
            if !target.is_empty() {
                format!("对比: {}", truncate_str(&target, 20))
            } else {
                "内容对比".to_string()
            }
        }
        "speech.tts" => {
            if !target.is_empty() {
                format!("朗读: {}", truncate_str(&target, 20))
            } else {
                "文字转语音".to_string()
            }
        }

        // 平台相关
        "platform.read" | "platform.bilibili" | "platform.bangumi" | "platform.steam"
        | "platform.github" | "platform.netease" | "platform.mal" => {
            let platform_name = match step.capability_id.as_str() {
                "platform.bilibili" => "B站",
                "platform.bangumi" => "Bangumi",
                "platform.mal" => "MyAnimeList",
                "platform.steam" => "Steam",
                "platform.github" => "GitHub",
                "platform.netease" => "网易云",
                _ => platform.unwrap_or("平台"),
            };
            if let Some(pn) = page_name {
                format!("获取{}的{}数据", pn, platform_name)
            } else {
                format!("获取{}数据", platform_name)
            }
        }

        // Tapp 相关
        "tapp.list" => "获取 Tapp 列表".to_string(),
        "tapp.page" => {
            if !target.is_empty() {
                format!("打开 Tapp: {}", target)
            } else {
                "打开 Tapp".to_string()
            }
        }

        // 搜索
        "search.global" => {
            if !target.is_empty() {
                format!("全局搜索: {}", truncate_str(&target, 20))
            } else {
                "全局搜索".to_string()
            }
        }

        // 报告
        "report.generate" => "生成报告".to_string(),

        // 缓存
        "cache.clear" => {
            if let Some(p) = platform {
                format!("清除 {} 缓存", p)
            } else {
                "清除缓存".to_string()
            }
        }

        // 默认
        _ => get_capability_friendly_name(&step.capability_id),
    }
}

/// 截断字符串
pub fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len).collect();
        format!("{}...", truncated)
    }
}

/// 敏感能力配置
/// 返回需要二次确认的能力及其配置
pub fn get_sensitive_capabilities() -> HashMap<&'static str, (&'static str, RiskLevel)> {
    let mut map = HashMap::new();

    // 高风险 - 不可逆操作
    map.insert(
        "cache.clear",
        (
            "此操作将清除指定平台的所有缓存数据，需要重新获取",
            RiskLevel::High,
        ),
    );
    map.insert(
        "brew.unsubscribe",
        ("此操作将取消订阅并删除相关数据", RiskLevel::High),
    );
    map.insert(
        "tapp.delete",
        ("此操作将删除 Tapp 及其所有数据", RiskLevel::High),
    );
    map.insert(
        "storage.delete",
        ("此操作将删除存储的数据，不可恢复", RiskLevel::High),
    );

    // 中风险 - 可能影响数据
    map.insert(
        "platform.write",
        ("此操作将修改平台数据", RiskLevel::Medium),
    );
    map.insert(
        "tapp.install",
        ("此操作将安装第三方 Tapp 组件", RiskLevel::Medium),
    );
    map.insert(
        "scheduler.create",
        ("此操作将创建定时任务，可能影响系统资源", RiskLevel::Medium),
    );
    map.insert(
        "scheduler.trigger",
        ("此操作将立即触发调度任务", RiskLevel::Medium),
    );
    map.insert("config.set", ("此操作将修改系统配置", RiskLevel::Medium));
    map.insert(
        "platform.refresh",
        ("此操作将刷新平台数据，可能消耗 API 配额", RiskLevel::Medium),
    );

    // 低风险 - 可逆操作
    map.insert("brew.mark", ("此操作将批量标记文章状态", RiskLevel::Low));
    map.insert("storage.set", ("此操作将存储数据到本地", RiskLevel::Low));
    map.insert("export.data", ("此操作将导出您的数据", RiskLevel::Low));

    map
}

/// 获取能力的使用提示（帮助 AI 更好地选择能力）
/// 这是 AI 选择正确能力的关键参考，必须全面覆盖所有能力
pub fn get_capability_usage_hint(capability_id: &str) -> &'static str {
    match capability_id {
        // ============ Brew 订阅系统 ============
        "brew.items" => "获取文章。用户说'看看订阅'、'最新文章'、'打开文章'、'总结文章'、'看看 X'时用这个。优先 sourceId；也可用 sourceName 宽松匹配。本地有文章时不要 webSearch",
        "brew.article" => {
            "【内部能力】根据已知ID获取文章详情。不要直接选择，由系统在已知文章ID时自动调用"
        }
        "brew.sources" => "【推荐·本地优先】从数据库列出/查找订阅源。用户说'友情链接'、'友链'时用 category=友情链接（或 friends）；说'我订阅了哪些'、'订阅源列表'、'看看 X'、'有没有 X 这个源'时使用。支持 sourceType=link|rss|brewlia。匹配后把 sourceId 传给 brew.items。本地有该源时不要改用 ai.webSearch",
        "brew.discover" => "发现/搜索 RSS 源。用户想找某个网站的 RSS 时使用",
        "brew.subscribe" => "添加新订阅。用户说'订阅xxx'、'添加订阅'时使用",
        "brew.stats" => "阅读统计。用户问'读了多少文章'、'订阅统计'时使用",
        "brew.read" => "通用订阅数据获取。读取 Brew RSS 订阅内容",
        "brew.page" => "Brew 页面内容。level=sources 返回含 sourceType/category 的订阅源列表；问友情链接可用 category=友情链接",
        "brew.schedule" => "Brew 调度控制。控制订阅调度器的启动/停止/刷新",
        "brew.generateReadingList" => "【推荐】生成阅读列表。用户说'给我推荐几篇文章'、'找些关于xx的文章'、'生成阅读列表'、'有什么值得看的'时使用",

        // ============ AI 智能处理 ============
        "ai.summarize" => "【必选】总结内容。用户说'总结'、'概括'、'讲讲大意'时必须使用",
        "ai.analyze" => "【必选】深度分析。用户说'分析'、'研究'、'评估'时必须使用",
        "ai.webSearch" => "【必选】联网搜索。查询外部实时信息时使用（新闻、公司、产品、天气等）",
        "ai.recommend" => "智能推荐。用户说'推荐'、'有什么好的'、'建议'时使用",
        "ai.chat" => "普通对话。用户闲聊或问通用问题时使用",
        "ai.image" => "AI 图片生成。用户说'生成图片'、'画一张'时使用",

        // ============ 平台数据 ============
        "platform.read" => "通用平台数据读取。获取平台缓存数据",
        "platform.stats" => "平台统计数据。获取平台数据的统计信息",
        "platform.write" => "平台数据写入。向平台缓存写入数据",
        "platform.refresh" => "刷新平台数据。触发平台数据重新获取",
        "bilibili.user" => "B站用户查询。获取 B 站用户信息、收藏、追番",
        "bangumi.user" => "Bangumi 用户查询。获取 Bangumi 用户基本信息",
        "bangumi.collections" => "Bangumi 收藏查询。获取 Bangumi 收藏、评分和观看状态",
        "steam.user" => "Steam 用户查询。获取 Steam 用户信息和游戏库",
        "netease.playlist" => "网易云歌单。获取用户网易云歌单和听歌记录",
        "github.repos" => "GitHub 仓库查询。查询 GitHub 仓库、贡献和活动",

        // ============ 音乐播放器 ============
        "music.control" => "【播放器控制】直接控制音乐播放器的当前状态。仅用于纯播放器操作：播放/暂停/下一首/上一首/静音/调音量。注意：用户说'放点音乐'、'找点音乐听'、'播放ACG音乐'等要求搜索音乐内容的，不要用这个，应该用 netease.searchPlaylist + music.playlist",
        "music.status" => "播放状态查询。用户问'现在放的什么歌'、'当前播放'时使用",
        "music.playlist" => "根据歌单ID加载并播放指定歌单。需要先通过 netease.searchPlaylist 获取歌单ID，然后用本能力加载。不要单独使用",
        "netease.searchPlaylist" => "搜索网易云歌单。用户说'放点音乐'、'找点音乐听'、'播放ACG音乐'、'推荐个歌单'时，先用这个搜索，然后配合 music.playlist 播放",

        // ============ Tapp 应用系统 ============
        "tapp.list" => "Tapp 应用列表。用户说'有哪些应用'、'应用列表'时使用",
        "tapp.page" => "Tapp 页面内容。获取应用列表/详情/组件/存储数据等",
        "tapp.generate" => "Tapp 生成。根据描述生成 Tapp 应用代码",
        "tapp.install" => "安装 Tapp。安装新的 Tapp 应用",
        "tapp.ui" => "Tapp UI 结构。解析 Tapp 应用的 HTML 结构和可交互元素",
        "tapp.understand" => "Tapp UI 智能理解。AI 分析 Tapp UI 并生成操作指令",
        "tapp.interact" => "Tapp 声明式交互。按 Manifest 声明的类型和 schema 请求 Tapp 处理数据",
        "tapp.windows" => "窗口状态查询。查询当前打开的 Tapp 窗口状态",
        "tapp.window.open" => "打开窗口。在多窗口模式下打开新的 Tapp 窗口",
        "tapp.window.close" => "关闭窗口。关闭指定的 Tapp 窗口",
        "tapp.window.focus" => "聚焦窗口。将指定窗口置为活跃状态",

        // ============ 报告系统 ============
        "report.create" => "生成报告。用户说'生成报告'、'做个总结报告'时使用",
        "report.list" => "报告列表。用户说'历史报告'时使用",
        "report.comprehensive" => "综合报告生成。生成跨平台综合分析报告",

        // ============ 路由导航 ============
        "router.state" => "路由状态。获取当前页面路由状态，了解用户在哪个页面",
        "router.navigate" => "【导航】路由导航。用户说'打开'、'跳转'、'去xx页面'时使用",

        // ============ 页面交互 ============
        "page.interact" => "页面元素交互。点击按钮、链接、标签页、菜单项等",
        "page.understand" => "页面 UI 智能理解。AI 分析当前页面 UI 并生成操作指令",
        "page.content" => "页面内容。读取当前页面显示的实际内容",

        // ============ 搜索 ============
        "search.global" => "全局搜索。跨平台搜索内容",
        "search.fuzzy" => "模糊搜索。Brew 源匹配名称/category/site_url；查询'友情链接'可命中友链分类源",

        // ============ 系统操作 ============
        "system.metrics" => "系统监控。获取系统运行状态和指标",
        "cache.status" => "缓存状态。获取各平台缓存状态",
        "cache.clear" => "清除缓存。清除指定平台的缓存数据",
        "config.get" => "获取配置。获取系统配置信息",
        "setup.status" => "系统设置状态。检查系统初始化和设置状态",
        "auth.status" => "认证状态。检查用户认证和权限状态",
        "export.data" => "数据导出。导出平台数据为指定格式",
        "image.cache" => "图片缓存。缓存外部图片到本地",
        "proxy.image" => "图片代理。代理获取外链图片",

        // ============ 定时任务 ============
        "scheduler.create" => "创建定时任务。创建定时执行的监控任务",
        "scheduler.list" => "定时任务列表。获取所有定时任务列表",
        "scheduler.trigger" => "立即执行任务。立即触发执行指定的定时任务",

        // ============ 后台任务 ============
        "task.submit" => "提交后台任务。提交平台数据处理任务",
        "task.status" => "任务状态查询。查询后台任务状态和进度",

        // ============ 数据处理 ============
        "data.transform" => "数据转换。对数据进行过滤、排序、聚合等操作",
        "smart.filter" => "智能内容过滤。对原始数据进行智能分类和过滤",
        "compare.content" => "内容比较。比较不同时间点的平台数据变化",

        // ============ 数据库查询 ============
        "database.anime" => "番剧数据库查询。查询预置番剧/电视剧/电影数据库",
        "database.game" => "游戏数据库查询。查询预置游戏数据库",
        "database.artist" => "艺术家数据库查询。查询预置歌手/艺术家数据库",
        "metadata.history" => "元数据历史。查询平台元数据变化历史",

        // ============ 用户画像 ============
        "profile.summary" => "用户画像。获取用户跨平台综合画像",

        // ============ 外部集成 ============
        "http.fetch" => "HTTP 请求。发起外部 HTTP 请求",
        "notion.query" => "Notion 数据查询。查询 Notion 数据库内容",
        "rsshub.instances" => "RSSHub 实例列表。获取 RSSHub 实例列表及状态",
        "rsshub.healthcheck" => "RSSHub 健康检查。对 RSSHub 实例进行健康检查",
        "hitokoto.get" => "获取一言。获取随机一言/语录",
        "weather.get" => "获取天气。获取天气信息",
        "time.info" => "时间信息。获取当前时间和日期信息",

        // ============ AI 增强阅读 ============
        "brewlia.annotate" => "AI 文章注释。为文章生成 AI 智能注释和解读",
        "brewlia.podcast" => "AI 播客生成。将文章转换为对话式播客文稿",

        // ============ 语音服务 ============
        "speech.tts" => "文字转语音。将文字内容转换为语音",

        // ============ 存储 ============
        "storage.set" => "存储数据。保存数据到 Tapp 存储",

        // ============ 其他 ============
        "icon.recommend" => "图标推荐。根据平台名称推荐合适的图标",
        "prompt.generate" => "提示词生成。为图片生成提供优化的提示词。必须在 description 参数中传入角色/场景的详细描述（角色全名、来源作品、外貌特征含发型发色瞳色服装等、场景、画风）。你应该利用自己的知识补充角色细节",
        "random.content" => "随机内容。获取随机推荐内容",
        "content.write" => "内容写入。写入内容数据",
        "context.reference" => "上下文引用。处理对话中的上下文引用",

        _ => "",
    }
}

/// 快速参考表：常见意图到能力的映射
pub fn get_quick_reference() -> Value {
    json!({
        "意图->能力快速映射": {
            "总结/概括/讲讲": ["ai.summarize"],
            "分析/研究/评估": ["ai.analyze"],
            "打开/跳转/前往/进入": ["router.navigate", "brew.items"],
            "搜索外部信息/新闻/公司/产品": ["ai.webSearch"],
            "看订阅/文章/最新文章": ["brew.items"],
            "生成阅读列表/推荐文章/找文章看": ["brew.generateReadingList"],
            "订阅/添加RSS": ["brew.discover", "brew.subscribe"],
            "B站/bilibili": ["platform.read", "bilibili.user"],
            "Bangumi/番组计划/动画收藏": ["platform.read", "bangumi.user", "bangumi.collections"],
            "MyAnimeList/MAL/动画列表/漫画列表": ["platform.read"],
            "Steam/游戏": ["platform.read", "steam.user"],
            "GitHub/代码/仓库": ["platform.read", "github.repos"],
            "网易云/音乐数据": ["platform.read", "netease.playlist"],
            "播放/暂停/下一首/上一首/音量": ["music.control"],
            "放点音乐/找点音乐听/播放ACG音乐": ["netease.searchPlaylist", "music.playlist"],
            "当前播放什么/播放状态": ["music.status"],
            "Tapp/应用列表": ["tapp.list"],
            "打开应用/Tapp窗口": ["tapp.window.open"],
            "与Tapp交互/点击按钮": ["tapp.interact", "tapp.understand"],
            "生成报告": ["report.create", "report.comprehensive"],
            "推荐/建议": ["ai.recommend"],
            "对话/聊天": ["ai.chat"],
            "当前页面内容": ["page.content", "brew.page", "tapp.page"],
            "刷新数据": ["platform.refresh"],
            "清除缓存": ["cache.clear"],
            "导出数据": ["export.data"],
            "定时任务": ["scheduler.create", "scheduler.list"],
            "生成图片/画图": ["prompt.generate", "ai.image"],
            "翻译": ["translate.text"],
            "文字转语音/朗读": ["speech.tts"],
            "天气": ["weather.get"],
            "一言/语录": ["hitokoto.get"]
        },
        "多步工作流模板": {
            "总结文章": {
                "steps": ["brew.items → ai.summarize"],
                "note": "先获取文章内容，再传给 ai.summarize（contentFrom 引用前步）"
            },
            "搜索并分析": {
                "steps": ["ai.webSearch → ai.analyze"],
                "note": "先搜索获取资料，再用 ai.analyze 深度分析"
            },
            "播放指定音乐": {
                "steps": ["netease.searchPlaylist → music.playlist"],
                "note": "先搜索歌单获取ID，再加载播放。两步必须有 depends_on"
            },
            "生成AI图片": {
                "steps": ["prompt.generate → ai.image"],
                "note": "先用 prompt.generate 生成优化 prompt（description 必须详细），再传给 ai.image"
            },
            "多平台对比": {
                "steps": ["platform.read(A) + platform.read(B) → ai.analyze"],
                "note": "并行获取各平台数据，然后汇总分析。analysisType=custom"
            },
            "发现并订阅RSS": {
                "steps": ["brew.discover → brew.subscribe"],
                "note": "先搜索 RSS 源，再用返回的 URL 订阅"
            },
            "翻译后朗读": {
                "steps": ["translate.text → speech.tts"],
                "note": "先翻译文本，再将翻译结果转语音"
            }
        },
        "特殊规则": [
            "用户说'打开最新的xx' -> action=navigate + brew.items",
            "用户说'总结文章' -> brew.items获取文章 + ai.summarize",
            "用户说'最近有什么xx新闻' -> ai.webSearch (外部信息)",
            "用户在brew页面说'总结' -> ai.summarize (使用pageContext)",
            "涉及'当前页面'/'这个'时 -> target.type=current_page",
            "用户说'播放/暂停/下一首/上一首' -> music.control",
            "用户说'放点音乐/找点音乐听/播放ACG音乐' -> netease.searchPlaylist + music.playlist (两步)",
            "用户说'播放歌单ID xxx' -> music.playlist",
            "用户说'给我推荐/找几篇文章/生成阅读列表' -> brew.generateReadingList",
            "用户说'注释文章/解读文章' -> brewlia.annotate (需要文章ID)",
            "用户说'做成播客/对话形式' -> brewlia.podcast (需要文章ID)",
            "brew.article 是内部能力，不要主动使用，由 brew.items 链式调用"
        ],
        "常见参数示例": {
            "platform.read": {"platform": "bilibili|bangumi|mal|steam|github|netease|x|discord|xbox|psn", "type": "overview|favorites|recent"},
            "ai.summarize": {"content": "文章内容或 contentFrom 引用", "maxLength": 300},
            "ai.analyze": {"content": "待分析文本", "analysisType": "sentiment|trends|custom", "customPrompt": "自定义分析角度"},
            "brew.items": {"limit": 10, "source_id": "可选源ID", "unread_only": true},
            "router.navigate": {"path": "/library, /brew, /reports, /config, /data-management, /tapp"},
            "music.control": {"action": "play|pause|next|prev|mute|unmute|volume", "volume": 50},
            "scheduler.create": {"name": "任务名", "cron": "*/30 * * * *", "capability_id": "定时执行的能力"}
        }
    })
}
