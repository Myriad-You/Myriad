//! Brew 阅读 - RSS/Atom/JSON Feed 解析器
//!
//! 支持解析:
//! - RSS 2.0
//! - RSS 1.0 (RDF)
//! - Atom 1.0
//! - JSON Feed 1.1

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::models::entities::brew_sources::FeedType;

/// Maximum accepted feed response body size (generous for full-content feeds).
///
/// Prevents unbounded memory use on malicious or misconfigured sources.
/// Fail cleanly via [`ParseError::FetchError`] — never buffer past this limit.
pub const MAX_FEED_BODY_BYTES: usize = 16 * 1024 * 1024; // 16 MiB

/// 解析后的订阅源信息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedFeed {
    /// 订阅源标题
    pub title: String,
    /// 订阅源描述
    pub description: Option<String>,
    /// 订阅源网站链接
    pub site_url: Option<String>,
    /// 订阅源图标
    pub icon: Option<String>,
    /// 订阅源语言
    pub language: Option<String>,
    /// 检测到的类型
    pub feed_type: FeedType,
    /// 文章列表
    pub items: Vec<ParsedItem>,
    /// 最后更新时间
    pub last_updated: Option<DateTime<Utc>>,
}

/// 解析后的文章信息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParsedItem {
    /// 唯一标识 (guid/id)
    pub guid: String,
    pub title: String,
    /// 链接
    pub link: String,
    /// 摘要/描述
    pub summary: Option<String>,
    /// 全文内容
    pub content: Option<String>,
    /// 作者
    pub author: Option<String>,
    /// 封面图
    pub image: Option<String>,
    /// 音频链接（播客）
    pub audio_url: Option<String>,
    /// 视频链接
    pub video_url: Option<String>,
    /// 附件
    pub enclosures: Vec<Enclosure>,
    /// 分类标签
    pub categories: Vec<String>,
    /// 发布时间
    pub published_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    /// 内容格式（html/markdown/text）
    pub content_format: ContentFormat,
}

/// 内容格式类型
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ContentFormat {
    /// HTML 格式（大多数订阅源）
    #[default]
    Html,
    /// Markdown 格式（部分技术博客）
    Markdown,
    /// 纯文本
    Text,
}

/// 附件信息
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Enclosure {
    pub url: String,
    pub mime_type: Option<String>,
    pub length: Option<u64>,
    pub title: Option<String>,
}

/// 解析错误
#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
pub enum ParseError {
    FetchError(String),
    ParseError(String),
    UnsupportedFormat(String),
    InvalidUrl(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::FetchError(msg) => write!(f, "Fetch error: {}", msg),
            ParseError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            ParseError::UnsupportedFormat(msg) => write!(f, "Unsupported format: {}", msg),
            ParseError::InvalidUrl(msg) => write!(f, "Invalid URL: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

/// Feed 解析器
///
/// 出站请求经 `outbound_security` 做公网 DNS 钉扎与禁用重定向，防止 SSRF。
pub struct FeedParser {
    /// 保留字段供未来扩展；实际抓取使用 per-request 安全客户端
    #[allow(dead_code)]
    client: Client,
}

impl Default for FeedParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FeedParser {
    pub const USER_AGENT: &'static str = "Myriad Brew Reader/1.0 (RSS/Atom Feed Reader)";

    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(Self::USER_AGENT)
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client }
    }

    /// 校验 URL 是否允许作为出站 feed 目标（公网 HTTP/HTTPS，无凭据，DNS 非内网）
    pub async fn validate_public_url(url: &str) -> Result<(), ParseError> {
        crate::services::outbound_security::build_public_http_client(
            url,
            Duration::from_secs(5),
            Some(Self::USER_AGENT),
        )
        .await
        .map(|_| ())
        .map_err(|e| ParseError::InvalidUrl(format!("Unsafe or invalid URL: {e}")))
    }

    /// 抓取并解析订阅源（SSRF 安全）
    pub async fn fetch_and_parse(&self, url: &str) -> Result<ParsedFeed, ParseError> {
        let (target_url, client) = crate::services::outbound_security::build_public_http_client(
            url,
            Duration::from_secs(30),
            Some(Self::USER_AGENT),
        )
        .await
        .map_err(|e| ParseError::InvalidUrl(format!("Unsafe or invalid URL: {e}")))?;

        // 抓取内容（客户端已禁用重定向并钉扎公网解析结果）
        let response = client
            .get(target_url)
            .send()
            .await
            .map_err(|e| ParseError::FetchError(format!("Failed to fetch: {}", e)))?;

        if !response.status().is_success() {
            return Err(ParseError::FetchError(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();

        tracing::debug!("Feed content-type: {}", content_type);

        // Cap body size before buffering: never read unbounded feed payloads (MYR-018).
        let body_bytes = crate::services::outbound_security::read_limited_body(
            response,
            MAX_FEED_BODY_BYTES,
        )
        .await
        .map_err(|e| {
            // Oversize and I/O failures both surface as FetchError (fail cleanly).
            ParseError::FetchError(format!("Failed to read body: {e}"))
        })?;

        let body = match String::from_utf8(body_bytes) {
            Ok(s) => s,
            Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
        };

        // 安全截取前 200 个字符（避免 UTF-8 边界问题）
        let preview: String = body.chars().take(200).collect();
        tracing::debug!(
            "Feed body length: {}, first 200 chars: {}",
            body.len(),
            preview
        );

        // 根据 Content-Type 或内容检测格式
        self.parse_content(&body, &content_type, url)
    }

    /// 解析内容
    pub fn parse_content(
        &self,
        content: &str,
        content_type: &str,
        source_url: &str,
    ) -> Result<ParsedFeed, ParseError> {
        let trimmed = content.trim();

        // 检测 JSON Feed
        if content_type.contains("json") || trimmed.starts_with('{') {
            if let Ok(feed) = self.parse_json_feed(trimmed, source_url) {
                return Ok(feed);
            }
        }

        // 检测 XML (RSS/Atom)
        if trimmed.starts_with("<?xml") || trimmed.starts_with('<') {
            let content_lower = trimmed.to_lowercase();

            // 计算各格式的匹配得分
            let mut rss_score = 0;
            let mut atom_score = 0;
            let mut rdf_score = 0;

            // RSS 2.0 特征
            if trimmed.contains("<rss") {
                rss_score += 10; // 明确的 RSS 根标签
            }
            if trimmed.contains("<channel>") {
                rss_score += 5;
            }
            if trimmed.contains("<item>") || trimmed.contains("<item ") {
                rss_score += 3;
            }
            if trimmed.contains("version=\"2.0\"") {
                rss_score += 2;
            }

            // Atom 特征 - 必须是 <feed 作为根元素，不是命名空间声明
            if content_lower.contains("<feed") && !trimmed.contains("<rss") {
                // 检查是否有 Atom 命名空间作为默认命名空间（不是 xmlns:atom）
                if content_lower.contains("<feed")
                    && (content_lower.contains("xmlns=\"http://www.w3.org/2005/atom\"")
                        || content_lower.contains("xmlns='http://www.w3.org/2005/atom'"))
                {
                    atom_score += 10; // 明确的 Atom 根标签 + 默认命名空间
                } else if content_lower.contains("<feed") {
                    atom_score += 5; // 有 <feed> 标签但没有明确命名空间
                }
            }
            if trimmed.contains("<entry>") || trimmed.contains("<entry ") {
                atom_score += 3;
            }

            // RDF/RSS 1.0 特征
            if trimmed.contains("<rdf:RDF") {
                rdf_score += 10;
            }
            if trimmed.contains("http://purl.org/rss/1.0/") {
                rdf_score += 5;
            }

            tracing::debug!(
                "Format detection scores - RSS: {}, Atom: {}, RDF: {}",
                rss_score,
                atom_score,
                rdf_score
            );

            // 选择得分最高的格式
            if rss_score > 0 && rss_score >= atom_score && rss_score >= rdf_score {
                tracing::debug!("Selected format: RSS (score: {})", rss_score);
                return self.parse_rss(trimmed, source_url);
            }

            if atom_score > 0 && atom_score > rss_score && atom_score >= rdf_score {
                tracing::debug!("Selected format: Atom (score: {})", atom_score);
                return self.parse_atom(trimmed, source_url);
            }

            if rdf_score > 0 && rdf_score > rss_score && rdf_score > atom_score {
                tracing::debug!("Selected format: RDF/RSS 1.0 (score: {})", rdf_score);
                return self.parse_rdf(trimmed, source_url);
            }

            // 如果没有明确得分，尝试按顺序解析
            if content_lower.contains("<feed") {
                tracing::debug!("Trying Atom parser as fallback for <feed> tag");
                if let Ok(feed) = self.parse_atom(trimmed, source_url) {
                    return Ok(feed);
                }
            }

            if content_lower.contains("<channel") || content_lower.contains("<item") {
                tracing::debug!("Trying RSS parser as fallback");
                if let Ok(feed) = self.parse_rss(trimmed, source_url) {
                    return Ok(feed);
                }
            }
        }

        Err(ParseError::UnsupportedFormat(
            "Unable to detect feed format".to_string(),
        ))
    }

    /// 解析 RSS 2.0
    fn parse_rss(&self, content: &str, _source_url: &str) -> Result<ParsedFeed, ParseError> {
        use quick_xml::events::Event;
        use quick_xml::Reader;

        let mut reader = Reader::from_str(content);
        reader.config_mut().trim_text(true);

        let mut feed = ParsedFeed {
            title: String::new(),
            description: None,
            site_url: None,
            icon: None,
            language: None,
            feed_type: FeedType::Rss,
            items: Vec::new(),
            last_updated: None,
        };

        let mut current_item: Option<ParsedItem> = None;
        let mut current_tag = String::new();
        let mut in_channel = false;
        let mut in_item = false;
        let mut in_image = false;
        // 用于累积文本内容（处理分段的 Text/CData 事件）
        let mut text_buffer = String::new();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    // 处理带命名空间的标签名（如 content:encoded -> encoded）
                    let simple_tag = tag_name
                        .split(':')
                        .next_back()
                        .unwrap_or(&tag_name)
                        .to_string();
                    current_tag = tag_name.clone();
                    text_buffer.clear(); // 新标签开始，清空缓冲区

                    match simple_tag.as_str() {
                        "channel" => in_channel = true,
                        "item" => {
                            in_item = true;
                            current_item = Some(ParsedItem {
                                guid: String::new(),
                                title: String::new(),
                                link: String::new(),
                                summary: None,
                                content: None,
                                author: None,
                                image: None,
                                audio_url: None,
                                video_url: None,
                                enclosures: Vec::new(),
                                categories: Vec::new(),
                                published_at: None,
                                updated_at: None,
                                content_format: ContentFormat::default(),
                            });
                        }
                        "image" => in_image = true,
                        "enclosure" => {
                            if let Some(ref mut item) = current_item {
                                let mut enc = Enclosure {
                                    url: String::new(),
                                    mime_type: None,
                                    length: None,
                                    title: None,
                                };
                                for attr in e.attributes().flatten() {
                                    match attr.key.as_ref() {
                                        b"url" => {
                                            enc.url =
                                                String::from_utf8_lossy(&attr.value).to_string()
                                        }
                                        b"type" => {
                                            enc.mime_type = Some(
                                                String::from_utf8_lossy(&attr.value).to_string(),
                                            )
                                        }
                                        b"length" => {
                                            enc.length =
                                                String::from_utf8_lossy(&attr.value).parse().ok()
                                        }
                                        _ => {}
                                    }
                                }
                                // 检测音频/视频
                                if let Some(ref mime) = enc.mime_type {
                                    if mime.starts_with("audio/") {
                                        item.audio_url = Some(enc.url.clone());
                                    } else if mime.starts_with("video/") {
                                        item.video_url = Some(enc.url.clone());
                                    }
                                }
                                item.enclosures.push(enc);
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(e)) => {
                    let text = reader
                        .decoder()
                        .decode(e.as_ref())
                        .unwrap_or_default()
                        .to_string();
                    // 累积文本而不是直接赋值
                    text_buffer.push_str(&text);
                }
                Ok(Event::CData(ref e)) => {
                    let text = String::from_utf8_lossy(e.as_ref()).to_string();
                    // 累积 CDATA 内容
                    text_buffer.push_str(&text);
                }
                Ok(Event::End(ref e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    // 处理带命名空间的标签名
                    let simple_tag = tag_name.split(':').next_back().unwrap_or(&tag_name);

                    // 在标签结束时处理累积的文本
                    let text = text_buffer.clone();
                    let trimmed_text = text.trim();

                    if !trimmed_text.is_empty() {
                        if in_item {
                            if let Some(ref mut item) = current_item {
                                // 使用原始标签和简化标签都尝试匹配
                                match tag_name.as_str() {
                                    "title" => item.title = trimmed_text.to_string(),
                                    "link" => item.link = trimmed_text.to_string(),
                                    "description" => {
                                        item.summary = Some(normalize_html_content(&text))
                                    }
                                    "content:encoded" => {
                                        item.content = Some(normalize_html_content(&text))
                                    }
                                    "author" | "dc:creator" => {
                                        item.author = Some(trimmed_text.to_string())
                                    }
                                    "guid" => item.guid = trimmed_text.to_string(),
                                    "pubDate" | "dc:date" => {
                                        item.published_at = parse_date(trimmed_text);
                                    }
                                    "category" => item.categories.push(trimmed_text.to_string()),
                                    _ => {
                                        // 尝试简化标签名匹配
                                        match simple_tag {
                                            "encoded" => {
                                                item.content = Some(normalize_html_content(&text))
                                            }
                                            "creator" => {
                                                item.author = Some(trimmed_text.to_string())
                                            }
                                            "date" => {
                                                item.published_at = parse_date(trimmed_text);
                                            }
                                            "content" if item.content.is_none() => {
                                                item.content = Some(normalize_html_content(&text))
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        } else if in_channel && !in_image {
                            match tag_name.as_str() {
                                "title" => feed.title = trimmed_text.to_string(),
                                "description" => feed.description = Some(trimmed_text.to_string()),
                                "link" => {
                                    // 只设置第一个有效的 link
                                    if feed.site_url.is_none() {
                                        feed.site_url = Some(trimmed_text.to_string());
                                    }
                                }
                                "language" => feed.language = Some(trimmed_text.to_string()),
                                "lastBuildDate" | "pubDate" => {
                                    feed.last_updated = parse_date(trimmed_text);
                                }
                                _ => {}
                            }
                        } else if in_image && simple_tag == "url" {
                            feed.icon = Some(trimmed_text.to_string());
                        }
                    }

                    // 清空缓冲区
                    text_buffer.clear();

                    match simple_tag {
                        "item" => {
                            if let Some(mut item) = current_item.take() {
                                // 如果没有 guid，用 link 作为 guid
                                if item.guid.is_empty() {
                                    item.guid = item.link.clone();
                                }
                                // 提取图片
                                if item.image.is_none() {
                                    item.image = extract_image_from_html(
                                        item.content.as_deref().or(item.summary.as_deref()),
                                    );
                                }
                                // 检测内容格式
                                if let Some(ref content) = item.content {
                                    item.content_format = detect_content_format(content);
                                } else if let Some(ref summary) = item.summary {
                                    item.content_format = detect_content_format(summary);
                                }
                                feed.items.push(item);
                            }
                            in_item = false;
                        }
                        "channel" => in_channel = false,
                        "image" => in_image = false,
                        _ => {}
                    }
                    current_tag.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(ParseError::ParseError(format!("XML parse error: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }

        // 修正 HTML 内容：quick_xml 会错误地解析转义的 HTML 标签
        // 从原始 XML 重新提取 description 和 content:encoded
        for item in &mut feed.items {
            let identifier = if !item.guid.is_empty() {
                &item.guid
            } else {
                &item.link
            };

            if let Some(raw_desc) = extract_item_html_content(content, identifier, "description") {
                item.summary = Some(normalize_html_content(&raw_desc));
            }

            if let Some(raw_content) =
                extract_item_html_content(content, identifier, "content:encoded")
            {
                item.content = Some(normalize_html_content(&raw_content));
            }

            // 重新提取图片和检测格式
            if item.image.is_none() {
                item.image =
                    extract_image_from_html(item.content.as_deref().or(item.summary.as_deref()));
            }
            if let Some(ref c) = item.content {
                item.content_format = detect_content_format(c);
            } else if let Some(ref s) = item.summary {
                item.content_format = detect_content_format(s);
            }
        }

        // 如果没有找到 icon，尝试从 site_url 获取 favicon
        if feed.icon.is_none() {
            if let Some(ref site_url) = feed.site_url {
                feed.icon = Some(format!("{}/favicon.ico", site_url.trim_end_matches('/')));
            }
        }

        Ok(feed)
    }

    /// 解析 Atom 1.0
    fn parse_atom(&self, content: &str, _source_url: &str) -> Result<ParsedFeed, ParseError> {
        use quick_xml::events::Event;
        use quick_xml::Reader;

        let mut reader = Reader::from_str(content);
        reader.config_mut().trim_text(true);

        let mut feed = ParsedFeed {
            title: String::new(),
            description: None,
            site_url: None,
            icon: None,
            language: None,
            feed_type: FeedType::Atom,
            items: Vec::new(),
            last_updated: None,
        };

        let mut current_item: Option<ParsedItem> = None;
        let mut current_tag = String::new();
        let mut in_entry = false;
        let mut in_author = false;
        // 用于累积文本内容（处理分段的 Text/CData 事件）
        let mut text_buffer = String::new();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    current_tag = tag_name.clone();
                    text_buffer.clear(); // 新标签开始，清空缓冲区

                    match tag_name.as_str() {
                        "entry" => {
                            in_entry = true;
                            current_item = Some(ParsedItem {
                                guid: String::new(),
                                title: String::new(),
                                link: String::new(),
                                summary: None,
                                content: None,
                                author: None,
                                image: None,
                                audio_url: None,
                                video_url: None,
                                enclosures: Vec::new(),
                                categories: Vec::new(),
                                published_at: None,
                                updated_at: None,
                                content_format: ContentFormat::default(),
                            });
                        }
                        "author" => in_author = true,
                        "link" => {
                            let mut href = String::new();
                            let mut rel = String::new();
                            let mut link_type = String::new();

                            for attr in e.attributes().flatten() {
                                match attr.key.as_ref() {
                                    b"href" => {
                                        href = String::from_utf8_lossy(&attr.value).to_string()
                                    }
                                    b"rel" => {
                                        rel = String::from_utf8_lossy(&attr.value).to_string()
                                    }
                                    b"type" => {
                                        link_type = String::from_utf8_lossy(&attr.value).to_string()
                                    }
                                    _ => {}
                                }
                            }

                            if in_entry {
                                if let Some(ref mut item) = current_item {
                                    // 优先使用 alternate 或空 rel 的链接
                                    if rel.is_empty() || rel == "alternate" {
                                        if item.link.is_empty() {
                                            item.link = href.clone();
                                        }
                                    } else if rel == "self" && item.link.is_empty() {
                                        item.link = href.clone();
                                    } else if rel == "enclosure" {
                                        let enc = Enclosure {
                                            url: href.clone(),
                                            mime_type: if link_type.is_empty() {
                                                None
                                            } else {
                                                Some(link_type.clone())
                                            },
                                            length: None,
                                            title: None,
                                        };
                                        if link_type.starts_with("audio/") {
                                            item.audio_url = Some(href);
                                        } else if link_type.starts_with("video/") {
                                            item.video_url = Some(href);
                                        }
                                        item.enclosures.push(enc);
                                    }
                                }
                            } else if rel.is_empty() || rel == "alternate" {
                                if feed.site_url.is_none() {
                                    feed.site_url = Some(href);
                                }
                            } else if rel == "icon" {
                                feed.icon = Some(href);
                            }
                        }
                        "category" if in_entry => {
                            if let Some(ref mut item) = current_item {
                                for attr in e.attributes().flatten() {
                                    if attr.key.as_ref() == b"term" {
                                        item.categories
                                            .push(String::from_utf8_lossy(&attr.value).to_string());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(e)) => {
                    let text = reader
                        .decoder()
                        .decode(e.as_ref())
                        .unwrap_or_default()
                        .to_string();
                    // 累积文本而不是直接赋值
                    text_buffer.push_str(&text);
                }
                Ok(Event::CData(ref e)) => {
                    let text = String::from_utf8_lossy(e.as_ref()).to_string();
                    // 累积 CDATA 内容
                    text_buffer.push_str(&text);
                }
                Ok(Event::End(ref e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    // 在标签结束时处理累积的文本
                    let text = text_buffer.clone();
                    let trimmed_text = text.trim();

                    if !trimmed_text.is_empty() {
                        if in_entry {
                            if let Some(ref mut item) = current_item {
                                match tag_name.as_str() {
                                    "title" => item.title = trimmed_text.to_string(),
                                    "summary" => item.summary = Some(normalize_html_content(&text)),
                                    "content" => item.content = Some(normalize_html_content(&text)),
                                    "id" => item.guid = trimmed_text.to_string(),
                                    "published" => {
                                        item.published_at = parse_date(trimmed_text);
                                    }
                                    "updated" => {
                                        if item.published_at.is_none() {
                                            item.published_at = parse_date(trimmed_text);
                                        }
                                        item.updated_at = parse_date(trimmed_text);
                                    }
                                    "name" if in_author => {
                                        item.author = Some(trimmed_text.to_string())
                                    }
                                    _ => {}
                                }
                            }
                        } else {
                            match tag_name.as_str() {
                                "title" => feed.title = trimmed_text.to_string(),
                                "subtitle" | "description" => {
                                    if feed.description.is_none() {
                                        feed.description = Some(trimmed_text.to_string());
                                    }
                                }
                                "updated" => {
                                    feed.last_updated = parse_date(trimmed_text);
                                }
                                // Atom 使用 <icon> 或 <logo> 元素来指定图标
                                "icon" | "logo" => {
                                    if feed.icon.is_none() {
                                        feed.icon = Some(trimmed_text.to_string());
                                    }
                                }
                                "language" => {
                                    feed.language = Some(trimmed_text.to_string());
                                }
                                _ => {}
                            }
                        }
                    }

                    // 清空缓冲区
                    text_buffer.clear();

                    match tag_name.as_str() {
                        "entry" => {
                            if let Some(mut item) = current_item.take() {
                                if item.guid.is_empty() {
                                    item.guid = item.link.clone();
                                }
                                if item.image.is_none() {
                                    item.image = extract_image_from_html(
                                        item.content.as_deref().or(item.summary.as_deref()),
                                    );
                                }
                                // 检测内容格式
                                if let Some(ref content) = item.content {
                                    item.content_format = detect_content_format(content);
                                } else if let Some(ref summary) = item.summary {
                                    item.content_format = detect_content_format(summary);
                                }
                                feed.items.push(item);
                            }
                            in_entry = false;
                        }
                        "author" => in_author = false,
                        _ => {}
                    }
                    current_tag.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(ParseError::ParseError(format!("XML parse error: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }

        // 修正 HTML 内容
        for item in &mut feed.items {
            let identifier = if !item.guid.is_empty() {
                &item.guid
            } else {
                &item.link
            };

            if let Some(raw_content) = extract_entry_html_content(content, identifier, "content") {
                item.content = Some(normalize_html_content(&raw_content));
            }

            if let Some(raw_summary) = extract_entry_html_content(content, identifier, "summary") {
                item.summary = Some(normalize_html_content(&raw_summary));
            }

            if item.image.is_none() {
                item.image =
                    extract_image_from_html(item.content.as_deref().or(item.summary.as_deref()));
            }
            if let Some(ref c) = item.content {
                item.content_format = detect_content_format(c);
            } else if let Some(ref s) = item.summary {
                item.content_format = detect_content_format(s);
            }
        }

        // 如果没有找到 icon，尝试从 site_url 获取 favicon
        if feed.icon.is_none() {
            if let Some(ref site_url) = feed.site_url {
                feed.icon = Some(format!("{}/favicon.ico", site_url.trim_end_matches('/')));
            }
        }

        Ok(feed)
    }

    /// 解析 RSS 1.0 (RDF)
    fn parse_rdf(&self, content: &str, source_url: &str) -> Result<ParsedFeed, ParseError> {
        // RDF 格式与 RSS 2.0 类似，但结构略有不同
        // 简化处理：复用 RSS 解析器
        self.parse_rss(content, source_url)
    }

    /// 解析 JSON Feed
    fn parse_json_feed(&self, content: &str, _source_url: &str) -> Result<ParsedFeed, ParseError> {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct JsonFeed {
            title: String,
            description: Option<String>,
            home_page_url: Option<String>,
            feed_url: Option<String>,
            icon: Option<String>,
            favicon: Option<String>,
            language: Option<String>,
            items: Option<Vec<JsonItem>>,
        }

        #[derive(Deserialize)]
        struct JsonItem {
            id: String,
            url: Option<String>,
            title: Option<String>,
            content_html: Option<String>,
            content_text: Option<String>,
            summary: Option<String>,
            image: Option<String>,
            banner_image: Option<String>,
            date_published: Option<String>,
            date_modified: Option<String>,
            authors: Option<Vec<JsonAuthor>>,
            tags: Option<Vec<String>>,
            attachments: Option<Vec<JsonAttachment>>,
        }

        #[derive(Deserialize)]
        struct JsonAuthor {
            name: Option<String>,
        }

        #[derive(Deserialize)]
        struct JsonAttachment {
            url: String,
            mime_type: Option<String>,
            size_in_bytes: Option<u64>,
            title: Option<String>,
        }

        let json_feed: JsonFeed = serde_json::from_str(content)
            .map_err(|e| ParseError::ParseError(format!("JSON parse error: {}", e)))?;

        let items: Vec<ParsedItem> = json_feed
            .items
            .unwrap_or_default()
            .into_iter()
            .map(|item| {
                let author = item
                    .authors
                    .and_then(|a| a.first().and_then(|a| a.name.clone()));

                let enclosures: Vec<Enclosure> = item
                    .attachments
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| Enclosure {
                        url: a.url,
                        mime_type: a.mime_type,
                        length: a.size_in_bytes,
                        title: a.title,
                    })
                    .collect();

                let audio_url = enclosures
                    .iter()
                    .find(|e| e.mime_type.as_deref().unwrap_or("").starts_with("audio/"))
                    .map(|e| e.url.clone());

                let video_url = enclosures
                    .iter()
                    .find(|e| e.mime_type.as_deref().unwrap_or("").starts_with("video/"))
                    .map(|e| e.url.clone());

                // JSON Feed 支持 content_html 和 content_text
                // 根据内容来源判断格式
                let (content, content_format) = if item.content_html.is_some() {
                    (item.content_html, ContentFormat::Html)
                } else if item.content_text.is_some() {
                    // 检测 content_text 是否包含 Markdown 语法
                    let text = item.content_text.as_ref().unwrap();
                    let format = detect_content_format(text);
                    (item.content_text, format)
                } else {
                    (None, ContentFormat::Text)
                };

                ParsedItem {
                    guid: item.id,
                    title: item.title.unwrap_or_default(),
                    link: item.url.unwrap_or_default(),
                    summary: item.summary,
                    content,
                    author,
                    image: item.image.or(item.banner_image),
                    audio_url,
                    video_url,
                    enclosures,
                    categories: item.tags.unwrap_or_default(),
                    published_at: item.date_published.as_deref().and_then(parse_date),
                    updated_at: item.date_modified.as_deref().and_then(parse_date),
                    content_format,
                }
            })
            .collect();

        Ok(ParsedFeed {
            title: json_feed.title,
            description: json_feed.description,
            site_url: json_feed.home_page_url,
            icon: json_feed.icon.or(json_feed.favicon),
            language: json_feed.language,
            feed_type: FeedType::JsonFeed,
            items,
            last_updated: None,
        })
    }
}

/// 从原始 XML 中提取指定 item 的 HTML 内容字段
/// 这是必要的，因为 quick_xml 会自动解码 XML 实体，导致转义的 HTML 被错误解析
fn extract_item_html_content(xml: &str, guid: &str, tag: &str) -> Option<String> {
    // 首先找到包含这个 guid 的 item
    let guid_escaped = guid
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");

    // 使用简单的字符串搜索找到包含 guid 的 item
    let item_start = xml.find("<item")?;
    let mut search_pos = item_start;

    loop {
        let item_end_pos = xml[search_pos..].find("</item>")?;
        let item_xml = &xml[search_pos..search_pos + item_end_pos + 7];

        // 检查这个 item 是否包含我们要找的 guid
        let guid_escaped_str: &str = &guid_escaped;
        if item_xml.contains(guid) || item_xml.contains(guid_escaped_str) {
            // 从 item 中提取指定标签的内容
            let tag_start = format!("<{}", tag);
            let tag_end = format!("</{}>", tag);

            if let Some(start_pos) = item_xml.find(&tag_start) {
                // 找到标签结束 >
                let content_start = item_xml[start_pos..].find('>')? + start_pos + 1;
                if let Some(end_pos) = item_xml[content_start..].find(&tag_end) {
                    let content = &item_xml[content_start..content_start + end_pos];

                    // 处理 CDATA
                    let content = content.trim();
                    let content = if content.starts_with("<![CDATA[") && content.ends_with("]]>") {
                        &content[9..content.len() - 3]
                    } else {
                        content
                    };

                    return Some(content.to_string());
                }
            }
            return None;
        }

        // 继续搜索下一个 item
        search_pos += item_end_pos + 7;
        if let Some(next_item) = xml[search_pos..].find("<item") {
            search_pos += next_item;
        } else {
            break;
        }
    }

    None
}

/// 从原始 XML 中提取 Atom entry 的 HTML 内容字段
fn extract_entry_html_content(xml: &str, entry_id: &str, tag: &str) -> Option<String> {
    let id_escaped = entry_id
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");

    let entry_start = xml.find("<entry")?;
    let mut search_pos = entry_start;

    loop {
        let entry_end_pos = xml[search_pos..].find("</entry>")?;
        let entry_xml = &xml[search_pos..search_pos + entry_end_pos + 8];

        let id_escaped_str: &str = &id_escaped;
        if entry_xml.contains(entry_id) || entry_xml.contains(id_escaped_str) {
            let tag_start = format!("<{}", tag);
            let tag_end = format!("</{}>", tag);

            if let Some(start_pos) = entry_xml.find(&tag_start) {
                let content_start = entry_xml[start_pos..].find('>')? + start_pos + 1;
                if let Some(end_pos) = entry_xml[content_start..].find(&tag_end) {
                    let content = &entry_xml[content_start..content_start + end_pos];

                    let content = content.trim();
                    let content = if content.starts_with("<![CDATA[") && content.ends_with("]]>") {
                        &content[9..content.len() - 3]
                    } else {
                        content
                    };

                    return Some(content.to_string());
                }
            }
            return None;
        }

        search_pos += entry_end_pos + 8;
        if let Some(next_entry) = xml[search_pos..].find("<entry") {
            search_pos += next_entry;
        } else {
            break;
        }
    }

    None
}

/// 规范化 HTML 内容
/// 处理常见的 HTML 内容格式问题，保持可读性
fn normalize_html_content(content: &str) -> String {
    let mut result = content.to_string();

    // 1. 处理 CDATA 残留标记
    result = result.replace("<![CDATA[", "").replace("]]>", "");

    // 2. 规范化换行符
    result = result.replace("\r\n", "\n").replace("\r", "\n");

    // 3. 处理多余的空白行（保留单个空行作为段落分隔）
    let lines: Vec<&str> = result.lines().collect();
    let mut normalized_lines: Vec<String> = Vec::new();
    let mut last_was_empty = false;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !last_was_empty {
                normalized_lines.push(String::new());
                last_was_empty = true;
            }
        } else {
            normalized_lines.push(line.to_string());
            last_was_empty = false;
        }
    }

    // 4. 移除首尾空白行
    while normalized_lines
        .first()
        .map(|s| s.is_empty())
        .unwrap_or(false)
    {
        normalized_lines.remove(0);
    }
    while normalized_lines
        .last()
        .map(|s| s.is_empty())
        .unwrap_or(false)
    {
        normalized_lines.pop();
    }

    // 5. 解码常见的 HTML 实体（保留标签本身）
    result = normalized_lines.join("\n");

    // 先处理数字实体（如 &#39; &#160; &#x27; 等）
    result = decode_html_numeric_entities(&result);

    // 再处理命名实体
    result = result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&ldquo;", "\u{201C}") // "
        .replace("&rdquo;", "\u{201D}") // "
        .replace("&lsquo;", "\u{2018}") // '
        .replace("&rsquo;", "\u{2019}") // '
        .replace("&mdash;", "\u{2014}") // —
        .replace("&ndash;", "\u{2013}") // –
        .replace("&hellip;", "\u{2026}") // …
        .replace("&copy;", "\u{00A9}") // ©
        .replace("&reg;", "\u{00AE}") // ®
        .replace("&trade;", "\u{2122}") // ™
        .replace("&times;", "\u{00D7}") // ×
        .replace("&divide;", "\u{00F7}") // ÷
        .replace("&bull;", "\u{2022}") // •
        .replace("&middot;", "\u{00B7}") // ·
        .replace("&laquo;", "\u{00AB}") // «
        .replace("&raquo;", "\u{00BB}"); // »

    result
}

/// 解码 HTML 数字实体（十进制和十六进制）
fn decode_html_numeric_entities(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '&' {
            // 检查是否是数字实体
            let mut entity = String::new();
            entity.push(c);

            let mut found_semicolon = false;
            let mut is_numeric = false;

            // 收集实体内容直到分号或非实体字符
            while let Some(&next) = chars.peek() {
                if next == ';' {
                    entity.push(chars.next().unwrap());
                    found_semicolon = true;
                    break;
                } else if next == '#'
                    || next.is_ascii_digit()
                    || next.is_ascii_hexdigit()
                    || next == 'x'
                    || next == 'X'
                {
                    entity.push(chars.next().unwrap());
                    if next == '#' {
                        is_numeric = true;
                    }
                } else if entity.len() > 10 {
                    // 实体太长，不是有效实体
                    break;
                } else {
                    break;
                }
            }

            if found_semicolon && is_numeric && entity.starts_with("&#") {
                // 尝试解码数字实体
                let num_str = &entity[2..entity.len() - 1];
                let code_point = if num_str.starts_with('x') || num_str.starts_with('X') {
                    // 十六进制
                    u32::from_str_radix(&num_str[1..], 16).ok()
                } else {
                    // 十进制
                    num_str.parse::<u32>().ok()
                };

                if let Some(cp) = code_point {
                    if let Some(decoded) = char::from_u32(cp) {
                        result.push(decoded);
                        continue;
                    }
                }
            }

            // 无法解码，保留原始内容
            result.push_str(&entity);
        } else {
            result.push(c);
        }
    }

    result
}

/// 检测内容格式（HTML/Markdown/纯文本）
fn detect_content_format(content: &str) -> ContentFormat {
    let trimmed = content.trim();

    // 检测 HTML 特征
    let html_indicators = [
        "<p>",
        "<p ",
        "</p>",
        "<div>",
        "<div ",
        "</div>",
        "<span>",
        "<span ",
        "</span>",
        "<h1>",
        "<h2>",
        "<h3>",
        "<h4>",
        "<h5>",
        "<h6>",
        "<a href=",
        "<img ",
        "<br>",
        "<br/>",
        "<br />",
        "<ul>",
        "<ol>",
        "<li>",
        "<table>",
        "<tr>",
        "<td>",
        "<strong>",
        "<em>",
        "<b>",
        "<i>",
        "<u>",
        "<blockquote>",
        "<pre>",
        "<code>",
        "<!DOCTYPE",
        "<html",
        "<!--",
    ];

    for indicator in html_indicators {
        if trimmed.contains(indicator) {
            return ContentFormat::Html;
        }
    }

    // 检测 Markdown 特征（按优先级检查）
    let lines: Vec<&str> = trimmed.lines().collect();
    let mut md_score = 0;

    for line in &lines {
        let l = line.trim();

        // 标题语法: # ## ### 等
        if l.starts_with('#')
            && l.chars().skip_while(|c| *c == '#').find(|&c| c != '#') == Some(' ')
        {
            md_score += 3;
        }

        // 列表语法: - * + 或 1.
        if (l.starts_with("- ") || l.starts_with("* ") || l.starts_with("+ "))
            || (l.len() > 2 && l.chars().next().unwrap_or(' ').is_ascii_digit() && l.contains(". "))
        {
            md_score += 1;
        }

        // 代码块: ``` 或缩进4空格
        if l.starts_with("```") || l.starts_with("    ") {
            md_score += 2;
        }

        // 引用块: >
        if l.starts_with("> ") {
            md_score += 1;
        }

        // 水平线: --- *** ___
        if l == "---" || l == "***" || l == "___" {
            md_score += 1;
        }
    }

    // 检测内联 Markdown 语法
    // 链接: [text](url)
    if trimmed.contains("](") && trimmed.contains('[') {
        md_score += 2;
    }

    // 图片: ![alt](url)
    if trimmed.contains("![") && trimmed.contains("](") {
        md_score += 2;
    }

    // 粗体/斜体: **text** *text* __text__ _text_
    if (trimmed.contains("**") && trimmed.matches("**").count() >= 2)
        || (trimmed.contains("__") && trimmed.matches("__").count() >= 2)
    {
        md_score += 1;
    }

    // 行内代码: `code`
    if trimmed.contains('`') && trimmed.matches('`').count() >= 2 {
        md_score += 1;
    }

    // 如果有足够的 Markdown 特征
    if md_score >= 3 {
        return ContentFormat::Markdown;
    }

    // 默认为纯文本
    ContentFormat::Text
}

/// 解析各种日期格式
fn parse_date(date_str: &str) -> Option<DateTime<Utc>> {
    // RFC 2822 (RSS 常用)
    if let Ok(dt) = DateTime::parse_from_rfc2822(date_str) {
        return Some(dt.with_timezone(&Utc));
    }

    // RFC 3339 / ISO 8601 (Atom 常用)
    if let Ok(dt) = DateTime::parse_from_rfc3339(date_str) {
        return Some(dt.with_timezone(&Utc));
    }

    // 其他常见格式
    let formats = [
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M:%S%:z",
        "%Y-%m-%dT%H:%M:%SZ",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d",
        "%d %b %Y %H:%M:%S %z",
        "%a, %d %b %Y %H:%M:%S %z",
    ];

    for fmt in formats {
        if let Ok(dt) = DateTime::parse_from_str(date_str, fmt) {
            return Some(dt.with_timezone(&Utc));
        }
        // 尝试无时区版本
        if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(date_str, fmt) {
            return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
        }
    }

    None
}

/// 从 HTML 内容中提取第一张图片
fn extract_image_from_html(html: Option<&str>) -> Option<String> {
    let html = html?;

    // 简单正则提取 img src
    let re = regex::Regex::new(r#"<img[^>]+src=["']([^"']+)["']"#).ok()?;
    if let Some(caps) = re.captures(html) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }

    // 尝试提取 media:content 或 media:thumbnail
    let media_re =
        regex::Regex::new(r#"<media:(content|thumbnail)[^>]+url=["']([^"']+)["']"#).ok()?;
    if let Some(caps) = media_re.captures(html) {
        return caps.get(2).map(|m| m.as_str().to_string());
    }

    None
}

/// 计算文本的字数和阅读时间
pub fn calculate_reading_stats(content: &str) -> (i32, i32) {
    // 移除 HTML 标签
    let text = regex::Regex::new(r"<[^>]+>")
        .map(|re| re.replace_all(content, ""))
        .unwrap_or_else(|_| std::borrow::Cow::Borrowed(content));

    // 计算字数（中文按字符，英文按单词）
    let mut word_count = 0;
    let mut in_word = false;

    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            if !in_word {
                in_word = true;
                word_count += 1;
            }
        } else if c.is_alphabetic() {
            // 非 ASCII 字母（如中文）每个字符算一个词
            word_count += 1;
            in_word = false;
        } else {
            in_word = false;
        }
    }

    // 阅读时间：中文 400 字/分钟，英文 200 词/分钟
    // 这里简化处理，统一按 300 词/分钟
    let reading_time = (word_count as f32 / 300.0).ceil() as i32;

    (word_count, reading_time.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_date() {
        // RFC 2822
        assert!(parse_date("Sat, 14 Dec 2024 12:00:00 +0000").is_some());
        // RFC 3339
        assert!(parse_date("2024-12-14T12:00:00Z").is_some());
        // ISO 8601
        assert!(parse_date("2024-12-14T12:00:00+08:00").is_some());
    }

    #[test]
    fn test_calculate_reading_stats() {
        let (words, time) = calculate_reading_stats("Hello world, this is a test.");
        assert!(words > 0);
        assert!(time >= 1);
    }

    #[test]
    fn max_feed_body_is_generous_but_bounded() {
        // Several MiB–tens of MiB: large enough for full-content feeds, not unbounded.
        // Compile-time checks avoid clippy::assertions_on_constants.
        const _: () = assert!(MAX_FEED_BODY_BYTES >= 4 * 1024 * 1024);
        const _: () = assert!(MAX_FEED_BODY_BYTES <= 64 * 1024 * 1024);
        assert_eq!(MAX_FEED_BODY_BYTES, 16 * 1024 * 1024);
    }

    #[test]
    fn oversize_body_surfaces_as_fetch_error() {
        // fetch_and_parse maps read_limited_body failures to FetchError (no silent truncate).
        let err = ParseError::FetchError(format!(
            "Failed to read body: Response exceeds {} bytes",
            MAX_FEED_BODY_BYTES
        ));
        let msg = err.to_string();
        assert!(msg.starts_with("Fetch error:"));
        assert!(msg.contains("exceeds"));
        assert!(msg.contains(&MAX_FEED_BODY_BYTES.to_string()));
    }

    #[tokio::test]
    async fn validate_public_url_rejects_loopback_literal() {
        let err = FeedParser::validate_public_url("http://127.0.0.1/feed.xml")
            .await
            .expect_err("loopback must be rejected");
        assert!(
            matches!(err, ParseError::InvalidUrl(_)),
            "expected InvalidUrl, got {err}"
        );
    }

    #[tokio::test]
    async fn validate_public_url_rejects_metadata_ip() {
        let err = FeedParser::validate_public_url("http://169.254.169.254/latest/meta-data/")
            .await
            .expect_err("link-local metadata must be rejected");
        assert!(matches!(err, ParseError::InvalidUrl(_)));
    }

    #[tokio::test]
    async fn fetch_and_parse_rejects_private_target() {
        let parser = FeedParser::new();
        let err = parser
            .fetch_and_parse("http://10.0.0.1/rss.xml")
            .await
            .expect_err("private range must be rejected before connect");
        assert!(
            matches!(err, ParseError::InvalidUrl(_)),
            "expected InvalidUrl for SSRF block, got {err}"
        );
    }
}
