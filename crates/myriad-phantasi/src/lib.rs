//! Pure Phantasi domain rules.
//!
//! Path constants.
//! No database, HTTP, or Axum.

pub mod feed_topic_cards;

pub use feed_topic_cards::{
    FEED_TOPIC_CARDS_KEY, feed_topic_cards_from_value, sanitize_feed_topic_cards,
};

/// User-visible SPA prefix. Internal module key stays `phantasi`.
pub const SPA_PREFIX: &str = "/journal";

/// ActivityPub object prefix. Changing article IDs under this path breaks federation.
pub const ARTICLE_OBJECT_PREFIX: &str = "/phantasi";

/// Public notes RSS.
pub const NOTES_RSS_PATH: &str = "/journal/notes.xml";

/// Platform config key for the notes RSS switch.
pub const NOTES_RSS_PREFERENCES_KEY: &str = "phantasi_notes_rss";

pub fn item_path(item_id: i32) -> String {
    format!("{SPA_PREFIX}/articles/{item_id}")
}

/// ActivityPub object ID. Changing this breaks federation.
pub fn article_federation_path(item_id: i32) -> String {
    format!("{ARTICLE_OBJECT_PREFIX}/articles/{item_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_do_not_contain_brew() {
        assert!(!NOTES_RSS_PATH.contains("brew"));
        assert_eq!(item_path(12), "/journal/articles/12");
        assert_eq!(article_federation_path(12), "/phantasi/articles/12");
        assert_eq!(SPA_PREFIX, "/journal");
        assert_eq!(NOTES_RSS_PATH, "/journal/notes.xml");
        assert_eq!(ARTICLE_OBJECT_PREFIX, "/phantasi");
        assert_eq!(FEED_TOPIC_CARDS_KEY, "phantasi_feed_topic_cards");
        assert!(!FEED_TOPIC_CARDS_KEY.contains("brew"));
    }
}
