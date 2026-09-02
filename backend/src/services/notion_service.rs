//! Notion 集成服务
//!
//! 支持从 Notion 数据库获取内容作为订阅源
//!
//! 使用方式：
//! 1. 创建 Notion Integration 获取 API Token
//! 2. 将数据库/页面分享给 Integration
//! 3. 使用数据库 ID 或页面 ID 作为订阅 URL
//!
//! URL 格式：
//! - notion://database/{database_id} - 订阅数据库
//! - notion://page/{page_id} - 订阅单个页面

use chrono::{DateTime, Utc};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

use crate::models::entities::brew_sources::FeedType;
use crate::services::brew_parser::{ContentFormat, ParsedFeed, ParsedItem};

/// Notion API 基础 URL
const NOTION_API_BASE: &str = "https://api.notion.com/v1";

/// Notion API 版本
const NOTION_API_VERSION: &str = "2022-06-28";

/// Notion 解析错误
#[derive(Debug)]
#[allow(dead_code)]
pub enum NotionError {
    InvalidUrl(String),
    ApiError(String),
    ParseError(String),
    MissingToken(String),
}

impl std::fmt::Display for NotionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotionError::InvalidUrl(msg) => write!(f, "Invalid Notion URL: {}", msg),
            NotionError::ApiError(msg) => write!(f, "Notion API error: {}", msg),
            NotionError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            NotionError::MissingToken(msg) => write!(f, "Missing Notion token: {}", msg),
        }
    }
}

impl std::error::Error for NotionError {}

fn notion_request_failed(error: reqwest::Error) -> NotionError {
    tracing::warn!(%error, "Notion request failed");
    if let Some(status) = error.status() {
        return NotionError::ApiError(format!("HTTP {}", status.as_u16()));
    }
    NotionError::ApiError(
        crate::services::agent::external_pure::classify_outbound_fetch(
            "Failed to reach Notion",
            &error.to_string(),
        ),
    )
}

fn notion_http_failed(status: reqwest::StatusCode, body: &str) -> NotionError {
    tracing::warn!(%status, body, "Notion HTTP error");
    let code = status.as_u16();
    match extract_notion_message(body) {
        Some(message) => NotionError::ApiError(format!("HTTP {code}: {message}")),
        None => NotionError::ApiError(format!("HTTP {code}")),
    }
}

fn notion_parse_failed(error: impl std::fmt::Display) -> NotionError {
    tracing::warn!(%error, "Failed to parse Notion response");
    NotionError::ParseError("Failed to parse Notion response".to_string())
}

fn extract_notion_message(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let message = value.get("message").and_then(|item| item.as_str())?.trim();
        if message.is_empty() || message.len() > 160 {
            return None;
        }
        return Some(message.to_string());
    }
    if trimmed.starts_with('{') || trimmed.contains("<html") {
        return None;
    }
    if trimmed.len() > 160 {
        return None;
    }
    Some(trimmed.to_string())
}

/// Notion 订阅配置
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotionConfig {
    /// Notion Integration Token
    pub token: String,
    /// 数据库 ID 或页面 ID
    pub resource_id: String,
    /// 资源类型：database 或 page
    pub resource_type: NotionResourceType,
    /// 可选的过滤条件（JSON 格式）
    pub filter: Option<Value>,
    /// 可选的排序条件（JSON 格式）
    pub sort: Option<Value>,
}

/// Notion 资源类型
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum NotionResourceType {
    #[default]
    Database,
    Page,
}

/// Notion 服务
pub struct NotionService {
    client: Client,
}

impl Default for NotionService {
    fn default() -> Self {
        Self::new()
    }
}

impl NotionService {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client }
    }

    /// 解析 Notion URL
    /// 支持格式：
    /// - notion://database/{database_id}?token={token}
    /// - notion://page/{page_id}?token={token}
    /// - https://www.notion.so/{workspace}/{database_id}?v={view_id} (需要额外提供 token)
    pub fn parse_notion_url(url: &str) -> Result<(NotionResourceType, String), NotionError> {
        // notion:// 协议格式
        if url.starts_with("notion://") {
            let path = url.strip_prefix("notion://").unwrap();
            let parts: Vec<&str> = path.split('/').collect();

            if parts.len() >= 2 {
                let resource_type = match parts[0] {
                    "database" => NotionResourceType::Database,
                    "page" => NotionResourceType::Page,
                    _ => {
                        return Err(NotionError::InvalidUrl(format!(
                            "Unknown resource type: {}",
                            parts[0]
                        )))
                    }
                };

                // 提取 ID（可能包含查询参数）
                let id = parts[1].split('?').next().unwrap_or(parts[1]);
                return Ok((resource_type, id.to_string()));
            }
        }

        // Notion 网页 URL 格式。新版客户端复制出的链接使用
        // https://app.notion.com/p/{id}，旧链接使用 notion.so/notion.site。
        let is_notion_web_url = Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(str::to_ascii_lowercase))
            .is_some_and(|host| {
                host == "notion.so"
                    || host.ends_with(".notion.so")
                    || host == "notion.site"
                    || host.ends_with(".notion.site")
                    || host == "app.notion.com"
            });

        if is_notion_web_url {
            // 提取最后一个路径段中的 ID（32字符的 hex）
            if let Some(id) = extract_notion_id_from_url(url) {
                // 默认假设是数据库，可以后续通过 API 验证
                return Ok((NotionResourceType::Database, id));
            }
        }

        Err(NotionError::InvalidUrl(format!(
            "Cannot parse Notion URL: {}",
            url
        )))
    }

    /// 获取 Notion 数据库内容
    pub async fn fetch_database(&self, config: &NotionConfig) -> Result<ParsedFeed, NotionError> {
        let url = format!("{}/databases/{}/query", NOTION_API_BASE, config.resource_id);

        // 构建请求体
        let mut body = serde_json::json!({});

        // 添加过滤条件
        if let Some(filter) = &config.filter {
            body["filter"] = filter.clone();
        }

        // 添加排序条件（默认按最后编辑时间倒序）
        if let Some(sort) = &config.sort {
            body["sorts"] = sort.clone();
        } else {
            body["sorts"] = serde_json::json!([{
                "timestamp": "last_edited_time",
                "direction": "descending"
            }]);
        }

        // 限制返回数量
        body["page_size"] = serde_json::json!(50);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", config.token))
            .header("Notion-Version", NOTION_API_VERSION)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(notion_request_failed)?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(notion_http_failed(status, &error_text));
        }

        let data: Value = response.json().await.map_err(notion_parse_failed)?;

        // 获取数据库信息
        let db_info = self.fetch_database_info(config).await?;

        // 解析结果
        let items = self.parse_database_results(&data, config).await?;

        Ok(ParsedFeed {
            title: db_info.title,
            description: db_info.description,
            site_url: Some(format!(
                "https://notion.so/{}",
                config.resource_id.replace("-", "")
            )),
            icon: db_info.icon,
            language: None,
            feed_type: FeedType::Notion,
            items,
            last_updated: Some(Utc::now()),
        })
    }

    /// 获取数据库元信息
    async fn fetch_database_info(
        &self,
        config: &NotionConfig,
    ) -> Result<DatabaseInfo, NotionError> {
        let url = format!("{}/databases/{}", NOTION_API_BASE, config.resource_id);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", config.token))
            .header("Notion-Version", NOTION_API_VERSION)
            .send()
            .await
            .map_err(notion_request_failed)?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(notion_http_failed(status, &error_text));
        }

        let data: Value = response.json().await.map_err(notion_parse_failed)?;

        // 提取标题
        let title = data["title"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|t| t["plain_text"].as_str())
            .unwrap_or("Notion Database")
            .to_string();

        // 提取描述
        let description = data["description"].as_array().and_then(|arr| {
            let texts: Vec<&str> = arr
                .iter()
                .filter_map(|t| t["plain_text"].as_str())
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join(""))
            }
        });

        // 提取图标
        let icon = data["icon"].as_object().and_then(|obj| {
            obj.get("emoji")
                .and_then(|e| e.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    obj.get("external")
                        .and_then(|e| e["url"].as_str())
                        .map(|s| s.to_string())
                })
                .or_else(|| {
                    obj.get("file")
                        .and_then(|f| f["url"].as_str())
                        .map(|s| s.to_string())
                })
        });

        Ok(DatabaseInfo {
            title,
            description,
            icon,
        })
    }

    /// 解析数据库查询结果
    async fn parse_database_results(
        &self,
        data: &Value,
        config: &NotionConfig,
    ) -> Result<Vec<ParsedItem>, NotionError> {
        let results = data["results"]
            .as_array()
            .ok_or_else(|| NotionError::ParseError("No results array".to_string()))?;

        let mut items = Vec::new();

        for page in results {
            if let Some(item) = self.parse_page_to_item(page, config).await {
                items.push(item);
            }
        }

        Ok(items)
    }

    /// 将 Notion 页面解析为文章项
    async fn parse_page_to_item(&self, page: &Value, config: &NotionConfig) -> Option<ParsedItem> {
        let id = page["id"].as_str()?;
        let url = page["url"].as_str().unwrap_or("");

        // 获取属性
        let properties = page["properties"].as_object()?;

        // 尝试获取标题（常见属性名）
        let title = self
            .extract_title_from_properties(properties)
            .unwrap_or_else(|| format!("Untitled - {}", &id[..8]));

        // 尝试获取作者
        let author = self.extract_text_property(properties, &["Author", "作者", "Created by"]);

        // 尝试获取封面图（优先级：cover > 属性中的图片 > 页面图标）
        let mut image = self.extract_page_cover(page);

        // 如果没有封面，尝试从属性中获取图片（扩展属性名列表）
        if image.is_none() {
            image = self.extract_image_property(
                properties,
                &[
                    // 英文
                    "Cover",
                    "Image",
                    "Thumbnail",
                    "Banner",
                    "Photo",
                    "Picture",
                    "Featured Image",
                    "Featured",
                    "Header",
                    "Hero",
                    "Poster",
                    "cover",
                    "image",
                    "thumbnail",
                    "banner",
                    "photo",
                    "picture",
                    // 中文
                    "封面",
                    "图片",
                    "缩略图",
                    "横幅",
                    "照片",
                    "头图",
                    "配图",
                    "主图",
                    // 日文
                    "カバー",
                    "画像",
                    "サムネイル",
                ],
            );
        }

        // 注意：不使用页面图标作为封面，因为图标通常是小尺寸的装饰性图片

        // 尝试获取分类/标签
        let categories =
            self.extract_multi_select(properties, &["Tags", "Category", "标签", "分类"]);

        // 解析时间 - 优先使用属性中的日期
        let date_from_property = self.extract_date_property(
            properties,
            &["Date", "日期", "发布日期", "Published", "创建日期"],
        );

        let created_time = date_from_property.or_else(|| {
            page["created_time"]
                .as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
        });

        let last_edited_time = page["last_edited_time"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        // 获取页面内容（可选，需要额外 API 调用）- 支持递归获取子块
        let content = self.fetch_page_content_recursive(id, config, 2).await.ok();

        // 尝试获取摘要/描述（优先级：属性 > 内容截取）
        let mut summary = self.extract_text_property(
            properties,
            &[
                // 英文
                "Summary",
                "Description",
                "Excerpt",
                "Abstract",
                "Intro",
                "Brief",
                "Overview",
                "Teaser",
                "Preview",
                "summary",
                "description",
                "excerpt",
                "abstract",
                "intro",
                // 中文
                "摘要",
                "描述",
                "简介",
                "概述",
                "内容简介",
                "导语",
                "概要",
            ],
        );

        // 如果没有摘要属性，从内容中提取
        if summary.is_none() {
            if let Some(ref content_html) = content {
                summary = Some(self.extract_summary_from_html(content_html, 200));
            }
        }

        // 如果内容中有图片且还没有封面图，提取第一张图片作为封面
        if image.is_none() {
            if let Some(ref content_html) = content {
                image = self.extract_first_image_from_html(content_html);
            }
        }

        // 尝试从 URL 属性中获取封面（有些用户会用 URL 属性存储图片链接）
        if image.is_none() {
            image = self.extract_url_as_image(properties);
        }

        Some(ParsedItem {
            guid: id.to_string(),
            title,
            link: url.to_string(),
            summary,
            content,
            author,
            image,
            audio_url: None,
            video_url: None,
            enclosures: Vec::new(),
            categories,
            published_at: created_time,
            updated_at: last_edited_time,
            content_format: ContentFormat::Html,
        })
    }

    /// 从属性中提取标题
    fn extract_title_from_properties(
        &self,
        properties: &serde_json::Map<String, Value>,
    ) -> Option<String> {
        // 常见的标题属性名
        let title_keys = ["Name", "Title", "名称", "标题", "name", "title"];

        for key in title_keys {
            if let Some(prop) = properties.get(key) {
                if let Some(title) = prop["title"].as_array() {
                    let texts: Vec<&str> = title
                        .iter()
                        .filter_map(|t| t["plain_text"].as_str())
                        .collect();
                    if !texts.is_empty() {
                        return Some(texts.join(""));
                    }
                }
            }
        }

        // 尝试找任何 title 类型的属性
        for (_, prop) in properties {
            if prop["type"].as_str() == Some("title") {
                if let Some(title) = prop["title"].as_array() {
                    let texts: Vec<&str> = title
                        .iter()
                        .filter_map(|t| t["plain_text"].as_str())
                        .collect();
                    if !texts.is_empty() {
                        return Some(texts.join(""));
                    }
                }
            }
        }

        None
    }

    /// 从属性中提取文本属性
    fn extract_text_property(
        &self,
        properties: &serde_json::Map<String, Value>,
        keys: &[&str],
    ) -> Option<String> {
        for key in keys {
            if let Some(prop) = properties.get(*key) {
                // rich_text 类型
                if let Some(rich_text) = prop["rich_text"].as_array() {
                    let texts: Vec<&str> = rich_text
                        .iter()
                        .filter_map(|t| t["plain_text"].as_str())
                        .collect();
                    if !texts.is_empty() {
                        return Some(texts.join(""));
                    }
                }
                // 普通 text 类型
                if let Some(text) = prop["text"].as_str() {
                    return Some(text.to_string());
                }
            }
        }
        None
    }

    /// 从页面 cover 字段提取封面
    fn extract_page_cover(&self, page: &Value) -> Option<String> {
        let cover = page["cover"].as_object()?;

        // external 类型
        if let Some(url) = cover.get("external").and_then(|e| e["url"].as_str()) {
            return Some(url.to_string());
        }
        // file 类型（Notion 托管）
        if let Some(url) = cover.get("file").and_then(|f| f["url"].as_str()) {
            return Some(url.to_string());
        }
        None
    }

    /// 从 URL 属性中提取可能的图片链接
    fn extract_url_as_image(&self, properties: &serde_json::Map<String, Value>) -> Option<String> {
        // 图片相关的 URL 属性名
        let image_url_keys = [
            "Image URL",
            "Cover URL",
            "Photo URL",
            "Picture URL",
            "图片链接",
            "封面链接",
            "image_url",
            "cover_url",
        ];

        for key in image_url_keys {
            if let Some(prop) = properties.get(key) {
                if let Some(url) = prop["url"].as_str() {
                    if self.is_image_url(url) {
                        return Some(url.to_string());
                    }
                }
            }
        }

        // 遍历所有 URL 类型的属性，查找可能的图片
        for (key, prop) in properties {
            if prop["type"].as_str() == Some("url") {
                if let Some(url) = prop["url"].as_str() {
                    // 检查是否是图片 URL
                    if self.is_image_url(url) {
                        // 只有当属性名暗示是图片时才使用
                        let key_lower = key.to_lowercase();
                        if key_lower.contains("image")
                            || key_lower.contains("cover")
                            || key_lower.contains("photo")
                            || key_lower.contains("图")
                            || key_lower.contains("封面")
                        {
                            return Some(url.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// 检查 URL 是否是图片链接
    fn is_image_url(&self, url: &str) -> bool {
        let url_lower = url.to_lowercase();
        // 常见图片扩展名
        let extensions = [
            ".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg", ".bmp", ".ico", ".avif",
        ];
        for ext in extensions {
            if url_lower.contains(ext) {
                return true;
            }
        }
        // 常见图片托管服务
        let image_hosts = [
            "imgur.com",
            "unsplash.com",
            "pexels.com",
            "cloudinary.com",
            "images.unsplash.com",
            "i.imgur.com",
            "cdn.",
            "img.",
            "notion.so/image",
            "s3.",
            "imagedelivery.",
            "image.",
        ];
        for host in image_hosts {
            if url_lower.contains(host) {
                return true;
            }
        }
        false
    }

    /// 从属性中提取图片
    fn extract_image_property(
        &self,
        properties: &serde_json::Map<String, Value>,
        keys: &[&str],
    ) -> Option<String> {
        for key in keys {
            if let Some(prop) = properties.get(*key) {
                // files 类型
                if let Some(files) = prop["files"].as_array() {
                    for file in files {
                        // external URL
                        if let Some(url) = file.get("external").and_then(|e| e["url"].as_str()) {
                            return Some(url.to_string());
                        }
                        // file URL (Notion hosted)
                        if let Some(url) = file.get("file").and_then(|f| f["url"].as_str()) {
                            return Some(url.to_string());
                        }
                        // 直接的 URL 字符串
                        if let Some(url) = file.get("url").and_then(|u| u.as_str()) {
                            return Some(url.to_string());
                        }
                    }
                }
                // url 类型（直接URL属性）
                if let Some(url) = prop["url"].as_str() {
                    if url.contains("http")
                        && (url.contains(".jpg")
                            || url.contains(".png")
                            || url.contains(".gif")
                            || url.contains(".webp")
                            || url.contains("image"))
                    {
                        return Some(url.to_string());
                    }
                }
            }
        }
        None
    }

    /// 从属性中提取日期
    fn extract_date_property(
        &self,
        properties: &serde_json::Map<String, Value>,
        keys: &[&str],
    ) -> Option<DateTime<Utc>> {
        for key in keys {
            if let Some(prop) = properties.get(*key) {
                // date 类型
                if let Some(date) = prop["date"].as_object() {
                    if let Some(start) = date.get("start").and_then(|s| s.as_str()) {
                        // 尝试解析完整日期时间
                        if let Ok(dt) = DateTime::parse_from_rfc3339(start) {
                            return Some(dt.with_timezone(&Utc));
                        }
                        // 尝试解析纯日期 (YYYY-MM-DD)
                        if let Ok(date) = chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d") {
                            return Some(DateTime::from_naive_utc_and_offset(
                                date.and_hms_opt(0, 0, 0).unwrap(),
                                Utc,
                            ));
                        }
                    }
                }
            }
        }
        None
    }

    /// 从 HTML 内容中提取摘要
    fn extract_summary_from_html(&self, html: &str, max_len: usize) -> String {
        // 移除 HTML 标签
        let text = html
            .replace("<br>", " ")
            .replace("<br/>", " ")
            .replace("<br />", " ")
            .replace("</p>", " ")
            .replace("</div>", " ")
            .replace("</li>", " ");

        // 简单的标签移除
        let mut result = String::new();
        let mut in_tag = false;
        for c in text.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => result.push(c),
                _ => {}
            }
        }

        // 清理多余空白
        let cleaned: String = result.split_whitespace().collect::<Vec<_>>().join(" ");

        // 截取指定长度
        if cleaned.chars().count() > max_len {
            let truncated: String = cleaned.chars().take(max_len).collect();
            format!("{}...", truncated.trim_end())
        } else {
            cleaned
        }
    }

    /// 从 HTML 内容中提取第一张有效的图片
    fn extract_first_image_from_html(&self, html: &str) -> Option<String> {
        let mut search_start = 0;

        // 遍历所有图片标签，找到第一个有效的
        while let Some(img_pos) = html[search_start..].find("<img") {
            let abs_pos = search_start + img_pos;
            let after_img = &html[abs_pos..];

            // 查找 src 属性
            if let Some(src_start) = after_img.find("src=\"").or_else(|| after_img.find("src='")) {
                let quote_char = if after_img[src_start..].starts_with("src=\"") {
                    '"'
                } else {
                    '\''
                };
                let url_start = src_start + 5;

                if let Some(url_end) = after_img[url_start..].find(quote_char) {
                    let url = &after_img[url_start..url_start + url_end];

                    // 验证是有效的图片 URL（排除数据 URI、太小的图片等）
                    if self.is_valid_cover_image(url) {
                        return Some(url.to_string());
                    }
                }
            }

            // 继续搜索下一个 img 标签
            search_start = abs_pos + 4;
        }

        // 也尝试从 figure 元素中提取
        if let Some(figure_img) = self.extract_image_from_figure(html) {
            return Some(figure_img);
        }

        None
    }

    /// 检查 URL 是否是有效的封面图片
    fn is_valid_cover_image(&self, url: &str) -> bool {
        // 必须是 http 或 https 或协议相对
        if !url.starts_with("http") && !url.starts_with("//") {
            return false;
        }

        // 排除数据 URI
        if url.starts_with("data:") {
            return false;
        }

        // 排除太短的 URL（可能是占位符）
        if url.len() < 20 {
            return false;
        }

        // 排除已知的小图标/占位符
        let skip_patterns = [
            "1x1",
            "pixel",
            "spacer",
            "blank",
            "placeholder",
            "loading",
            "spinner",
            "icon",
            "favicon",
            "logo",
            "badge",
            "button",
            "arrow",
            "bullet",
        ];
        let url_lower = url.to_lowercase();
        for pattern in skip_patterns {
            if url_lower.contains(pattern) {
                return false;
            }
        }

        true
    }

    /// 从 figure 元素中提取图片
    fn extract_image_from_figure(&self, html: &str) -> Option<String> {
        // 查找 notion-image 或 figure 标签
        let patterns = ["class=\"notion-image\"", "<figure"];

        for pattern in patterns {
            if let Some(pos) = html.find(pattern) {
                let after_pattern = &html[pos..];
                // 在这个 figure 内查找 img
                if let Some(img_pos) = after_pattern.find("<img") {
                    // 确保 img 在 figure 关闭前
                    let figure_end = after_pattern
                        .find("</figure>")
                        .unwrap_or(after_pattern.len());
                    if img_pos < figure_end {
                        let img_section = &after_pattern[img_pos..];
                        if let Some(src_start) = img_section.find("src=\"") {
                            let url_start = src_start + 5;
                            if let Some(url_end) = img_section[url_start..].find('"') {
                                let url = &img_section[url_start..url_start + url_end];
                                if self.is_valid_cover_image(url) {
                                    return Some(url.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// 从属性中提取多选标签
    fn extract_multi_select(
        &self,
        properties: &serde_json::Map<String, Value>,
        keys: &[&str],
    ) -> Vec<String> {
        for key in keys {
            if let Some(prop) = properties.get(*key) {
                // multi_select 类型
                if let Some(options) = prop["multi_select"].as_array() {
                    return options
                        .iter()
                        .filter_map(|o| o["name"].as_str())
                        .map(|s| s.to_string())
                        .collect();
                }
                // select 类型（单选）
                if let Some(option) = prop["select"].as_object() {
                    if let Some(name) = option["name"].as_str() {
                        return vec![name.to_string()];
                    }
                }
            }
        }
        Vec::new()
    }

    /// 获取页面内容（blocks）- 简单版本
    #[allow(dead_code)]
    async fn fetch_page_content(
        &self,
        page_id: &str,
        config: &NotionConfig,
    ) -> Result<String, NotionError> {
        self.fetch_page_content_recursive(page_id, config, 0).await
    }

    /// 获取页面内容（blocks）- 支持递归获取子块
    fn fetch_page_content_recursive<'a>(
        &'a self,
        block_id: &'a str,
        config: &'a NotionConfig,
        depth: u8,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, NotionError>> + Send + 'a>>
    {
        Box::pin(async move {
            // 限制递归深度，避免无限循环
            if depth > 3 {
                return Ok(String::new());
            }

            let url = format!("{}/blocks/{}/children", NOTION_API_BASE, block_id);

            let response = self
                .client
                .get(&url)
                .header("Authorization", format!("Bearer {}", config.token))
                .header("Notion-Version", NOTION_API_VERSION)
                .send()
                .await
                .map_err(notion_request_failed)?;

            if !response.status().is_success() {
                return Err(NotionError::ApiError(
                    "Failed to fetch page content".to_string(),
                ));
            }

            let data: Value = response.json().await.map_err(notion_parse_failed)?;

            let blocks = data["results"]
                .as_array()
                .ok_or_else(|| NotionError::ParseError("No blocks array".to_string()))?;

            // 将 blocks 转换为 HTML，并递归获取子块
            let html = self.blocks_to_html_recursive(blocks, config, depth).await;

            Ok(html)
        })
    }

    /// 将 Notion blocks 转换为 HTML - 支持递归获取子块内容
    fn blocks_to_html_recursive<'a>(
        &'a self,
        blocks: &'a [Value],
        config: &'a NotionConfig,
        depth: u8,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send + 'a>> {
        Box::pin(async move {
            let mut html = String::new();
            let mut in_bulleted_list = false;
            let mut in_numbered_list = false;

            for block in blocks {
                let block_type = block["type"].as_str().unwrap_or("");
                let block_id = block["id"].as_str().unwrap_or("");
                let has_children = block["has_children"].as_bool().unwrap_or(false);

                // 处理列表的开始和结束标签
                let is_bulleted = block_type == "bulleted_list_item";
                let is_numbered = block_type == "numbered_list_item";

                // 结束之前的列表
                if !is_bulleted && in_bulleted_list {
                    html.push_str("</ul>");
                    in_bulleted_list = false;
                }
                if !is_numbered && in_numbered_list {
                    html.push_str("</ol>");
                    in_numbered_list = false;
                }

                // 开始新的列表
                if is_bulleted && !in_bulleted_list {
                    html.push_str("<ul class=\"notion-list\">");
                    in_bulleted_list = true;
                }
                if is_numbered && !in_numbered_list {
                    html.push_str("<ol class=\"notion-list\">");
                    in_numbered_list = true;
                }

                // 使用同步方法处理单个块
                let block_html = self.block_to_html(block);
                html.push_str(&block_html);

                // 如果块有子内容，递归获取
                if has_children && !block_id.is_empty() {
                    // 只对特定类型递归获取子块
                    let should_recurse = matches!(
                        block_type,
                        "toggle"
                            | "callout"
                            | "quote"
                            | "bulleted_list_item"
                            | "numbered_list_item"
                            | "to_do"
                            | "column"
                            | "column_list"
                            | "synced_block"
                            | "table"
                    );

                    if should_recurse {
                        if let Ok(children_html) = self
                            .fetch_page_content_recursive(block_id, config, depth + 1)
                            .await
                        {
                            if !children_html.is_empty() {
                                // 根据块类型选择如何插入子内容
                                match block_type {
                                    "toggle" => {
                                        // toggle 的子内容放在 details 内
                                        html.push_str(&format!(
                                            "<div class=\"notion-toggle-content\">{}</div>",
                                            children_html
                                        ));
                                    }
                                    "callout" => {
                                        html.push_str(&format!(
                                            "<div class=\"notion-callout-children\">{}</div>",
                                            children_html
                                        ));
                                    }
                                    "table" => {
                                        // 表格行作为子块
                                        html.push_str(&children_html);
                                    }
                                    _ => {
                                        html.push_str(&children_html);
                                    }
                                }
                            }
                        }
                    }
                }

                // 关闭某些需要闭合的标签
                match block_type {
                    "toggle" => html.push_str("</details>"),
                    "column_list" => html.push_str("</div>"),
                    "column" => html.push_str("</div>"),
                    "table" => html.push_str("</table>"),
                    _ => {}
                }
            }

            // 关闭可能未关闭的列表
            if in_bulleted_list {
                html.push_str("</ul>");
            }
            if in_numbered_list {
                html.push_str("</ol>");
            }

            html
        })
    }

    /// 将单个 block 转换为 HTML（不含闭合标签）
    fn block_to_html(&self, block: &Value) -> String {
        let block_type = block["type"].as_str().unwrap_or("");

        match block_type {
            "paragraph" => {
                let text = self.rich_text_to_html(&block["paragraph"]["rich_text"]);
                let color = block["paragraph"]["color"].as_str().unwrap_or("default");
                if !text.is_empty() {
                    let class = self.color_to_class(color);
                    format!("<p class=\"{}\">{}</p>", class, text)
                } else {
                    String::new()
                }
            }
            "heading_1" => {
                let text = self.rich_text_to_html(&block["heading_1"]["rich_text"]);
                let color = block["heading_1"]["color"].as_str().unwrap_or("default");
                let class = self.color_to_class(color);
                let is_toggleable = block["heading_1"]["is_toggleable"]
                    .as_bool()
                    .unwrap_or(false);
                if is_toggleable {
                    format!("<details class=\"notion-heading-toggle {}\"><summary><h1>{}</h1></summary>", class, text)
                } else {
                    format!("<h1 class=\"{}\">{}</h1>", class, text)
                }
            }
            "heading_2" => {
                let text = self.rich_text_to_html(&block["heading_2"]["rich_text"]);
                let color = block["heading_2"]["color"].as_str().unwrap_or("default");
                let class = self.color_to_class(color);
                let is_toggleable = block["heading_2"]["is_toggleable"]
                    .as_bool()
                    .unwrap_or(false);
                if is_toggleable {
                    format!("<details class=\"notion-heading-toggle {}\"><summary><h2>{}</h2></summary>", class, text)
                } else {
                    format!("<h2 class=\"{}\">{}</h2>", class, text)
                }
            }
            "heading_3" => {
                let text = self.rich_text_to_html(&block["heading_3"]["rich_text"]);
                let color = block["heading_3"]["color"].as_str().unwrap_or("default");
                let class = self.color_to_class(color);
                let is_toggleable = block["heading_3"]["is_toggleable"]
                    .as_bool()
                    .unwrap_or(false);
                if is_toggleable {
                    format!("<details class=\"notion-heading-toggle {}\"><summary><h3>{}</h3></summary>", class, text)
                } else {
                    format!("<h3 class=\"{}\">{}</h3>", class, text)
                }
            }
            "bulleted_list_item" => {
                let text = self.rich_text_to_html(&block["bulleted_list_item"]["rich_text"]);
                let color = block["bulleted_list_item"]["color"]
                    .as_str()
                    .unwrap_or("default");
                let class = self.color_to_class(color);
                format!("<li class=\"{}\">{}</li>", class, text)
            }
            "numbered_list_item" => {
                let text = self.rich_text_to_html(&block["numbered_list_item"]["rich_text"]);
                let color = block["numbered_list_item"]["color"]
                    .as_str()
                    .unwrap_or("default");
                let class = self.color_to_class(color);
                format!("<li class=\"{}\">{}</li>", class, text)
            }
            "to_do" => {
                let text = self.rich_text_to_html(&block["to_do"]["rich_text"]);
                let checked = block["to_do"]["checked"].as_bool().unwrap_or(false);
                let color = block["to_do"]["color"].as_str().unwrap_or("default");
                let class = self.color_to_class(color);
                let checkbox_class = if checked {
                    "notion-checkbox checked"
                } else {
                    "notion-checkbox"
                };
                format!(
                    "<div class=\"notion-todo {}\"><span class=\"{}\"></span><span class=\"notion-todo-text{}\">{}</span></div>",
                    class, checkbox_class, if checked { " checked" } else { "" }, text
                )
            }
            "toggle" => {
                let text = self.rich_text_to_html(&block["toggle"]["rich_text"]);
                let color = block["toggle"]["color"].as_str().unwrap_or("default");
                let class = self.color_to_class(color);
                // 返回开始标签，闭合标签在处理子块后添加
                format!(
                    "<details class=\"notion-toggle {}\"><summary>{}</summary>",
                    class, text
                )
            }
            "code" => {
                let code_text = self.rich_text_to_plain(&block["code"]["rich_text"]);
                let language = block["code"]["language"].as_str().unwrap_or("plain text");
                let caption = self.rich_text_to_html(&block["code"]["caption"]);
                let escaped_code = Self::escape_html(&code_text);
                format!(
                    "<figure class=\"notion-code\"><pre><code class=\"language-{}\">{}</code></pre>{}{}{}</figure>",
                    language, escaped_code,
                    if caption.is_empty() { "" } else { "<figcaption>" },
                    caption,
                    if caption.is_empty() { "" } else { "</figcaption>" }
                )
            }
            "quote" => {
                let text = self.rich_text_to_html(&block["quote"]["rich_text"]);
                let color = block["quote"]["color"].as_str().unwrap_or("default");
                let class = self.color_to_class(color);
                format!(
                    "<blockquote class=\"notion-quote {}\">{}</blockquote>",
                    class, text
                )
            }
            "callout" => {
                let text = self.rich_text_to_html(&block["callout"]["rich_text"]);
                let color = block["callout"]["color"].as_str().unwrap_or("default");
                let class = self.color_to_class(color);
                let bg_class = self.callout_color_to_bg_class(color);
                let icon = if let Some(emoji) = block["callout"]["icon"]["emoji"].as_str() {
                    format!("<span class=\"notion-callout-icon\">{}</span>", emoji)
                } else if let Some(url) = self.get_file_url(&block["callout"]["icon"]) {
                    format!(
                        "<img src=\"{}\" class=\"notion-callout-icon\" alt=\"\" />",
                        url
                    )
                } else {
                    "<span class=\"notion-callout-icon\">💡</span>".to_string()
                };
                format!(
                    "<div class=\"notion-callout {} {}\">{}<div class=\"notion-callout-content\">{}</div></div>",
                    class, bg_class, icon, text
                )
            }
            "divider" => "<hr class=\"notion-divider\"/>".to_string(),
            "image" => {
                if let Some(url) = self.get_file_url(&block["image"]) {
                    let caption = self.rich_text_to_html(&block["image"]["caption"]);
                    format!(
                        "<figure class=\"notion-image\"><img src=\"{}\" alt=\"{}\" loading=\"lazy\" />{}{}{}</figure>",
                        url, Self::escape_html(&caption),
                        if caption.is_empty() { "" } else { "<figcaption>" },
                        caption,
                        if caption.is_empty() { "" } else { "</figcaption>" }
                    )
                } else {
                    String::new()
                }
            }
            "video" => self.video_block_to_html(block),
            "audio" => {
                if let Some(url) = self.get_file_url(&block["audio"]) {
                    let caption = self.rich_text_to_html(&block["audio"]["caption"]);
                    format!(
                        "<figure class=\"notion-audio\"><audio src=\"{}\" controls preload=\"metadata\"></audio>{}{}{}</figure>",
                        url,
                        if caption.is_empty() { "" } else { "<figcaption>" },
                        caption,
                        if caption.is_empty() { "" } else { "</figcaption>" }
                    )
                } else {
                    String::new()
                }
            }
            "file" => {
                if let Some(url) = self.get_file_url(&block["file"]) {
                    let caption = self.rich_text_to_html(&block["file"]["caption"]);
                    let name = block["file"]["name"].as_str().unwrap_or("附件");
                    let display_name = if caption.is_empty() { name } else { &caption };
                    format!(
                        "<a href=\"{}\" class=\"notion-file\" target=\"_blank\" download><span class=\"notion-file-icon\">📎</span><span class=\"notion-file-name\">{}</span></a>",
                        url, display_name
                    )
                } else {
                    String::new()
                }
            }
            "pdf" => {
                if let Some(url) = self.get_file_url(&block["pdf"]) {
                    let caption = self.rich_text_to_html(&block["pdf"]["caption"]);
                    format!(
                        "<figure class=\"notion-pdf\"><iframe src=\"{}\" class=\"notion-pdf-embed\"></iframe>{}{}{}</figure>",
                        url,
                        if caption.is_empty() { "" } else { "<figcaption>" },
                        caption,
                        if caption.is_empty() { "" } else { "</figcaption>" }
                    )
                } else {
                    String::new()
                }
            }
            "bookmark" => {
                if let Some(url) = block["bookmark"]["url"].as_str() {
                    let caption = self.rich_text_to_html(&block["bookmark"]["caption"]);
                    let display = if caption.is_empty() {
                        Self::extract_domain(url)
                    } else {
                        caption.clone()
                    };
                    format!(
                        "<a href=\"{}\" class=\"notion-bookmark\" target=\"_blank\" rel=\"noopener noreferrer\"><span class=\"notion-bookmark-icon\">🔗</span><span class=\"notion-bookmark-title\">{}</span><span class=\"notion-bookmark-url\">{}</span></a>",
                        url, display, url
                    )
                } else {
                    String::new()
                }
            }
            "link_preview" => {
                if let Some(url) = block["link_preview"]["url"].as_str() {
                    let domain = Self::extract_domain(url);
                    format!(
                        "<a href=\"{}\" class=\"notion-link-preview\" target=\"_blank\" rel=\"noopener noreferrer\"><span class=\"notion-link-icon\">🔗</span><span>{}</span></a>",
                        url, domain
                    )
                } else {
                    String::new()
                }
            }
            "embed" => {
                if let Some(url) = block["embed"]["url"].as_str() {
                    let caption = self.rich_text_to_html(&block["embed"]["caption"]);
                    format!(
                        "<figure class=\"notion-embed\"><div class=\"notion-embed-wrapper\"><iframe src=\"{}\" allowfullscreen></iframe></div>{}{}{}</figure>",
                        url,
                        if caption.is_empty() { "" } else { "<figcaption>" },
                        caption,
                        if caption.is_empty() { "" } else { "</figcaption>" }
                    )
                } else {
                    String::new()
                }
            }
            "equation" => {
                if let Some(expression) = block["equation"]["expression"].as_str() {
                    format!(
                        "<div class=\"notion-equation\">$${}$$</div>",
                        Self::escape_html(expression)
                    )
                } else {
                    String::new()
                }
            }
            "table_of_contents" => "<nav class=\"notion-toc\"><p>📋 目录</p></nav>".to_string(),
            "breadcrumb" => String::new(),
            "column_list" => "<div class=\"notion-columns\">".to_string(),
            "column" => "<div class=\"notion-column\">".to_string(),
            "synced_block" => String::new(),
            "template" => String::new(),
            "link_to_page" => {
                let page_id = block["link_to_page"]["page_id"].as_str().unwrap_or("");
                if !page_id.is_empty() {
                    format!(
                        "<a href=\"https://notion.so/{}\" class=\"notion-page-link\" target=\"_blank\">📄 链接的页面</a>",
                        page_id.replace('-', "")
                    )
                } else {
                    String::new()
                }
            }
            "child_page" => {
                // 子页面块
                let title = block["child_page"]["title"].as_str().unwrap_or("子页面");
                let block_id = block["id"].as_str().unwrap_or("");
                format!(
                    "<a href=\"https://notion.so/{}\" class=\"notion-child-page\" target=\"_blank\"><span class=\"notion-page-icon\">📄</span><span class=\"notion-page-title\">{}</span></a>",
                    block_id.replace('-', ""),
                    Self::escape_html(title)
                )
            }
            "child_database" => {
                // 子数据库块
                let title = block["child_database"]["title"]
                    .as_str()
                    .unwrap_or("数据库");
                let block_id = block["id"].as_str().unwrap_or("");
                format!(
                    "<a href=\"https://notion.so/{}\" class=\"notion-child-database\" target=\"_blank\"><span class=\"notion-database-icon\">📊</span><span class=\"notion-database-title\">{}</span></a>",
                    block_id.replace('-', ""),
                    Self::escape_html(title)
                )
            }
            "table" => {
                let has_header = block["table"]["has_column_header"]
                    .as_bool()
                    .unwrap_or(false);
                format!(
                    "<table class=\"notion-table{}\">",
                    if has_header { " has-header" } else { "" }
                )
            }
            "table_row" => {
                let cells = block["table_row"]["cells"].as_array();
                if let Some(cells) = cells {
                    let mut row = String::from("<tr>");
                    for cell in cells {
                        let cell_text = self.rich_text_to_html(cell);
                        row.push_str(&format!("<td>{}</td>", cell_text));
                    }
                    row.push_str("</tr>");
                    row
                } else {
                    String::new()
                }
            }
            _ => {
                // 未知类型，尝试提取 rich_text
                if let Some(content) = block.get(block_type) {
                    if let Some(rich_text) = content.get("rich_text") {
                        let text = self.rich_text_to_html(rich_text);
                        if !text.is_empty() {
                            return format!("<p>{}</p>", text);
                        }
                    }
                }
                String::new()
            }
        }
    }

    /// 视频块转 HTML
    fn video_block_to_html(&self, block: &Value) -> String {
        if let Some(url) = self.get_file_url(&block["video"]) {
            let caption = self.rich_text_to_html(&block["video"]["caption"]);

            // 检查是否是嵌入式视频
            if url.contains("youtube.com") || url.contains("youtu.be") {
                let video_id = Self::extract_youtube_id(&url);
                format!(
                    "<figure class=\"notion-video\"><div class=\"notion-video-embed\"><iframe src=\"https://www.youtube.com/embed/{}\" allowfullscreen></iframe></div>{}{}{}</figure>",
                    video_id,
                    if caption.is_empty() { "" } else { "<figcaption>" },
                    caption,
                    if caption.is_empty() { "" } else { "</figcaption>" }
                )
            } else if url.contains("vimeo.com") {
                let video_id = Self::extract_vimeo_id(&url);
                format!(
                    "<figure class=\"notion-video\"><div class=\"notion-video-embed\"><iframe src=\"https://player.vimeo.com/video/{}\" allowfullscreen></iframe></div>{}{}{}</figure>",
                    video_id,
                    if caption.is_empty() { "" } else { "<figcaption>" },
                    caption,
                    if caption.is_empty() { "" } else { "</figcaption>" }
                )
            } else if url.contains("bilibili.com") {
                let bvid = Self::extract_bilibili_id(&url);
                format!(
                    "<figure class=\"notion-video\"><div class=\"notion-video-embed\"><iframe src=\"https://player.bilibili.com/player.html?bvid={}&high_quality=1\" allowfullscreen></iframe></div>{}{}{}</figure>",
                    bvid,
                    if caption.is_empty() { "" } else { "<figcaption>" },
                    caption,
                    if caption.is_empty() { "" } else { "</figcaption>" }
                )
            } else {
                format!(
                    "<figure class=\"notion-video\"><video src=\"{}\" controls preload=\"metadata\"></video>{}{}{}</figure>",
                    url,
                    if caption.is_empty() { "" } else { "<figcaption>" },
                    caption,
                    if caption.is_empty() { "" } else { "</figcaption>" }
                )
            }
        } else {
            String::new()
        }
    }

    /// callout 颜色转背景色 class
    fn callout_color_to_bg_class(&self, color: &str) -> String {
        match color {
            "gray_background" => "notion-callout-gray".to_string(),
            "brown_background" => "notion-callout-brown".to_string(),
            "orange_background" => "notion-callout-orange".to_string(),
            "yellow_background" => "notion-callout-yellow".to_string(),
            "green_background" => "notion-callout-green".to_string(),
            "blue_background" => "notion-callout-blue".to_string(),
            "purple_background" => "notion-callout-purple".to_string(),
            "pink_background" => "notion-callout-pink".to_string(),
            "red_background" => "notion-callout-red".to_string(),
            _ => String::new(),
        }
    }

    /// 将颜色转换为 CSS class
    fn color_to_class(&self, color: &str) -> String {
        match color {
            "gray" => "notion-gray".to_string(),
            "brown" => "notion-brown".to_string(),
            "orange" => "notion-orange".to_string(),
            "yellow" => "notion-yellow".to_string(),
            "green" => "notion-green".to_string(),
            "blue" => "notion-blue".to_string(),
            "purple" => "notion-purple".to_string(),
            "pink" => "notion-pink".to_string(),
            "red" => "notion-red".to_string(),
            "gray_background" => "notion-bg-gray".to_string(),
            "brown_background" => "notion-bg-brown".to_string(),
            "orange_background" => "notion-bg-orange".to_string(),
            "yellow_background" => "notion-bg-yellow".to_string(),
            "green_background" => "notion-bg-green".to_string(),
            "blue_background" => "notion-bg-blue".to_string(),
            "purple_background" => "notion-bg-purple".to_string(),
            "pink_background" => "notion-bg-pink".to_string(),
            "red_background" => "notion-bg-red".to_string(),
            _ => String::new(),
        }
    }

    /// 将 rich_text 转换为纯文本（用于代码块等）
    fn rich_text_to_plain(&self, rich_text: &Value) -> String {
        let arr = match rich_text.as_array() {
            Some(a) => a,
            None => return String::new(),
        };

        arr.iter()
            .filter_map(|item| item["plain_text"].as_str())
            .collect::<Vec<_>>()
            .join("")
    }

    /// 从 YouTube URL 提取视频 ID
    fn extract_youtube_id(url: &str) -> String {
        // 支持多种 YouTube URL 格式
        if let Some(pos) = url.find("v=") {
            url[pos + 2..].split('&').next().unwrap_or("").to_string()
        } else if let Some(pos) = url.find("youtu.be/") {
            url[pos + 9..].split('?').next().unwrap_or("").to_string()
        } else if let Some(pos) = url.find("embed/") {
            url[pos + 6..].split('?').next().unwrap_or("").to_string()
        } else {
            String::new()
        }
    }

    /// 从 Vimeo URL 提取视频 ID
    fn extract_vimeo_id(url: &str) -> String {
        url.split('/')
            .next_back()
            .unwrap_or("")
            .split('?')
            .next()
            .unwrap_or("")
            .to_string()
    }

    /// 从 Bilibili URL 提取 BV 号
    fn extract_bilibili_id(url: &str) -> String {
        if let Some(pos) = url.find("BV") {
            url[pos..]
                .split(|c: char| !c.is_alphanumeric())
                .next()
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        }
    }

    /// 从 URL 提取域名
    fn extract_domain(url: &str) -> String {
        url.replace("https://", "")
            .replace("http://", "")
            .split('/')
            .next()
            .unwrap_or(url)
            .to_string()
    }

    /// 将 rich_text 转换为 HTML（支持颜色、样式、mention 和 equation）
    fn rich_text_to_html(&self, rich_text: &Value) -> String {
        let arr = match rich_text.as_array() {
            Some(a) => a,
            None => return String::new(),
        };

        let mut html = String::new();

        for item in arr {
            let item_type = item["type"].as_str().unwrap_or("text");
            let annotations = &item["annotations"];
            let href = item["href"].as_str();

            // 根据类型获取内容
            let mut result = match item_type {
                "mention" => {
                    // 处理 mention 类型（@用户、@页面、@日期、@数据库等）
                    self.mention_to_html(item)
                }
                "equation" => {
                    // 处理行内公式
                    let expression = item["equation"]["expression"].as_str().unwrap_or("");
                    format!(
                        "<span class=\"notion-inline-equation\">${}$</span>",
                        Self::escape_html(expression)
                    )
                }
                _ => {
                    // text 类型或其他
                    let text = item["plain_text"].as_str().unwrap_or("");
                    Self::escape_html(text)
                }
            };

            // 应用样式（对于非 mention 类型）
            if item_type != "mention" {
                if annotations["bold"].as_bool().unwrap_or(false) {
                    result = format!("<strong>{}</strong>", result);
                }
                if annotations["italic"].as_bool().unwrap_or(false) {
                    result = format!("<em>{}</em>", result);
                }
                if annotations["strikethrough"].as_bool().unwrap_or(false) {
                    result = format!("<del>{}</del>", result);
                }
                if annotations["underline"].as_bool().unwrap_or(false) {
                    result = format!("<u>{}</u>", result);
                }
                if annotations["code"].as_bool().unwrap_or(false) {
                    result = format!("<code>{}</code>", result);
                }

                // 颜色（文字颜色和背景色）
                let color = annotations["color"].as_str().unwrap_or("default");
                if color != "default" {
                    let class = self.color_to_class(color);
                    if !class.is_empty() {
                        result = format!("<span class=\"{}\">{}</span>", class, result);
                    }
                }
            }

            // 链接
            if let Some(url) = href {
                result = format!("<a href=\"{}\">{}</a>", url, result);
            }

            html.push_str(&result);
        }

        html
    }

    /// 将 mention 转换为 HTML
    fn mention_to_html(&self, item: &Value) -> String {
        let mention = &item["mention"];
        let mention_type = mention["type"].as_str().unwrap_or("");
        let plain_text = item["plain_text"].as_str().unwrap_or("");

        match mention_type {
            "user" => {
                // @用户
                let name = mention["user"]["name"].as_str().unwrap_or(plain_text);
                let avatar = mention["user"]["avatar_url"].as_str();
                if let Some(avatar_url) = avatar {
                    format!(
                        "<span class=\"notion-mention notion-mention-user\"><img src=\"{}\" class=\"notion-mention-avatar\" alt=\"\" /><span class=\"notion-mention-name\">@{}</span></span>",
                        avatar_url,
                        Self::escape_html(name)
                    )
                } else {
                    format!(
                        "<span class=\"notion-mention notion-mention-user\"><span class=\"notion-mention-name\">@{}</span></span>",
                        Self::escape_html(name)
                    )
                }
            }
            "page" => {
                // @页面
                let page_id = mention["page"]["id"].as_str().unwrap_or("");
                format!(
                    "<a href=\"https://notion.so/{}\" class=\"notion-mention notion-mention-page\" target=\"_blank\"><span class=\"notion-mention-icon\">📄</span><span class=\"notion-mention-title\">{}</span></a>",
                    page_id.replace('-', ""),
                    Self::escape_html(plain_text)
                )
            }
            "database" => {
                // @数据库
                let db_id = mention["database"]["id"].as_str().unwrap_or("");
                format!(
                    "<a href=\"https://notion.so/{}\" class=\"notion-mention notion-mention-database\" target=\"_blank\"><span class=\"notion-mention-icon\">📊</span><span class=\"notion-mention-title\">{}</span></a>",
                    db_id.replace('-', ""),
                    Self::escape_html(plain_text)
                )
            }
            "date" => {
                // @日期
                let start = mention["date"]["start"].as_str().unwrap_or("");
                let end = mention["date"]["end"].as_str();
                let date_display = if let Some(end_date) = end {
                    format!("{} → {}", start, end_date)
                } else {
                    start.to_string()
                };
                format!(
                    "<span class=\"notion-mention notion-mention-date\"><span class=\"notion-mention-icon\">📅</span><span class=\"notion-mention-value\">{}</span></span>",
                    Self::escape_html(&date_display)
                )
            }
            "link_preview" => {
                // 链接预览
                let url = mention["link_preview"]["url"].as_str().unwrap_or("");
                format!(
                    "<a href=\"{}\" class=\"notion-mention notion-mention-link\" target=\"_blank\" rel=\"noopener noreferrer\"><span class=\"notion-mention-icon\">🔗</span><span class=\"notion-mention-title\">{}</span></a>",
                    url,
                    Self::escape_html(plain_text)
                )
            }
            "template_mention" => {
                // 模板提及（如 @today, @now 等）
                let template_type = mention["template_mention"]["type"].as_str().unwrap_or("");
                let icon = match template_type {
                    "template_mention_date" => "📅",
                    "template_mention_user" => "👤",
                    _ => "📝",
                };
                format!(
                    "<span class=\"notion-mention notion-mention-template\"><span class=\"notion-mention-icon\">{}</span><span class=\"notion-mention-value\">{}</span></span>",
                    icon,
                    Self::escape_html(plain_text)
                )
            }
            _ => {
                // 未知类型，显示原文
                format!(
                    "<span class=\"notion-mention\">{}</span>",
                    Self::escape_html(plain_text)
                )
            }
        }
    }

    /// 获取文件 URL（支持 external 和 file 类型）
    fn get_file_url(&self, file_obj: &Value) -> Option<String> {
        if let Some(url) = file_obj["external"]["url"].as_str() {
            return Some(url.to_string());
        }
        if let Some(url) = file_obj["file"]["url"].as_str() {
            return Some(url.to_string());
        }
        None
    }

    /// 获取单个页面内容
    pub async fn fetch_page(&self, config: &NotionConfig) -> Result<ParsedFeed, NotionError> {
        let url = format!("{}/pages/{}", NOTION_API_BASE, config.resource_id);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", config.token))
            .header("Notion-Version", NOTION_API_VERSION)
            .send()
            .await
            .map_err(notion_request_failed)?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(notion_http_failed(status, &error_text));
        }

        let page: Value = response.json().await.map_err(notion_parse_failed)?;

        // 解析页面为单个文章
        let item = self
            .parse_page_to_item(&page, config)
            .await
            .ok_or_else(|| NotionError::ParseError("Failed to parse page".to_string()))?;

        Ok(ParsedFeed {
            title: item.title.clone(),
            description: item.summary.clone(),
            site_url: Some(item.link.clone()),
            icon: None,
            language: None,
            feed_type: FeedType::Notion,
            items: vec![item],
            last_updated: Some(Utc::now()),
        })
    }

    /// 根据配置获取内容
    /// 如果资源类型未知（从网页 URL 解析），会自动尝试检测
    pub async fn fetch(&self, config: &NotionConfig) -> Result<ParsedFeed, NotionError> {
        match config.resource_type {
            NotionResourceType::Database => {
                // 尝试作为数据库获取
                match self.fetch_database(config).await {
                    Ok(feed) => Ok(feed),
                    Err(NotionError::ApiError(msg))
                        if msg.contains("is a page, not a database") =>
                    {
                        // 自动切换到页面模式
                        tracing::info!(
                            "Resource {} is a page, switching to page API",
                            config.resource_id
                        );
                        let page_config = NotionConfig {
                            resource_type: NotionResourceType::Page,
                            ..config.clone()
                        };
                        self.fetch_page(&page_config).await
                    }
                    Err(e) => Err(e),
                }
            }
            NotionResourceType::Page => self.fetch_page(config).await,
        }
    }

    /// HTML 转义辅助函数
    fn escape_html(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#x27;")
    }
}

/// 数据库信息
struct DatabaseInfo {
    title: String,
    description: Option<String>,
    icon: Option<String>,
}

/// 从 Notion URL 中提取 ID
fn extract_notion_id_from_url(url: &str) -> Option<String> {
    // Notion ID 是 32 字符的 hex（有时带短横线）
    let clean_url = url.split('?').next().unwrap_or(url);
    let parts: Vec<&str> = clean_url.split('/').collect();

    // 从后向前查找，找到第一个看起来像 ID 的部分
    for part in parts.iter().rev() {
        // 移除可能的页面标题前缀（格式：Title-xxxxx）
        let id_part = if part.contains('-') {
            // 找最后一个部分（可能是 ID）
            part.split('-').next_back().unwrap_or(part)
        } else {
            part
        };

        // 检查是否是 32 字符的 hex
        let clean_id = id_part.replace("-", "");
        if clean_id.len() == 32 && clean_id.chars().all(|c| c.is_ascii_hexdigit()) {
            // 返回带短横线格式的 ID
            return Some(format!(
                "{}-{}-{}-{}-{}",
                &clean_id[0..8],
                &clean_id[8..12],
                &clean_id[12..16],
                &clean_id[16..20],
                &clean_id[20..32]
            ));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_notion_message_keeps_official_phrase_and_drops_json() {
        assert_eq!(
            extract_notion_message(
                r#"{"object":"error","status":401,"code":"unauthorized","message":"API token is invalid."}"#
            )
            .as_deref(),
            Some("API token is invalid.")
        );
        assert_eq!(
            extract_notion_message(r#"{"object":"error","status":500}"#),
            None
        );
        assert_eq!(extract_notion_message("<html>nope</html>"), None);
    }

    #[test]
    fn test_parse_notion_url() {
        // notion:// 协议
        let (resource_type, id) =
            NotionService::parse_notion_url("notion://database/abc123def456789").unwrap();
        assert_eq!(resource_type, NotionResourceType::Database);
        assert_eq!(id, "abc123def456789");

        let (resource_type, id) = NotionService::parse_notion_url("notion://page/xyz789").unwrap();
        assert_eq!(resource_type, NotionResourceType::Page);
        assert_eq!(id, "xyz789");

        let (resource_type, id) = NotionService::parse_notion_url(
            "https://app.notion.com/p/2d19015ffdc880dea8d3c523ab64bf1d?v=2d19015ffdc88070936f000c7b2fa774",
        )
        .unwrap();
        assert_eq!(resource_type, NotionResourceType::Database);
        assert_eq!(id, "2d19015f-fdc8-80de-a8d3-c523ab64bf1d");

        assert!(NotionService::parse_notion_url(
            "https://app.notion.com.example.com/p/2d19015ffdc880dea8d3c523ab64bf1d"
        )
        .is_err());
    }

    #[test]
    fn test_extract_notion_id() {
        // 标准 Notion URL
        let id = extract_notion_id_from_url(
            "https://www.notion.so/workspace/My-Database-a1b2c3d4e5f6789012345678abcdef90",
        );
        assert!(id.is_some());

        // 带查询参数
        let id =
            extract_notion_id_from_url("https://notion.so/a1b2c3d4e5f6789012345678abcdef90?v=123");
        assert!(id.is_some());
    }
}
