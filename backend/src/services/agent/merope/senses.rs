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

pub use myriad_merope::reading::{Hit, Taken};
use myriad_merope::reading::{
    SEARCH_RESULTS, disallowed_for_her, focused, meme_entries, page_text, path_is_closed,
    pick_track, timedtext_text, video_address, vtt_text, wikipedia_hits, wikipedia_language,
};

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

// --- robots.txt ------------------------------------------------------------------

static ROBOTS: LazyLock<Mutex<HashMap<String, (Instant, Vec<String>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
