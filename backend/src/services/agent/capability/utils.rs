//! 能力系统工具函数
//!
//! 包含能力名称映射、步骤描述、风险评估等辅助函数

use super::super::types::{Capability, RecipeStep};
use serde_json::{Value, json};

/// 获取能力的友好名称
pub fn get_capability_friendly_name(capability_id: &str) -> String {
    match capability_id {
        "platform.read" => "Loading platform data".to_string(),
        "platform.bilibili" => "Loading Bilibili data".to_string(),
        "platform.steam" => "Loading Steam data".to_string(),
        "platform.github" => "Loading GitHub data".to_string(),
        "platform.netease" => "Loading NetEase data".to_string(),
        "platform.mal" => "Loading MyAnimeList data".to_string(),
        "ai.summarize" => "Summarizing".to_string(),
        "ai.analyze" => "Analyzing".to_string(),
        "ai.chat" => "Chatting".to_string(),
        "ai.webSearch" => "Searching the web".to_string(),
        "phantasi.discover" => "Discovering feeds".to_string(),
        "phantasi.subscribe" => "Subscribing to a feed".to_string(),
        "phantasi.read" => "Loading feed content".to_string(),
        "phantasi.items" => "Loading articles".to_string(),
        "phantasi.article" => "Loading article".to_string(),
        "phantasi.sources" => "Loading feeds".to_string(),
        "phantasi.stats" => "Reading stats".to_string(),
        "tapp.list" => "Listing apps".to_string(),
        "tapp.page" => "Opening an app".to_string(),
        "ai.image" => "Generating an image".to_string(),
        "prompt.generate" => "Generating a prompt".to_string(),
        "seo.inspect" => "Reading site SEO content".to_string(),
        "seo.generate" => "Drafting SEO copy".to_string(),
        "seo.apply" => "Saving SEO copy".to_string(),
        "compare.content" => "Comparing content".to_string(),
        "speech.tts" => "Reading aloud".to_string(),
        "search.global" => "Searching".to_string(),
        "report.generate" => "Generating a report".to_string(),
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
                    return format!("Running skill: {}", truncate_str(trimmed, 20));
                }
                return "Running skill".to_string();
            }
            // MCP 工具：提取工具名
            if let Some(rest) = capability_id.strip_prefix("mcp.") {
                // mcp.server_id.tool_name → 取最后一段
                if let Some(tool_name) = rest.rsplit('.').next() {
                    return format!("Calling tool: {}", tool_name);
                }
                return "Calling a tool".to_string();
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
/// 返回类似 `Search {target}` / `Subscribe {target}` 的描述
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
        "phantasi" => "feeds",
        "tapp" => "apps",
        "report" => "reports",
        "dashboard" => "dashboard",
        _ => "this page",
    });

    match step.capability_id.as_str() {
        // RSS/Phantasi 相关
        "phantasi.discover" => {
            if !target.is_empty() {
                format!("Search {}", target)
            } else if let Some(u) = url {
                format!("Discovering feeds: {}", truncate_str(u, 30))
            } else {
                "Discovering feeds".to_string()
            }
        }
        "phantasi.subscribe" => {
            if !target.is_empty() {
                format!("Subscribe {}", target)
            } else if let Some(u) = url {
                format!("Subscribe: {}", truncate_str(u, 30))
            } else {
                "Subscribing to a feed".to_string()
            }
        }
        "phantasi.read" => {
            if let Some(pn) = page_name {
                format!("Loading {pn} content")
            } else if !target.is_empty() {
                format!("Loading content from {target}")
            } else {
                "Loading feed content".to_string()
            }
        }
        "phantasi.items" => {
            if !target.is_empty() {
                format!("Loading articles from {target}")
            } else {
                "Loading articles".to_string()
            }
        }
        "phantasi.article" => {
            if !target.is_empty() {
                format!("Loading article: {}", truncate_str(&target, 30))
            } else {
                "Loading article".to_string()
            }
        }

        // AI 相关
        "ai.webSearch" => {
            if !target.is_empty() {
                format!("Search: {}", truncate_str(&target, 20))
            } else {
                "Searching the web".to_string()
            }
        }
        "ai.summarize" => {
            if let Some(pn) = page_name {
                format!("Summarizing {pn}")
            } else if !target.is_empty() {
                format!("Summarizing: {}", truncate_str(&target, 20))
            } else {
                "Summarizing".to_string()
            }
        }
        "ai.analyze" => {
            if let Some(pn) = page_name {
                format!("Analyzing {pn}")
            } else if !target.is_empty() {
                format!("Analyzing: {}", truncate_str(&target, 20))
            } else {
                "Analyzing".to_string()
            }
        }
        "ai.chat" => "Chatting".to_string(),
        "ai.image" => {
            let desc = params
                .get("description")
                .and_then(|v| v.as_str())
                .or(params.get("prompt").and_then(|v| v.as_str()));
            if let Some(d) = desc {
                format!("Generating an image: {}", truncate_str(d, 20))
            } else if !target.is_empty() {
                format!("Generating an image: {}", truncate_str(&target, 20))
            } else {
                "Generating an image".to_string()
            }
        }
        "prompt.generate" => {
            let desc = params.get("description").and_then(|v| v.as_str());
            if let Some(d) = desc {
                format!("Generating a prompt: {}", truncate_str(d, 20))
            } else if !target.is_empty() {
                format!("Generating a prompt: {}", truncate_str(&target, 20))
            } else {
                "Generating a prompt".to_string()
            }
        }
        "compare.content" => {
            if !target.is_empty() {
                format!("Comparing: {}", truncate_str(&target, 20))
            } else {
                "Comparing content".to_string()
            }
        }
        "speech.tts" => {
            if !target.is_empty() {
                format!("Reading aloud: {}", truncate_str(&target, 20))
            } else {
                "Reading aloud".to_string()
            }
        }

        // 平台相关
        "platform.read" | "platform.bilibili" | "platform.bangumi" | "platform.steam"
        | "platform.github" | "platform.netease" | "platform.mal" => {
            let platform_name = match step.capability_id.as_str() {
                "platform.bilibili" => "Bilibili",
                "platform.bangumi" => "Bangumi",
                "platform.mal" => "MyAnimeList",
                "platform.steam" => "Steam",
                "platform.github" => "GitHub",
                "platform.netease" => "NetEase",
                _ => platform.unwrap_or("platform"),
            };
            format!("Loading {platform_name} data")
        }

        // Tapp 相关
        "tapp.list" => "Listing apps".to_string(),
        "tapp.page" => {
            if !target.is_empty() {
                format!("Opening app: {target}")
            } else {
                "Opening an app".to_string()
            }
        }

        // 搜索
        "search.global" => {
            if !target.is_empty() {
                format!("Search: {}", truncate_str(&target, 20))
            } else {
                "Searching".to_string()
            }
        }

        // 报告
        "report.generate" => "Generating a report".to_string(),

        // 缓存
        "cache.clear" => {
            if let Some(p) = platform {
                format!("Clearing {p} cache")
            } else {
                "Clearing cache".to_string()
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

/// 能力在 Planner 索引中的说明文字。
///
/// 优先用 [`get_capability_usage_hint`] 里人工编写的提示（它带「什么时候该选这个」
/// 的触发语），缺失时回退到能力自己的 `description`。
///
/// 回退是必需的：hint 表的兜底分支返回空串，漏登记的能力会以 `"h": ""` 进入索引——
/// 模型只看得到一个能力 ID。
pub fn resolve_capability_hint(capability: &Capability) -> &str {
    let hint = get_capability_usage_hint(&capability.id);
    if hint.is_empty() {
        &capability.description
    } else {
        hint
    }
}

/// 获取能力的使用提示（帮助 AI 更好地选择能力）
///
/// 返回空串表示该能力没有人工编写的提示；调用方应走
/// [`resolve_capability_hint`] 以便回退到 `description`。
pub fn get_capability_usage_hint(capability_id: &str) -> &'static str {
    match capability_id {
        // Phantasi 订阅系统
        "phantasi.items" => {
            "Get articles. Use when the user says look at feeds / 看看订阅 / 最新文章 / 打开文章 / 总结文章 / 看看 X. Prefer sourceId; sourceName is a loose match. Do not webSearch when local articles exist."
        }
        "phantasi.article" => {
            "[internal] Load one article by a known id. Do not pick this directly; the system calls it when the article id is already known."
        }
        "phantasi.sources" => {
            "[preferred, local first] List or find feeds. For friend links / 友情链接 / 友链 use category=友情链接 (or friends). Use when the user asks which feeds they have, feed list, or whether a named feed exists. sourceType=link|rss|phantasiai. Pass sourceId to phantasi.items. Do not switch to ai.webSearch when the feed exists locally."
        }
        "phantasi.discover" => "Discover RSS feeds. Use when the user wants the RSS for a site.",
        "phantasi.subscribe" => "Add a feed. Use when the user says subscribe / 订阅 / 添加订阅.",
        "phantasi.stats" => {
            "Reading stats. Use when the user asks how many articles they have read or for feed stats."
        }
        "phantasi.read" => "Generic feed read. Load Phantasi RSS content.",
        "phantasi.page" => {
            "Phantasi page content. level=sources returns feeds with sourceType/category. Friend links: category=友情链接."
        }
        "phantasi.schedule" => "Phantasi scheduler. Start, stop, or refresh.",
        "phantasi.generateReadingList" => {
            "[preferred] Build a reading list. Use when the user asks for article recommendations, articles about X, a reading list, or what is worth reading."
        }

        // AI 智能处理
        "ai.summarize" => {
            "[required] Summarize. Use when the user says summarize / 总结 / 概括 / 讲讲大意 / 要約."
        }
        "ai.analyze" => {
            "[required] Analyze. Use when the user says analyze / 分析 / 研究 / 评估 / 分析して."
        }
        "ai.webSearch" => {
            "[required] Web search. Use for live external facts (news, companies, products, weather)."
        }
        "ai.recommend" => {
            "Recommend. Use when the user says recommend / 推荐 / 有什么好的 / おすすめ."
        }
        "ai.chat" => "Chat. Use for small talk or general questions.",
        "ai.image" => {
            "Generate an image. Use when the user says generate an image / 生成图片 / 画一张. Optional width/height (256–2048, default 1024); pass them for portrait, landscape, or wallpaper."
        }

        // 平台数据
        "platform.read" => "Read cached platform data.",
        "platform.stats" => "Read platform statistics.",
        "platform.write" => "Write to platform cache.",
        "platform.refresh" => "Refresh platform data.",
        "bilibili.user" => "Bilibili user. Profile, favorites, following.",
        "bangumi.user" => "Bangumi user profile.",
        "bangumi.collections" => "Bangumi collections, scores, and watch status.",
        "steam.user" => "Steam profile and library from cache.",
        "netease.playlist" => "NetEase playlists and listening history.",
        "github.repos" => "GitHub repositories, contributions, and activity.",

        // 音乐播放器
        "music.control" => {
            "[player control] Play/pause/next/previous/mute/volume only. If the user asks to find or play some music (放点音乐 / 找点音乐听 / 播放ACG音乐), use netease.searchPlaylist then music.playlist instead."
        }
        "music.status" => "Player status from the browser. The backend has no player.",
        "music.playlist" => {
            "Load and play a playlist by id. Get the id from netease.searchPlaylist first. Do not use alone."
        }
        "netease.searchPlaylist" => {
            "Search NetEase playlists. Use when the user says play some music / 放点音乐 / 找点音乐听 / 播放ACG音乐 / 推荐个歌单, then play with music.playlist."
        }

        // Tapp 应用系统
        "tapp.list" => "Installed apps. Use when the user asks which apps they have.",
        "tapp.page" => "App page content: list, detail, widgets, storage.",
        "tapp.generate" => "Generate app code from a description.",
        "tapp.install" => "Install an app.",
        "tapp.ui" => "Parse app HTML and interactive elements.",
        "tapp.understand" => "Analyze app UI and produce actions.",
        "tapp.interact" => "Declared app interaction. Send Manifest type and schema to the app.",
        "tapp.windows" => "List open app windows.",
        "tapp.window.open" => "Open an app window in multi-window mode.",
        "tapp.window.close" => "Close an app window.",
        "tapp.window.focus" => "Focus an app window.",

        // 报告系统
        "report.create" => {
            "Create a report. Use when the user says generate a report / 生成报告 / 做个总结报告."
        }
        "report.list" => {
            "List reports. Platform reports from platform_reports; without platform, include this user's Agent report.create records."
        }

        // 路由导航
        "router.state" => "Current route. What page the user is on.",
        "router.navigate" => {
            "[navigate] Go to a page. Use when the user says open / go to / 打开 / 跳转 / 去xx页面."
        }

        // 页面交互
        "page.interact" => "Click buttons, links, tabs, menus.",
        "page.understand" => "Analyze the current page UI and produce actions.",
        "page.content" => {
            "Page content. Use a frontend snapshot if present, otherwise phantasi.page / tapp.page / platform.read."
        }

        // 搜索
        "search.global" => "Search across platforms.",
        "search.fuzzy" => {
            "Fuzzy search. Phantasi sources match name/category/site_url; 友情链接 hits the friend-link category."
        }

        // 系统操作
        "system.metrics" => "This process: memory, uptime, task counts (not full host monitoring).",
        "cache.status" => "Cache status per platform.",
        "cache.clear" => "Clear cached data for a platform.",
        "config.get" => {
            "Read config. AI is Standard (enabled/provider/model, no secrets); platforms are connection flags; ui is public display fields."
        }
        "seo.inspect" => {
            "Agent SEO/GEO step 1. Read branding + guest-visible original writing/notes/apps. Ground truth. Never friend-links or third-party feeds. Always before seo.generate."
        }
        "seo.generate" => {
            "Agent SEO/GEO step 2. Draft description/keywords/ai_intro from inspect facts. Does not save. Show the draft. seo.apply only after the owner confirms. Omit title to use branding. Scheduled review may generate drafts but must not apply."
        }
        "seo.apply" => {
            "Agent SEO/GEO step 3. Save description/keywords/ai_intro after the owner confirms. Never visibility policy. Scheduled Agent SEO optimization must never call this."
        }
        "setup.status" => {
            "Setup status: tables and owner (same as HTTP /api/setup/status, no AI keys)."
        }
        "auth.status" => "Sign-in and permission status.",
        "permission.check" => {
            "Granted permissions for this session role. With tappId, intersect with that install's approved permissions; granted is false if re-auth is needed."
        }
        "export.data" => "Export platform data.",
        "image.cache" => {
            "With url, download into image-cache. Without url, action=status or action=clear."
        }
        "proxy.image" => "Fetch an external image through the image proxy.",

        // Tapp 定时任务（tapp_scheduled_tasks）
        "scheduler.create" => {
            "Create a scheduled app task (needs tappId). For installed apps only, not agent heartbeat."
        }
        "scheduler.list" => "List this user's app scheduled tasks (not HEARTBEAT.md).",
        "scheduler.trigger" => "Run an app scheduled task now.",

        // Agent Heartbeat（HEARTBEAT.md，自然语言指令）
        "heartbeat.list" => {
            "[preferred] List agent heartbeat tasks. Use when the user asks which scheduled/heartbeat tasks they have. Not platform auto-refresh, not Tapp scheduler."
        }
        "heartbeat.create" => {
            "[preferred] Create an agent heartbeat. Use when the user says schedule / every day / every hour / heartbeat / 定时 / 每天 / 每隔 / 心跳. params: name, schedule (5-field cron), action (natural language), enabled default true. Example: schedule=\"0 * * * *\" action=\"check whether akiday updated\". Do not use scheduler.create."
        }
        "heartbeat.update" => "Update a heartbeat by id (name/schedule/action/enabled).",
        "heartbeat.delete" => "Delete a HEARTBEAT.md task by id.",
        "heartbeat.toggle" => "Enable or disable a heartbeat by id.",

        // 后台任务
        "task.submit" => "Submit a background platform-data task.",
        "task.status" => "Agent task status by taskId, or recent tasks.",

        // 数据处理
        "data.transform" => "Filter, sort, or aggregate data.",
        "smart.filter" => "Classify and filter raw data.",
        "compare.content" => "Compare platform data across time.",

        // 数据库查询
        "database.anime" => "Query the built-in anime/TV/film database.",
        "database.game" => "Query the built-in game database.",
        "database.artist" => "Query the built-in artist database.",
        "metadata.history" => "Platform metadata history.",

        // 用户画像
        "profile.summary" => "Cross-platform profile summary.",

        // 外部集成
        "http.fetch" => "Outbound HTTP request.",
        "notion.query" => "Query a Notion database.",
        "rsshub.instances" => "Configured RSSHub instances and health.",
        "rsshub.healthcheck" => "Probe configured RSSHub instances (optional instanceId).",
        "hitokoto.get" => "Random quote.",
        "weather.get" => "Weather.",
        "time.info" => "Convert wall-clock time (IANA/UTC/local/+08:00). Unknown zones fail.",

        // AI 增强阅读
        "phantasiai.annotate" => "AI notes and reading help for an article.",
        "phantasiai.podcast" => "Turn an article into a spoken-dialogue script.",

        // 语音服务
        "speech.tts" => "Text to speech. Same path as /api/speech/tts; the frontend plays it.",

        // 存储
        "storage.set" => "Save data to app storage.",

        // 其他
        "icon.recommend" => "Recommend an icon for a platform name.",
        "prompt.generate" => {
            "Write a better image prompt. description must include character/scene detail (full name, work, appearance including hair/eyes/clothes, scene, style). Fill in character details from your knowledge."
        }
        "random.content" => "Random recommendation from platform data.",
        "content.write" => "Write content data.",
        "context.reference" => {
            "Reuse earlier step output. params.stepId is the step id, path is a field path (e.g. results[0].title), transform is none/stringify/parse/join/first/last."
        }

        _ => "",
    }
}

/// 快速参考表：常见意图到能力的映射
pub fn get_quick_reference() -> Value {
    json!({
        "intent_to_capability": {
            "summarize / 总结 / 概括 / 讲讲": ["ai.summarize"],
            "analyze / 分析 / 研究 / 评估": ["ai.analyze"],
            "open / go to / 打开 / 跳转 / 前往 / 进入": ["router.navigate", "phantasi.items"],
            "search news/company/product / 搜索外部信息": ["ai.webSearch"],
            "feeds / articles / 看订阅 / 最新文章": ["phantasi.items"],
            "reading list / 生成阅读列表 / 推荐文章": ["phantasi.generateReadingList"],
            "subscribe / 订阅 / 添加RSS": ["phantasi.discover", "phantasi.subscribe"],
            "Bilibili / B站": ["platform.read", "bilibili.user"],
            "Bangumi / 番组计划 / 动画收藏": ["platform.read", "bangumi.user", "bangumi.collections"],
            "MyAnimeList / MAL": ["platform.read"],
            "Steam / games / 游戏": ["platform.read", "steam.user"],
            "GitHub / repos / 仓库": ["platform.read", "github.repos"],
            "NetEase / 网易云": ["platform.read", "netease.playlist"],
            "play/pause/next/previous/volume / 播放/暂停/下一首/上一首/音量": ["music.control"],
            "play some music / 放点音乐 / 找点音乐听 / 播放ACG音乐": ["netease.searchPlaylist", "music.playlist"],
            "now playing / 当前播放 / 播放状态": ["music.status"],
            "apps / Tapp / 应用列表": ["tapp.list"],
            "open app window / 打开应用": ["tapp.window.open"],
            "app UI / 与Tapp交互 / 点击按钮": ["tapp.interact", "tapp.understand"],
            "report / 生成报告": ["report.create"],
            "recommend / 推荐 / 建议": ["ai.recommend"],
            "chat / 对话 / 聊天": ["ai.chat"],
            "current page / 当前页面内容": ["page.content", "phantasi.page", "tapp.page"],
            "refresh / 刷新数据": ["platform.refresh"],
            "clear cache / 清除缓存": ["cache.clear"],
            "export / 导出数据": ["export.data"],
            "schedule / heartbeat / 定时任务 / 每天 / 每隔 / 心跳": ["heartbeat.create", "heartbeat.list", "heartbeat.update", "heartbeat.delete"],
            "app scheduled task / Tapp定时任务": ["scheduler.create", "scheduler.list"],
            "generate image / 生成图片 / 画图": ["prompt.generate", "ai.image"],
            "translate / 翻译": ["translate.text"],
            "tts / 文字转语音 / 朗读": ["speech.tts"],
            "weather / 天气": ["weather.get"],
            "quote / 一言 / 语录": ["hitokoto.get"],
            "SEO / GEO / llms.txt / 站点简介 / 搜索描述 / meta description": ["seo.inspect", "seo.generate", "seo.apply"]
        },
        "workflow_templates": {
            "summarize_article": {
                "steps": ["phantasi.items → ai.summarize"],
                "note": "Load the article first, then ai.summarize with contentFrom."
            },
            "search_then_analyze": {
                "steps": ["ai.webSearch → ai.analyze"],
                "note": "Search first, then ai.analyze."
            },
            "play_music": {
                "steps": ["netease.searchPlaylist → music.playlist"],
                "note": "Search for a playlist id, then load it. The second step must depend_on the first."
            },
            "generate_image": {
                "steps": ["prompt.generate → ai.image"],
                "note": "Write a detailed prompt first, then ai.image. Size is ai.image width/height (256–2048, default 1024); put portrait/landscape/user size in params, not in the prompt."
            },
            "compare_platforms": {
                "steps": ["platform.read(A) + platform.read(B) → ai.analyze"],
                "note": "Read platforms in parallel, then analyze. analysisType=custom."
            },
            "discover_and_subscribe": {
                "steps": ["phantasi.discover → phantasi.subscribe"],
                "note": "Find the RSS URL, then subscribe with that URL."
            },
            "translate_then_speak": {
                "steps": ["translate.text → speech.tts"],
                "note": "Translate first, then speak the translation."
            }
        },
        "special_rules": [
            "open latest X -> action=navigate + phantasi.items",
            "summarize article / 总结文章 -> phantasi.items then ai.summarize",
            "recent news about X -> ai.webSearch (external)",
            "summarize on a phantasi page -> ai.summarize with pageContext",
            "this page / 当前页面 / 这个 -> target.type=current_page",
            "play/pause/next/previous -> music.control",
            "play some music / 放点音乐 -> netease.searchPlaylist + music.playlist (two steps)",
            "play playlist id X -> music.playlist",
            "recommend articles / reading list -> phantasi.generateReadingList",
            "annotate article / 注释文章 -> phantasiai.annotate (needs article id)",
            "make a podcast / 做成播客 -> phantasiai.podcast (needs article id)",
            "phantasi.article is internal; phantasi.items chains to it",
            "every day/hour / 定时检查/总结 -> heartbeat.create with a natural-language action; not scheduler.create",
            "which scheduled/heartbeat tasks -> heartbeat.list",
            "platform auto-refresh vs app scheduler vs heartbeat are different: platform.refresh / scheduler.* / heartbeat.*",
            "optimize SEO / GEO / Agent 优化 SEO / 站点简介 -> step1 seo.inspect (ground truth) -> step2 seo.generate (draft only) -> show current vs rewrite -> seo.apply only after the owner confirms; never change visibility; scheduled review: inspect, generate if bad, notify, never apply"
        ],
        "param_examples": {
            "platform.read": {"platform": "bilibili|bangumi|mal|steam|github|netease|x|discord|xbox|psn", "type": "overview|favorites|recent"},
            "ai.summarize": {"content": "article text or contentFrom", "maxLength": 300},
            "ai.analyze": {"content": "text to analyze", "analysisType": "sentiment|trends|custom", "customPrompt": "custom angle"},
            "phantasi.items": {"limit": 10, "source_id": "optional source id", "unread_only": true},
            "router.navigate": {"path": "/, /library, /journal, /reports, /config, /tapp"},
            "music.control": {"action": "play|pause|toggle|next|previous|mute|unmute|volume", "volume": 50},
            "scheduler.create": {"tappId": "installed app id", "name": "task name", "scheduleType": "cron", "schedule": {"cron": "*/30 * * * *"}},
            "heartbeat.create": {"name": "Phantasi morning summary", "schedule": "0 9 * * *", "action": "summarize phantasi feeds", "enabled": true},
            "seo.generate": {"hint": "indie developer writing Rust", "fields": ["site_ai_intro"]},
            "seo.apply": {"site_description": "…", "site_ai_intro": "…"}
        }
    })
}

#[cfg(test)]
mod sensitive_caps_tests {
    use crate::services::agent::capability::{
        CapabilityRegistry, capability_requires_confirmation,
    };
    use crate::services::agent::types::RiskLevel;

    #[test]
    fn confirmation_authority_is_the_registry() {
        let registry = CapabilityRegistry::new();
        for id in [
            "phantasi.subscribe",
            "phantasi.schedule",
            "http.fetch",
            "task.submit",
            "scheduler.create",
            "scheduler.trigger",
            "cache.clear",
            "platform.write",
            "platform.refresh",
            "heartbeat.delete",
        ] {
            let cap = registry.get(id).unwrap_or_else(|| panic!("missing {id}"));
            let (msg, risk) = capability_requires_confirmation(cap)
                .unwrap_or_else(|| panic!("{id} must require confirmation"));
            assert!(!msg.is_empty(), "{id} message");
            assert!(
                matches!(
                    risk,
                    RiskLevel::Low | RiskLevel::Medium | RiskLevel::High | RiskLevel::Critical
                ),
                "{id} risk {risk:?}"
            );
        }
        assert!(registry.get("phantasi.unsubscribe").is_none());
        assert!(registry.get("config.set").is_none());
        assert!(registry.get("storage.delete").is_none());
        assert!(registry.get("tapp.delete").is_none());
        let submit = registry.get("task.submit").unwrap();
        assert_eq!(
            capability_requires_confirmation(submit).map(|(_, r)| r),
            Some(RiskLevel::Medium)
        );
    }
}
