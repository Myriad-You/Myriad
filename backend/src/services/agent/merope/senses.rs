//! Her senses for the world beyond the site: searching, reading a page,
//! reading a video by its subtitles.
//!
//! Each sense is read-only and reaches only public addresses (DNS pinned,
//! every redirect hop checked). A page is read only where its robots.txt
//! lets any reader in. Everything that comes back is untrusted text: she
//! takes it in, never follows it. Which senses work right now is checked,
//! not assumed (`available`), and she is only offered those.
//!
//! Searching goes through the site's search provider when one is set up,
//! and otherwise through Wikipedia's public search API (in the language the
//! question is asked in), so she can always look something up.
//!
//! A video is read by its subtitles, fetched the way YouTube's own player
//! fetches them: the uploader's subtitles in the spoken language first, then
//! the automatic captions, then subtitles in English, Japanese or Chinese.
//! Nothing needs installing for that. If someone has put `yt-dlp` on the
//! host (`YT_DLP_PATH`, or on the PATH), it is tried when that way fails; it
//! is never required.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

const AGENT_NAME: &str = "MyriadPersona/1.0";
/// YouTube serves its player only to a browser.
const BROWSER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const PAGE_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_PAGE_BYTES: usize = 3 * 1024 * 1024;
const MAX_PAGE_CHARS: usize = 6_000;
const MAX_TRANSCRIPT_CHARS: usize = 8_000;
const VIDEO_TIMEOUT: Duration = Duration::from_secs(90);

/// Who she is when she reads: named, with the site to reach about her, as
/// Wikimedia and polite crawling ask of automated readers.
async fn user_agent() -> String {
    let site = crate::oauth_url_builder::SiteConfig::get_base_url().await;
    format!("{AGENT_NAME} (+{site}; reading on her own)")
}
const SEARCH_RESULTS: usize = 6;
/// robots.txt answers are kept this long per site.
const ROBOTS_FOR: Duration = Duration::from_secs(24 * 60 * 60);

/// What her senses can do right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Senses {
    pub search: bool,
    /// Searching goes to Wikipedia (no search provider set up), which
    /// finds articles by their subject, not by a string of keywords.
    pub search_is_wikipedia: bool,
    pub read: bool,
    pub video: bool,
}

pub async fn available() -> Senses {
    Senses {
        // Wikipedia is always there to search.
        search: true,
        search_is_wikipedia: !crate::services::agent::web_search::available().await,
        read: true,
        // Subtitles are fetched directly; yt-dlp is only a fallback.
        video: true,
    }
}

/// One search result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Search the web with the site's provider, or Wikipedia without one (or
/// when it fails).
pub async fn search(query: &str) -> Result<Vec<Hit>, String> {
    if crate::services::agent::web_search::available().await {
        match search_web(query).await {
            Ok(hits) if !hits.is_empty() => return Ok(hits),
            _ => {}
        }
    }
    search_wikipedia(query).await
}

/// The Wikipedia a question is asked for: Japanese if it has kana, Chinese
/// if it is in Han characters, English otherwise.
fn wikipedia_language(query: &str) -> &'static str {
    let kana = query.chars().any(|c| matches!(c, '\u{3040}'..='\u{30ff}'));
    let han = query.chars().any(|c| matches!(c, '\u{4e00}'..='\u{9fff}'));
    if kana {
        "ja"
    } else if han {
        "zh"
    } else {
        "en"
    }
}

fn without_tags(text: &str) -> String {
    let mut plain = String::new();
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => plain.push(c),
            _ => {}
        }
    }
    plain.replace("&quot;", "\"").replace("&amp;", "&")
}

fn wikipedia_hits(language: &str, payload: &Value) -> Vec<Hit> {
    payload
        .get("pages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(SEARCH_RESULTS)
        .filter_map(|page| {
            let key = page.get("key")?.as_str()?;
            let title = page.get("title")?.as_str()?.to_string();
            let mut url =
                url::Url::parse(&format!("https://{language}.wikipedia.org/wiki/")).ok()?;
            url.path_segments_mut().ok()?.pop_if_empty().push(key);
            let excerpt = page.get("excerpt").and_then(Value::as_str).unwrap_or("");
            let description = page
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            Some(Hit {
                title,
                url: url.to_string(),
                snippet: format!("{description} {}", without_tags(excerpt))
                    .trim()
                    .chars()
                    .take(300)
                    .collect(),
            })
        })
        .collect()
}

async fn search_wikipedia(query: &str) -> Result<Vec<Hit>, String> {
    let language = wikipedia_language(query);
    let mut url = url::Url::parse(&format!(
        "https://api.wikimedia.org/core/v1/wikipedia/{language}/search/page"
    ))
    .map_err(|_| "search address".to_string())?;
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("limit", &SEARCH_RESULTS.to_string());
    let fetched = crate::services::outbound_security::get_public_following_redirects(
        url.as_str(),
        PAGE_TIMEOUT,
        Some(&user_agent().await),
    )
    .await
    .map_err(|_| "could not reach Wikipedia".to_string())?;
    if !fetched.response.status().is_success() {
        return Err(format!("Wikipedia answered {}", fetched.response.status()));
    }
    let body = crate::services::outbound_security::read_limited_body(fetched.response, 512 * 1024)
        .await
        .map_err(|_| "Wikipedia's answer was too large".to_string())?;
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| "Wikipedia's answer was unreadable".to_string())?;
    Ok(wikipedia_hits(language, &payload))
}

async fn search_web(query: &str) -> Result<Vec<Hit>, String> {
    let payload = crate::services::agent::web_search::execute_from_value(
        &serde_json::json!({ "query": query, "maxResults": SEARCH_RESULTS }),
    )
    .await?;
    let field = |result: &Value, key: &str| {
        result
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    Ok(payload
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(SEARCH_RESULTS)
        .map(|result| Hit {
            title: field(result, "name"),
            url: field(result, "url"),
            snippet: field(result, "description").chars().take(300).collect(),
        })
        .filter(|hit| hit.url.starts_with("http"))
        .collect())
}

/// What she read or watched: where, and the text of it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Taken {
    pub url: String,
    pub title: String,
    pub text: String,
}

// --- robots.txt ------------------------------------------------------------------

static ROBOTS: LazyLock<Mutex<HashMap<String, (Instant, Vec<String>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// User agents of AI readers and crawlers: a site that closes itself to
/// them closes itself to her.
const AI_AGENTS: [&str; 14] = [
    "claude-user",
    "claude-web",
    "claudebot",
    "anthropic-ai",
    "chatgpt-user",
    "gptbot",
    "oai-searchbot",
    "google-extended",
    "perplexity-user",
    "perplexitybot",
    "mistralai-user",
    "ccbot",
    "bytespider",
    "meta-externalagent",
];

/// The paths robots.txt closes to her: the group for every reader
/// (`User-agent: *`) and any group naming an AI agent.
fn disallowed_for_her(robots: &str) -> Vec<String> {
    let mut rules = Vec::new();
    let mut applies = false;
    let mut last_was_agent = false;
    for line in robots.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
        match key.as_str() {
            "user-agent" => {
                // Consecutive user-agent lines share one group.
                let agent = value.to_ascii_lowercase();
                let her = agent == "*" || AI_AGENTS.contains(&agent.as_str());
                applies = if last_was_agent { applies || her } else { her };
                last_was_agent = true;
            }
            "disallow" => {
                last_was_agent = false;
                if applies && !value.is_empty() {
                    rules.push(value.to_string());
                }
            }
            _ => last_was_agent = false,
        }
    }
    rules
}

/// Whether a robots.txt path pattern (`*` for anything, `$` for the end)
/// matches the start of the path.
fn pattern_matches(pattern: &str, path: &str) -> bool {
    let (pattern, anchored) = match pattern.strip_suffix('$') {
        Some(pattern) => (pattern, true),
        None => (pattern, false),
    };
    let pieces: Vec<&str> = pattern.split('*').collect();
    let Some(mut rest) = path.strip_prefix(pieces[0]) else {
        return false;
    };
    for (index, piece) in pieces.iter().enumerate().skip(1) {
        if index == pieces.len() - 1 && anchored {
            return rest.ends_with(piece);
        }
        match rest.find(piece) {
            Some(at) => rest = &rest[at + piece.len()..],
            None => return false,
        }
    }
    !anchored || rest.is_empty()
}

fn path_is_closed(path: &str, rules: &[String]) -> bool {
    rules.iter().any(|rule| pattern_matches(rule, path))
}

/// Whether the site lets any reader at this address. If robots.txt cannot
/// be read, it does.
async fn allowed(url: &url::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let origin = format!("{}://{host}", url.scheme());
    let cached = ROBOTS.lock().ok().and_then(|robots| {
        robots
            .get(&origin)
            .filter(|(at, _)| at.elapsed() < ROBOTS_FOR)
            .map(|(_, rules)| rules.clone())
    });
    let rules = match cached {
        Some(rules) => rules,
        None => {
            let rules = match crate::services::outbound_security::get_public_following_redirects(
                &format!("{origin}/robots.txt"),
                Duration::from_secs(10),
                Some(&user_agent().await),
            )
            .await
            {
                Ok(fetched) if fetched.response.status().is_success() => {
                    let body = crate::services::outbound_security::read_limited_body(
                        fetched.response,
                        256 * 1024,
                    )
                    .await
                    .unwrap_or_default();
                    disallowed_for_her(&String::from_utf8_lossy(&body))
                }
                _ => Vec::new(),
            };
            if let Ok(mut robots) = ROBOTS.lock() {
                robots.insert(origin, (Instant::now(), rules.clone()));
            }
            rules
        }
    };
    let mut path = url.path().to_string();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    !path_is_closed(&path, &rules)
}

// --- a page ------------------------------------------------------------------------

/// A page's title, and its readable text in blocks (paragraphs, list items,
/// headings) of its article or main part if it has one.
fn page_text(html: &str) -> (String, Vec<String>) {
    let document = scraper::Html::parse_document(html);
    let title = scraper::Selector::parse("title")
        .ok()
        .and_then(|selector| document.select(&selector).next())
        .map(|element| element.text().collect::<String>().trim().to_string())
        .unwrap_or_default();
    let text_of = |selector: &str| -> String {
        let Ok(selector) = scraper::Selector::parse(selector) else {
            return String::new();
        };
        document
            .select(&selector)
            .flat_map(|element| {
                element.descendants().filter_map(|node| {
                    let scraper::node::Node::Text(text) = node.value() else {
                        return None;
                    };
                    let skipped = node
                        .parent()
                        .and_then(|parent| parent.value().as_element())
                        .is_some_and(|parent| {
                            myriad_agent_rules::scrape_should_skip_tag(parent.name())
                        });
                    let text = text.trim();
                    (!skipped && !text.is_empty()).then(|| text.to_string())
                })
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    // The article itself where the page marks it (Wikipedia keeps it in
    // #mw-content-text), then the main part, then the whole page.
    let mut container = "body";
    let mut text = String::new();
    for selector in ["#mw-content-text", "article", "[role=main]", "main", "body"] {
        text = text_of(selector);
        if text.chars().count() >= 200 {
            container = selector;
            break;
        }
    }
    // Its paragraphs, list items and headings, one block each.
    let selector = [" p", " li", " h2", " h3", " blockquote", " dd"]
        .iter()
        .map(|inner| format!("{container}{inner}"))
        .collect::<Vec<_>>()
        .join(",");
    let blocks: Vec<String> = scraper::Selector::parse(&selector)
        .ok()
        .map(|selector| {
            document
                .select(&selector)
                .map(|element| {
                    element
                        .text()
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|block| !block.is_empty())
                .collect()
        })
        .unwrap_or_default();
    (
        title,
        if blocks.is_empty() {
            vec![text]
        } else {
            blocks
        },
    )
}

/// The words a passage about `about` would share with it: longer words for
/// alphabetic text, pairs of characters for Chinese and Japanese.
fn terms(about: &str) -> Vec<String> {
    let lower = about.to_lowercase();
    let mut terms: Vec<String> = lower
        .split(|c: char| !c.is_alphanumeric() || !c.is_ascii())
        .filter(|word| word.chars().count() >= 4)
        .map(str::to_string)
        .collect();
    let wide: Vec<char> = lower.chars().collect();
    for pair in wide.windows(2) {
        if pair.iter().all(|c| !c.is_ascii() && c.is_alphanumeric()) {
            terms.push(pair.iter().collect());
        }
    }
    terms.sort();
    terms.dedup();
    terms
}

/// What to read of a long page: the opening, then the passages that touch
/// what she is looking for, in the page's order, up to what she reads.
fn focused(blocks: &[String], about: Option<&str>, most: usize) -> String {
    const OPENING: usize = 2;
    let total: usize = blocks.iter().map(|block| block.chars().count() + 1).sum();
    let keep: Vec<bool> = match about.map(terms).filter(|terms| !terms.is_empty()) {
        Some(terms) if total > most => {
            let score = |block: &String| {
                let lower = block.to_lowercase();
                terms
                    .iter()
                    .filter(|term| lower.contains(term.as_str()))
                    .count()
            };
            let mut ranked: Vec<(usize, usize)> = blocks
                .iter()
                .enumerate()
                .skip(OPENING)
                .map(|(index, block)| (score(block), index))
                .filter(|(score, _)| *score > 0)
                .collect();
            ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            let mut keep = vec![false; blocks.len()];
            let mut used = 0;
            for index in
                (0..blocks.len().min(OPENING)).chain(ranked.into_iter().map(|(_, index)| index))
            {
                let size = blocks[index].chars().count() + 1;
                if used + size > most {
                    continue;
                }
                keep[index] = true;
                used += size;
            }
            keep
        }
        _ => vec![true; blocks.len()],
    };
    let text = blocks
        .iter()
        .zip(keep)
        .filter(|(_, keep)| *keep)
        .map(|(block, _)| block.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    myriad_agent_rules::compress_and_truncate_text(&text, most).0
}

/// A page, read with an eye to `about` (what she is finding out) when it is
/// too long to read whole.
pub async fn read(address: &str, about: Option<&str>) -> Result<Taken, String> {
    let url = url::Url::parse(address).map_err(|_| "not an address".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("not a web page".into());
    }
    if !allowed(&url).await {
        return Err("the site asks readers not to read this page".into());
    }
    let fetched = crate::services::outbound_security::get_public_following_redirects(
        address,
        PAGE_TIMEOUT,
        Some(&user_agent().await),
    )
    .await
    .map_err(|_| "could not reach the page".to_string())?;
    let status = fetched.response.status();
    if !status.is_success() {
        return Err(format!("the page answered {status}"));
    }
    let bytes =
        crate::services::outbound_security::read_limited_body(fetched.response, MAX_PAGE_BYTES)
            .await
            .map_err(|_| "the page was too large to read".to_string())?;
    let (title, blocks) = page_text(&String::from_utf8_lossy(&bytes));
    let text = focused(&blocks, about, MAX_PAGE_CHARS);
    if text.trim().is_empty() {
        return Err("nothing readable on the page".into());
    }
    Ok(Taken {
        url: fetched.url.to_string(),
        title,
        text,
    })
}

// --- a word or a meme ----------------------------------------------------------------

/// Where a word, slang or meme may have an entry, by its script: Chinese
/// net slang in 萌娘百科, Japanese in ニコニコ大百科, English in Know Your
/// Meme; the others after, in case.
fn meme_entries(term: &str) -> Vec<String> {
    let term = term.trim();
    if term.is_empty() || term.chars().count() > 40 {
        return Vec::new();
    }
    let encoded = |text: &str| {
        url::form_urlencoded::byte_serialize(text.as_bytes())
            .collect::<String>()
            .replace('+', "%20")
    };
    let slug: String = term
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let moegirl = format!("https://zh.moegirl.org.cn/{}", encoded(term));
    let nico = format!("https://dic.nicovideo.jp/a/{}", encoded(term));
    let kym = (!slug.is_empty()).then(|| format!("https://knowyourmeme.com/memes/{slug}"));
    let ordered = match wikipedia_language(term) {
        "ja" => vec![Some(nico), Some(moegirl), kym],
        "zh" => vec![Some(moegirl), Some(nico), kym],
        _ => vec![kym, Some(moegirl), Some(nico)],
    };
    ordered.into_iter().flatten().collect()
}

/// A word, slang or meme, looked up in the dictionaries people keep for
/// them. Only an entry of that name counts; nothing is guessed.
pub async fn define(term: &str) -> Result<Taken, String> {
    for entry in meme_entries(term) {
        if let Ok(page) = read(&entry, Some(term)).await {
            return Ok(page);
        }
    }
    Err("no entry for it in the slang and meme dictionaries".into())
}

// --- a video -----------------------------------------------------------------------

fn yt_dlp() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("YT_DLP_PATH") {
        let path = std::path::PathBuf::from(path);
        return path.is_file().then_some(path);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("yt-dlp"))
            .find(|path| path.is_file())
    })
}

/// A video address she may watch: YouTube only, for now.
fn video_address(address: &str) -> Option<String> {
    let url = url::Url::parse(address).ok()?;
    let host = url
        .host_str()?
        .trim_start_matches("www.")
        .trim_start_matches("m.");
    let id = match host {
        "youtube.com" => url
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, id)| id.to_string())
            .or_else(|| {
                url.path()
                    .strip_prefix("/shorts/")
                    .map(|id| id.trim_end_matches('/').to_string())
            })?,
        "youtu.be" => url.path().trim_start_matches('/').to_string(),
        _ => return None,
    };
    (id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    .then(|| format!("https://www.youtube.com/watch?v={id}"))
}

/// A WebVTT caption file as plain running text: without timings, tags, or
/// the lines rolling captions repeat.
fn vtt_text(vtt: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in vtt.lines() {
        let line = line.trim();
        if line.is_empty()
            || line == "WEBVTT"
            || line.contains("-->")
            || line.starts_with("Kind:")
            || line.starts_with("Language:")
            || line.starts_with("NOTE")
        {
            continue;
        }
        let mut plain = String::new();
        let mut in_tag = false;
        for c in line.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => plain.push(c),
                _ => {}
            }
        }
        let plain = plain.replace("&nbsp;", " ").replace("&amp;", "&");
        let plain = plain.trim();
        if plain.is_empty() || lines.last().is_some_and(|last| last == plain) {
            continue;
        }
        lines.push(plain.to_string());
    }
    lines.join(" ")
}

async fn subtitles(tool: &std::path::Path, address: &str, langs: &[&str]) -> Option<String> {
    let dir = std::env::temp_dir().join(format!("myriad-subs-{}", uuid::Uuid::new_v4().simple()));
    tokio::fs::create_dir_all(&dir).await.ok()?;
    let mut command = tokio::process::Command::new(tool);
    command
        .args([
            "--skip-download",
            "--no-warnings",
            "--no-playlist",
            "--sub-format",
            "vtt",
        ])
        .args(langs)
        .args(["-o", "%(id)s.%(ext)s", "--paths"])
        .arg(&dir)
        .arg(address)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let ran = tokio::time::timeout(VIDEO_TIMEOUT, command.status()).await;
    let mut text = None;
    if matches!(ran, Ok(Ok(_))) {
        if let Ok(mut entries) = tokio::fs::read_dir(&dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry.path().extension().is_some_and(|ext| ext == "vtt") {
                    if let Ok(vtt) = tokio::fs::read_to_string(entry.path()).await {
                        let words = vtt_text(&vtt);
                        if !words.is_empty() {
                            text = Some(words);
                            break;
                        }
                    }
                }
            }
        }
    }
    let _ = tokio::fs::remove_dir_all(&dir).await;
    text
}

async fn video_title(tool: &std::path::Path, address: &str) -> String {
    let mut command = tokio::process::Command::new(tool);
    command
        .args([
            "--skip-download",
            "--no-warnings",
            "--no-playlist",
            "--print",
            "title",
        ])
        .arg(address)
        .kill_on_drop(true);
    match tokio::time::timeout(Duration::from_secs(40), command.output()).await {
        Ok(Ok(output)) => String::from_utf8_lossy(&output.stdout)
            .trim()
            .chars()
            .take(200)
            .collect(),
        _ => String::new(),
    }
}

/// The subtitle track to read: the uploader's own in the spoken language
/// (the language of the automatic captions), else the automatic captions,
/// else the uploader's in a language she reads.
fn pick_track(tracks: &[Value]) -> Option<&Value> {
    fn language(track: &Value) -> &str {
        track
            .get("languageCode")
            .and_then(Value::as_str)
            .unwrap_or("")
    }
    let automatic = |track: &Value| track.get("kind").and_then(Value::as_str) == Some("asr");
    let spoken = tracks.iter().find(|track| automatic(track)).map(language);
    tracks
        .iter()
        .find(|track| !automatic(track) && spoken.is_some_and(|spoken| language(track) == spoken))
        .or_else(|| tracks.iter().find(|track| automatic(track)))
        .or_else(|| {
            ["en", "ja", "zh"].into_iter().find_map(|wanted| {
                tracks
                    .iter()
                    .find(|track| language(track).split('-').next() == Some(wanted))
            })
        })
}

/// YouTube's timed text as plain running text.
fn timedtext_text(xml: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<p ") {
        rest = &rest[start..];
        let Some(open_end) = rest.find('>') else {
            break;
        };
        let Some(close) = rest.find("</p>") else {
            break;
        };
        let inner = &rest[open_end + 1..close.max(open_end + 1)];
        let mut plain = String::new();
        let mut in_tag = false;
        for c in inner.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => plain.push(c),
                _ => {}
            }
        }
        let plain = plain
            .replace("&amp;", "&")
            .replace("&#39;", "'")
            .replace("&quot;", "\"")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !plain.is_empty() && lines.last() != Some(&plain) {
            lines.push(plain);
        }
        rest = &rest[close + 4..];
    }
    lines.join(" ")
}

async fn get_text(address: &str) -> Result<String, String> {
    let fetched = crate::services::outbound_security::get_public_following_redirects(
        address,
        PAGE_TIMEOUT,
        Some(BROWSER_AGENT),
    )
    .await
    .map_err(|_| "could not reach the video".to_string())?;
    if !fetched.response.status().is_success() {
        return Err(format!("the video answered {}", fetched.response.status()));
    }
    let body =
        crate::services::outbound_security::read_limited_body(fetched.response, MAX_PAGE_BYTES)
            .await
            .map_err(|_| "the video page was too large".to_string())?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// A YouTube video's subtitles and title, the way its player gets them.
async fn watch_directly(address: &str, id: &str) -> Result<(String, String), String> {
    const PLAYER: &str = "https://www.youtube.com/youtubei/v1/player";
    let page = get_text(address).await?;
    let key = page
        .split("\"INNERTUBE_API_KEY\":\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .filter(|key| {
            key.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
        .ok_or_else(|| "the video page did not load as expected".to_string())?;
    let (url, client) = crate::services::outbound_security::build_public_http_client(
        &format!("{PLAYER}?key={key}"),
        PAGE_TIMEOUT,
        Some(BROWSER_AGENT),
    )
    .await
    .map_err(|_| "could not reach the video".to_string())?;
    let player: Value = client
        .post(url)
        .json(&serde_json::json!({
            "context": { "client": { "clientName": "ANDROID", "clientVersion": "20.10.38" } },
            "videoId": id,
        }))
        .send()
        .await
        .map_err(|_| "could not reach the video".to_string())?
        .json()
        .await
        .map_err(|_| "the video's player answered oddly".to_string())?;
    let title = player
        .pointer("/videoDetails/title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(200)
        .collect();
    let tracks = player
        .pointer("/captions/playerCaptionsTracklistRenderer/captionTracks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let track = pick_track(&tracks)
        .and_then(|track| track.get("baseUrl").and_then(Value::as_str))
        .filter(|url| url.starts_with("https://www.youtube.com/api/timedtext"))
        .ok_or_else(|| "the video has no subtitles to read".to_string())?;
    let text = timedtext_text(&get_text(track).await?);
    if text.is_empty() {
        return Err("the video's subtitles were empty".into());
    }
    Ok((title, text))
}

/// A video, by what is said in it.
pub async fn watch(address: &str) -> Result<Taken, String> {
    let address = video_address(address).ok_or_else(|| "not a video she can watch".to_string())?;
    let id = address.rsplit('=').next().unwrap_or_default().to_string();
    let direct = watch_directly(&address, &id).await;
    let (title, text) = match (direct, yt_dlp()) {
        (Ok(found), _) => found,
        (Err(_), Some(tool)) => {
            let text = watch_with_yt_dlp(&tool, &address).await?;
            (video_title(&tool, &address).await, text)
        }
        (Err(why), None) => return Err(why),
    };
    let (text, _) = myriad_agent_rules::compress_and_truncate_text(&text, MAX_TRANSCRIPT_CHARS);
    Ok(Taken {
        title,
        url: address,
        text,
    })
}

/// The fallback, only where someone has put yt-dlp on the host.
async fn watch_with_yt_dlp(tool: &std::path::Path, address: &str) -> Result<String, String> {
    // The spoken language's own captions first, one request; then the
    // uploader's subtitles in a language she reads.
    match subtitles(
        tool,
        &address,
        &["--write-auto-subs", "--sub-langs", ".*-orig"],
    )
    .await
    {
        Some(text) => Ok(text),
        None => subtitles(
            tool,
            &address,
            &["--write-subs", "--sub-langs", "en,ja,zh-Hans,zh-Hant"],
        )
        .await
        .ok_or_else(|| "the video has no subtitles to read".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn she_reads_under_her_name_with_the_site_to_reach() {
        let agent = user_agent().await;
        assert!(agent.starts_with("MyriadPersona/1.0 (+http"), "{agent}");
        assert!(agent.ends_with("; reading on her own)"));
    }

    #[test]
    fn robots_rules_for_everyone_and_for_ai_readers_are_honored() {
        let robots = "User-agent: SemrushBot\nUser-agent: *\nDisallow: /private/\nDisallow: /search$\nAllow: /private/ok\n\nUser-agent: Other\nDisallow: /other\n\nUser-agent: DotBot\nUser-agent: claude-user\nDisallow: /ebooks/*/text*";
        let rules = disallowed_for_her(robots);
        assert_eq!(rules, vec!["/private/", "/search$", "/ebooks/*/text*"]);
        assert!(path_is_closed("/private/x", &rules));
        assert!(path_is_closed("/search", &rules));
        assert!(!path_is_closed("/search/results", &rules));
        assert!(!path_is_closed("/other", &rules));
        assert!(path_is_closed(
            "/ebooks/charles-dickens/great-expectations/text/single-page",
            &rules
        ));
        assert!(!path_is_closed(
            "/ebooks/charles-dickens/great-expectations",
            &rules
        ));
        assert!(!path_is_closed(
            "/",
            &disallowed_for_her("User-agent: *\nDisallow:\n")
        ));
        // A site closed to GPTBot is closed to her.
        assert!(path_is_closed(
            "/any",
            &disallowed_for_her("User-agent: GPTBot\nDisallow: /\n")
        ));
    }

    #[test]
    fn a_page_reads_as_its_article() {
        let long = "有用的正文。".repeat(50);
        let html = format!(
            "<html><head><title>标题</title><script>var x=1;</script></head><body><nav>菜单</nav><article><p>{long}</p><script>ignored()</script></article></body></html>"
        );
        let (title, blocks) = page_text(&html);
        let text = focused(&blocks, None, MAX_PAGE_CHARS);
        assert_eq!(title, "标题");
        assert!(
            text.starts_with("有用的正文。") && !text.contains("菜单") && !text.contains("ignored")
        );
    }

    #[test]
    fn a_long_page_is_read_for_what_she_is_looking_for() {
        let mut blocks: Vec<String> =
            vec!["Opening about modulation.".into(), "Second opening.".into()];
        blocks.extend(
            (0..40).map(|i| format!("Filler paragraph number {i} about nothing much at all.")),
        );
        blocks
            .push("The truck driver's gear change became common in pop songs of the 1960s.".into());
        let text = focused(
            &blocks,
            Some("when did the truck driver's gear change become popular"),
            400,
        );
        assert!(
            text.starts_with("Opening about modulation. Second opening."),
            "{text}"
        );
        assert!(text.contains("1960s"), "{text}");
        assert!(!text.contains("Filler paragraph number 39"));
        // Short pages are read whole.
        assert_eq!(
            focused(&blocks[..2], Some("anything"), 400),
            "Opening about modulation. Second opening."
        );
        assert!(terms("最后一遍副歌升调").contains(&"升调".to_string()));
    }

    #[test]
    fn wikipedia_is_searched_in_the_question_s_language() {
        assert_eq!(wikipedia_language("為什麼 J-pop 要轉調"), "zh");
        assert_eq!(wikipedia_language("転調 サビ"), "ja");
        assert_eq!(wikipedia_language("key change last chorus"), "en");
        let payload = serde_json::json!({ "pages": [{
            "key": "まゆみ_(KANの曲)",
            "title": "まゆみ (KANの曲)",
            "excerpt": "<span class=\"searchmatch\">転調</span>するサビ",
            "description": "KANのシングル"
        }]});
        let hits = wikipedia_hits("ja", &payload);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].snippet, "KANのシングル 転調するサビ");
        assert!(hits[0].url.starts_with("https://ja.wikipedia.org/wiki/"));
        assert!(url::Url::parse(&hits[0].url).is_ok());
    }

    #[test]
    fn a_meme_is_looked_up_where_its_language_keeps_them() {
        let zh = meme_entries("绝绝子");
        assert!(zh[0].starts_with("https://zh.moegirl.org.cn/%E7%BB%9D"));
        assert_eq!(
            meme_entries("エモい")[0],
            "https://dic.nicovideo.jp/a/%E3%82%A8%E3%83%A2%E3%81%84"
        );
        assert_eq!(
            meme_entries("Doge!")[0],
            "https://knowyourmeme.com/memes/doge"
        );
        assert!(meme_entries("").is_empty());
        assert!(meme_entries(&"长".repeat(41)).is_empty());
    }

    #[test]
    fn only_youtube_videos_are_watched() {
        assert_eq!(
            video_address("https://youtu.be/arj7oStGLkU").as_deref(),
            Some("https://www.youtube.com/watch?v=arj7oStGLkU")
        );
        assert_eq!(
            video_address("https://m.youtube.com/watch?v=arj7oStGLkU&t=30").as_deref(),
            Some("https://www.youtube.com/watch?v=arj7oStGLkU")
        );
        assert!(video_address("https://evil.example/watch?v=arj7oStGLkU").is_none());
        assert!(video_address("https://www.youtube.com/watch?v=short").is_none());
    }

    #[test]
    fn the_spoken_language_s_own_subtitles_come_first() {
        let tracks = serde_json::json!([
            { "languageCode": "ar", "baseUrl": "a" },
            { "languageCode": "en", "kind": "asr", "baseUrl": "auto" },
            { "languageCode": "en", "baseUrl": "own" },
            { "languageCode": "ja", "baseUrl": "ja" }
        ]);
        let tracks = tracks.as_array().unwrap();
        assert_eq!(pick_track(tracks).unwrap()["baseUrl"], "own");
        assert_eq!(pick_track(&tracks[..2]).unwrap()["baseUrl"], "auto");
        assert_eq!(
            pick_track(&[tracks[0].clone(), tracks[3].clone()]).unwrap()["baseUrl"],
            "ja"
        );
        assert!(pick_track(&tracks[..1]).is_none());
        let xml = "<?xml version=\"1.0\" ?><timedtext format=\"3\"><body><p t=\"1\" d=\"2\">So in college,</p><p t=\"3\" d=\"2\"><s>I</s><s> was</s> a &amp; b &#39;c&#39;</p></body></timedtext>";
        assert_eq!(timedtext_text(xml), "So in college, I was a & b 'c'");
    }

    #[test]
    fn rolling_captions_read_once() {
        let vtt = "WEBVTT\nKind: captions\nLanguage: en\n\n00:00:12.559 --> 00:00:14.350 align:start\n \nso<00:00:12.759><c> in</c>\n\n00:00:14.350 --> 00:00:14.360\nso in\n \n\n00:00:14.360 --> 00:00:17.029\nso in\ncollege<00:00:15.360><c> I</c><c> was</c>\n\n00:00:17.029 --> 00:00:17.039\ncollege I was\n";
        assert_eq!(vtt_text(vtt), "so in college I was");
    }
}

/// Use each sense once for real: no model calls.
/// `YT_DLP_PATH=… cargo test … senses_for_real -- --ignored --nocapture`
#[cfg(test)]
mod live {
    #[tokio::test]
    #[ignore = "real search, pages and videos"]
    async fn senses_for_real() {
        let hits = super::search_wikipedia("転調 サビ 最後")
            .await
            .expect("search");
        for hit in &hits {
            println!("hit: {} | {} | {}", hit.title, hit.url, hit.snippet);
        }
        let page = super::read(&hits[0].url, Some("転調 サビ"))
            .await
            .expect("read");
        println!(
            "page: {} ({}) {} chars: {}",
            page.title,
            page.url,
            page.text.chars().count(),
            page.text.chars().take(200).collect::<String>()
        );
        let closed = super::read(
            "https://standardebooks.org/ebooks/charles-dickens/great-expectations/text/single-page",
            None,
        )
        .await;
        println!("robots-closed page: {closed:?}");
        match super::watch("https://youtu.be/arj7oStGLkU").await {
            Ok(video) => println!(
                "video: {} {} chars: {}",
                video.title,
                video.text.chars().count(),
                video.text.chars().take(200).collect::<String>()
            ),
            Err(why) => println!("video: {why}"),
        }
    }
}
