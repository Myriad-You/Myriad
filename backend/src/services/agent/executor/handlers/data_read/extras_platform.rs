use super::super::HandlerContext;
use crate::services::data_paths::platform_filtered_file;
use crate::services::netease_utils::{get_random_china_ip, get_random_user_agent};
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::HashMap;

pub(super) async fn execute_netease_playlist(
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let query_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("playlists");

    let cache_file = platform_filtered_file("netease");
    if let Ok(content) = tokio::fs::read_to_string(cache_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let result = match query_type {
                "playlists" => data
                    .get("content_analysis")
                    .and_then(|v| v.get("playlists"))
                    .cloned()
                    .unwrap_or(json!([])),
                "recent" => data
                    .get("content_analysis")
                    .and_then(|v| v.get("recent_songs"))
                    .cloned()
                    .unwrap_or(json!([])),
                "favorites" => data
                    .get("content_analysis")
                    .and_then(|v| v.get("favorite_songs"))
                    .cloned()
                    .unwrap_or(json!([])),
                _ => json!([]),
            };

            let playlists = if query_type == "playlists" {
                result.clone()
            } else {
                json!([])
            };
            let songs = if query_type == "playlists" {
                json!([])
            } else {
                result.clone()
            };
            return Ok(json!({
                "type": query_type,
                "data": result,
                "playlists": playlists,
                "songs": songs
            }));
        }
    }

    Err("Failed to read Netease data".to_string())
}

/// 联网搜索网易云歌单
pub(super) async fn execute_netease_search_playlist(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    let keyword = params
        .get("keyword")
        .and_then(|v| v.as_str())
        .unwrap_or("轻音乐");
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

    tracing::info!(
        keyword = %keyword,
        limit = limit,
        "[data_read] Searching Netease playlists online"
    );

    // 映射关键词到网易云分类
    let mapped_category = map_keyword_to_netease_category(keyword);

    // 判断是否需要 AI 辅助理解
    let final_category = if mapped_category == keyword && keyword.chars().count() > 2 {
        // 映射没变化，说明是模糊描述，尝试调用 AI 理解
        if let Some(ai_analyzer) = ctx.ai_analyzer {
            match ai_understand_music_intent(ai_analyzer, keyword).await {
                Ok(ai_category) => {
                    tracing::info!(
                        keyword = %keyword,
                        ai_category = %ai_category,
                        "[data_read] AI understood music intent"
                    );
                    ai_category
                }
                Err(_) => "轻音乐".to_string(),
            }
        } else {
            "轻音乐".to_string()
        }
    } else {
        mapped_category
    };

    // 尝试多种分类
    let categories_to_try = vec![
        final_category.clone(),
        "轻音乐".to_string(),
        "流行".to_string(),
    ];

    let mut all_playlists: Vec<Value> = Vec::new();

    // 使用项目已有的 IP 伪装
    let client_ip = get_random_china_ip();
    let proxy_ip = get_random_china_ip();
    let forwarded_for = format!("{}, {}", client_ip, proxy_ip);
    let user_agent = get_random_user_agent();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| {
            tracing::error!(error = %e, "HTTP client build failed");
            "HTTP client error".to_string()
        })?;

    for category in &categories_to_try {
        let encoded_category = urlencoding::encode(category);
        let search_url = format!(
            "https://music.163.com/api/playlist/list?cat={}&order=hot&offset=0&total=true&limit={}",
            encoded_category,
            limit * 2
        );

        if let Ok(response) = client
            .get(&search_url)
            .header("Referer", "https://music.163.com/")
            .header("User-Agent", user_agent)
            .header("X-Forwarded-For", forwarded_for.clone())
            .header("X-Real-IP", client_ip.clone())
            .send()
            .await
        {
            let data =
                match crate::services::outbound_security::read_limited_body(response, 512 * 1024)
                    .await
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                {
                    Some(d) => d,
                    None => continue,
                };
            if let Some(playlists) = data.get("playlists").and_then(|p| p.as_array()) {
                if !playlists.is_empty() {
                    tracing::info!(
                        category = %category,
                        count = playlists.len(),
                        "[data_read] Found playlists with category"
                    );
                    all_playlists = playlists.clone();
                    break;
                }
            }
        }
    }

    // 按关键词相关性排序
    let keyword_lower = keyword.to_lowercase();
    let mut scored_playlists: Vec<(Value, i32)> = all_playlists
        .into_iter()
        .map(|p| {
            let score = score_playlist_relevance(&p, &keyword_lower);
            (p, score)
        })
        .collect();

    scored_playlists.sort_by_key(|b| Reverse(b.1));

    // 格式化输出
    let formatted_playlists: Vec<Value> = scored_playlists
        .into_iter()
        .take(limit)
        .map(|(p, _score)| {
            json!({
                "id": p.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                "name": p.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "trackCount": p.get("trackCount").and_then(|v| v.as_i64()).unwrap_or(0),
                "playCount": p.get("playCount").and_then(|v| v.as_i64()).unwrap_or(0),
                "coverUrl": p.get("coverImgUrl").and_then(|v| v.as_str()).unwrap_or(""),
                "creator": p.get("creator").and_then(|c| c.get("nickname")).and_then(|v| v.as_str()).unwrap_or(""),
                "description": p.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                "tags": p.get("tags").and_then(|t| t.as_array()).map(|arr|
                    arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>()
                ).unwrap_or_default()
            })
        })
        .collect();

    if formatted_playlists.is_empty() {
        return Ok(json!({
            "success": false,
            "keyword": keyword,
            "playlists": [],
            "message": crate::services::agent::response_agent::playlist_not_found()
        }));
    }

    let recommended_id = formatted_playlists
        .first()
        .and_then(|p| p.get("id"))
        .and_then(|id| id.as_i64())
        .map(|n| n.to_string());

    let recommended_name = formatted_playlists
        .first()
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    Ok(json!({
        "success": true,
        "keyword": keyword,
        "mappedCategory": final_category,
        "playlists": formatted_playlists,
        "recommendedPlaylistId": recommended_id,
        "recommendedPlaylistName": recommended_name,
        "source": "netease",
        "message": crate::services::agent::response_agent::playlist_found(formatted_playlists.len(), keyword)
    }))
}

/// 读取 GitHub 仓库数据
pub(super) async fn execute_github_repos(params: &HashMap<String, Value>) -> Result<Value, String> {
    let query_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("repos");

    let cache_file = platform_filtered_file("github");
    if let Ok(content) = tokio::fs::read_to_string(cache_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let result = match query_type {
                "repos" => data
                    .get("content_analysis")
                    .and_then(|v| v.get("recent_repos"))
                    .cloned()
                    .unwrap_or(json!([])),
                "contributions" => data
                    .get("content_analysis")
                    .and_then(|v| v.get("contribution_calendar"))
                    .cloned()
                    .unwrap_or(json!([])),
                "starred" => data
                    .get("content_analysis")
                    .and_then(|v| v.get("starred_repos"))
                    .cloned()
                    .unwrap_or(json!([])),
                _ => data.get("content_analysis").cloned().unwrap_or(json!({})),
            };

            return Ok(json!({
                "type": query_type,
                "data": result
            }));
        }
    }

    Err("Failed to read GitHub data".to_string())
}

// 辅助函数

/// 映射关键词到网易云分类
fn map_keyword_to_netease_category(keyword: &str) -> String {
    let keyword_lower = keyword.to_lowercase();

    // 关键词到网易云分类的映射
    let mappings: Vec<(&[&str], &str)> = vec![
        (
            &["工作", "办公", "专注", "学习", "编程", "coding"],
            "轻音乐",
        ),
        (&["睡眠", "睡前", "入睡", "助眠", "安静"], "轻音乐"),
        (&["放松", "舒缓", "休闲", "轻松"], "轻音乐"),
        (&["运动", "健身", "跑步", "动感", "激情"], "电子"),
        (&["古风", "中国风", "国风"], "古风"),
        (&["摇滚", "rock"], "摇滚"),
        (&["民谣", "folk"], "民谣"),
        (&["电子", "edm", "electronic", "dj"], "电子"),
        (&["说唱", "嘻哈", "rap", "hip-hop"], "说唱"),
        (&["流行", "pop", "热门"], "流行"),
        (&["古典", "classical", "钢琴", "交响"], "古典"),
        (&["爵士", "jazz"], "爵士"),
        (&["蓝调", "blues"], "蓝调"),
        (&["乡村", "country"], "乡村"),
        (&["acg", "动漫", "二次元", "日语"], "ACG"),
        (&["华语", "中文", "国语"], "华语"),
        (&["英文", "欧美", "英语"], "欧美"),
        (&["日语", "日本", "日系"], "日语"),
        (&["韩语", "韩国", "韩流", "kpop"], "韩语"),
    ];

    for (keywords, category) in mappings {
        for k in keywords {
            if keyword_lower.contains(k) {
                return category.to_string();
            }
        }
    }

    // 没匹配上就返回原关键词
    keyword.to_string()
}

/// 计算歌单与关键词的相关性分数
fn score_playlist_relevance(playlist: &Value, keyword: &str) -> i32 {
    let mut score = 0;

    // 名称匹配
    if let Some(name) = playlist.get("name").and_then(|n| n.as_str()) {
        let name_lower = name.to_lowercase();
        if name_lower.contains(keyword) {
            score += 100;
        }
        // 部分匹配
        for word in keyword.split_whitespace() {
            if name_lower.contains(word) {
                score += 30;
            }
        }
    }

    // 描述匹配
    if let Some(desc) = playlist.get("description").and_then(|d| d.as_str()) {
        let desc_lower = desc.to_lowercase();
        if desc_lower.contains(keyword) {
            score += 50;
        }
        // 检查相关词
        for term in get_related_terms(keyword) {
            if desc_lower.contains(term) {
                score += 15;
            }
        }
    }

    // 标签匹配
    if let Some(tags) = playlist.get("tags").and_then(|t| t.as_array()) {
        for tag in tags {
            if let Some(tag_str) = tag.as_str() {
                if tag_str.to_lowercase().contains(keyword) {
                    score += 80;
                }
                // 检查相关词
                for term in get_related_terms(keyword) {
                    if tag_str.to_lowercase().contains(term) {
                        score += 25;
                    }
                }
            }
        }
    }

    // 播放量加分（热门歌单优先）
    if let Some(play_count) = playlist.get("playCount").and_then(|p| p.as_i64()) {
        score += std::cmp::min(play_count / 1_000_000, 20) as i32;
    }

    score
}

/// 获取关键词的相关词/同义词
fn get_related_terms(keyword: &str) -> Vec<&'static str> {
    let term_groups: &[&[&str]] = &[
        // 放松相关
        &[
            "放松", "轻松", "舒缓", "休息", "休闲", "慵懒", "惬意", "chill",
        ],
        // 安静相关
        &["安静", "静心", "静谧", "宁静", "平静", "冥想", "禅"],
        // 学习/工作相关
        &[
            "学习", "阅读", "读书", "看书", "工作", "专注", "集中", "效率", "coding", "编程",
        ],
        // 睡眠相关
        &["睡眠", "助眠", "入睡", "晚安", "深夜", "夜晚", "催眠"],
        // 运动相关
        &["运动", "健身", "跑步", "锻炼", "燃脂", "有氧", "gym"],
        // 轻音乐相关
        &[
            "轻音乐",
            "纯音乐",
            "器乐",
            "钢琴",
            "吉他",
            "小提琴",
            "无人声",
        ],
        // 治愈相关
        &["治愈", "温暖", "温馨", "舒适", "暖心", "感动"],
        // 伤感相关
        &["伤感", "难过", "悲伤", "失恋", "分手", "孤独", "寂寞"],
        // 欢快相关
        &["欢快", "开心", "快乐", "愉悦", "活力", "元气", "阳光"],
        // ACG相关
        &["acg", "动漫", "二次元", "日漫", "番剧", "游戏", "anime"],
    ];

    for group in term_groups {
        if group
            .iter()
            .any(|t| keyword.contains(t) || t.contains(keyword))
        {
            return group.iter().filter(|&&t| t != keyword).copied().collect();
        }
    }

    Vec::new()
}

/// AI 理解音乐意图
async fn ai_understand_music_intent(
    ai_analyzer: &crate::services::analyzer::AiAnalyzer,
    user_input: &str,
) -> Result<String, String> {
    let prompt = format!(
        r#"用户想听的音乐描述是："{}"

请分析用户的音乐需求，然后返回一个最匹配的网易云音乐分类标签。
可选分类：流行、轻音乐、电子、摇滚、民谣、说唱、古风、古典、爵士、蓝调、ACG、华语、欧美、日语、韩语

只返回分类名称，不要任何解释。"#,
        user_input
    );

    match ai_analyzer.analyze(&prompt).await {
        Ok(response) => {
            let category = response.trim().to_string();
            // 验证返回的分类是否有效
            let valid_categories = [
                "流行",
                "轻音乐",
                "电子",
                "摇滚",
                "民谣",
                "说唱",
                "古风",
                "古典",
                "爵士",
                "蓝调",
                "ACG",
                "华语",
                "欧美",
                "日语",
                "韩语",
            ];
            if valid_categories.contains(&category.as_str()) {
                Ok(category)
            } else {
                // AI 返回了无效分类，使用默认值
                Ok("轻音乐".to_string())
            }
        }
        Err(error) => {
            tracing::error!(%error, "AI analysis failed");
            Err("AI analysis failed".to_string())
        }
    }
}

// 追加的数据读取能力

/// 获取 B 站追番列表
pub(super) async fn execute_bilibili_bangumi(
    _params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let cache_file = platform_filtered_file("bilibili");
    if let Ok(content) = tokio::fs::read_to_string(cache_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let bangumis = data
                .get("content_analysis")
                .and_then(|v| v.get("anime_analysis"))
                .cloned()
                .unwrap_or(json!([]));

            return Ok(json!({
                "source": "local_cache",
                "bangumis": bangumis,
                "items": bangumis,
                "total": bangumis.as_array().map(|a| a.len()).unwrap_or(0)
            }));
        }
    }
    Err("Failed to read Bilibili bangumi data".to_string())
}

/// 获取 Steam 愿望单
pub(super) async fn execute_steam_wishlist(
    params: &HashMap<String, Value>,
) -> Result<Value, String> {
    let _ = params; // 未使用参数
    let cache_file = platform_filtered_file("steam");
    if let Ok(content) = tokio::fs::read_to_string(cache_file).await {
        if let Ok(data) = serde_json::from_str::<Value>(&content) {
            let wishlist = data
                .get("content_analysis")
                .and_then(|v| v.get("wishlist"))
                .cloned()
                .unwrap_or(json!([]));

            return Ok(json!({
                "source": "local_cache",
                "wishlist": wishlist,
                "items": wishlist,
                "total": wishlist.as_array().map(|a| a.len()).unwrap_or(0)
            }));
        }
    }
    Err("Failed to read Steam wishlist data".to_string())
}
