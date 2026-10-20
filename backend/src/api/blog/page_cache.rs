//! 拼好的博客页：同一份文件配同一条导航岛、同一边深浅色只拼一次。
//!
//! 页面内容只由键里的东西决定（文件的修改时间和长度、深浅色、导航岛的整段 HTML），
//! 所以缓存不会把一位访客的页面给另一位：两人拿到同一份，是因为他们本来就该看到同一份。
//! 站长的页面带创作台，不进这里。ETag 是内容的摘要，重建、改代码之后自然就变。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use axum::body::Bytes;
use sha2::{Digest, Sha256};

const MAX_ENTRIES: usize = 512;
const MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Hash, PartialEq, Eq)]
struct Key {
    path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
    dark: Option<bool>,
    nav: String,
}

#[derive(Clone)]
pub(super) struct Page {
    pub body: Bytes,
    pub etag: String,
}

struct Entry {
    page: Page,
    used: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Entry>,
    bytes: usize,
    tick: u64,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);

/// 这一页给这条导航岛、这一边深浅色拼好的样子。文件读不到就是 `None`。
pub(super) async fn page(
    path: &Path,
    dark: Option<bool>,
    nav: &str,
    build: impl FnOnce(String) -> String,
) -> Option<Page> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    let key = Key {
        path: path.to_path_buf(),
        modified: meta.modified().ok(),
        len: meta.len(),
        dark,
        nav: nav.to_string(),
    };
    {
        let mut cache = CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.tick += 1;
        let tick = cache.tick;
        if let Some(entry) = cache.entries.get_mut(&key) {
            entry.used = tick;
            return Some(entry.page.clone());
        }
    }
    let html = tokio::fs::read_to_string(path).await.ok()?;
    let body = Bytes::from(build(html));
    let page = Page {
        etag: etag_of(&body),
        body,
    };
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.insert(key, page.clone());
    Some(page)
}

impl Cache {
    fn insert(&mut self, key: Key, page: Page) {
        let size = page.body.len() + key.nav.len();
        if size > MAX_BYTES {
            return;
        }
        self.tick += 1;
        let used = self.tick;
        if let Some(old) = self.entries.insert(key.clone(), Entry { page, used }) {
            self.bytes -= old.page.body.len() + key.nav.len();
        }
        self.bytes += size;
        while self.entries.len() > MAX_ENTRIES || self.bytes > MAX_BYTES {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes -= entry.page.body.len() + oldest.nav.len();
            }
        }
    }
}

fn etag_of(body: &[u8]) -> String {
    let digest = Sha256::digest(body);
    format!("\"{}\"", hex::encode(&digest[..16]))
}

/// 样式、图片的 ETag：长度和修改时间，不用读文件。
pub(super) fn file_etag(meta: &std::fs::Metadata) -> String {
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    format!("\"{:x}-{modified:x}\"", meta.len())
}

/// `If-None-Match` 里有没有这个 ETag（弱比较，`*` 也算）。
pub(super) fn etag_matches(header: Option<&axum::http::HeaderValue>, etag: &str) -> bool {
    let Some(value) = header.and_then(|value| value.to_str().ok()) else {
        return false;
    };
    value.split(',').map(str::trim).any(|tag| {
        tag == "*"
            || tag.strip_prefix("W/").unwrap_or(tag) == etag.strip_prefix("W/").unwrap_or(etag)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn if_none_match_is_compared_weakly_and_as_a_list() {
        let etag = "\"abc\"";
        let matches = |raw: &str| etag_matches(Some(&HeaderValue::from_str(raw).unwrap()), etag);
        assert!(matches("\"abc\""));
        assert!(matches("W/\"abc\""));
        assert!(matches("\"x\", \"abc\""));
        assert!(matches("*"));
        assert!(!matches("\"abcd\""));
        assert!(!matches("abc"));
        assert!(!etag_matches(None, etag));
    }

    #[test]
    fn cache_stays_within_its_limits_and_drops_the_least_recent() {
        let mut cache = Cache::default();
        let key = |n: usize| Key {
            path: PathBuf::from(format!("/p/{n}")),
            modified: None,
            len: 0,
            dark: None,
            nav: String::new(),
        };
        let page = |len: usize| Page {
            body: Bytes::from(vec![b'x'; len]),
            etag: String::new(),
        };
        for n in 0..MAX_ENTRIES + 10 {
            cache.insert(key(n), page(1));
        }
        assert_eq!(cache.entries.len(), MAX_ENTRIES);
        assert!(!cache.entries.contains_key(&key(0)));
        assert!(cache.entries.contains_key(&key(MAX_ENTRIES + 9)));
        cache.insert(key(0), page(MAX_BYTES - 100));
        assert!(cache.bytes <= MAX_BYTES);
        assert!(cache.entries.contains_key(&key(0)));
        cache.insert(key(1), page(MAX_BYTES + 1));
        assert!(!cache.entries.contains_key(&key(1)));
    }

    #[test]
    fn etag_follows_the_content() {
        assert_eq!(etag_of(b"a"), etag_of(b"a"));
        assert_ne!(etag_of(b"a"), etag_of(b"b"));
    }
}
