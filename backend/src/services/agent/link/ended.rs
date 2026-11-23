//! How a link ended, as its log line tells it.

use axum::extract::ws::CloseFrame;

use super::contract::Close;

/// The longest text of the other side's a log line takes.
const REASON_CHARS: usize = 120;

/// A run of token characters this long is not logged: it may be a credential
/// (base64, with or without padding, a JWT, or one percent-encoded).
const TOKEN_CHARS: usize = 24;

/// WebSocket's own number for a close frame that carries none.
const NO_CODE: u16 = 1005;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ended {
    /// The site closed it.
    Here(Close),
    /// The child's close frame came.
    Child { code: u16, reason: String },
    /// Gone with no close frame: what the socket said of it.
    Cut(String),
}

impl Ended {
    pub(super) fn by_child(frame: Option<CloseFrame>) -> Self {
        frame.map_or_else(
            || Self::Child {
                code: NO_CODE,
                reason: String::new(),
            },
            |frame| Self::Child {
                code: frame.code,
                reason: loggable(frame.reason.as_str()),
            },
        )
    }

    pub(super) fn cut(said: &str) -> Self {
        Self::Cut(loggable(said))
    }

    /// Why the site closed it, when it did.
    pub(super) fn here(&self) -> Option<Close> {
        match self {
            Self::Here(why) => Some(*why),
            Self::Child { .. } | Self::Cut(_) => None,
        }
    }

    pub(super) fn why(&self) -> &'static str {
        match self {
            Self::Here(why) => why.name(),
            Self::Child { .. } => "closed by the child",
            Self::Cut(_) => "cut off",
        }
    }

    /// The close frame's number; `none` only when there was no frame.
    pub(super) fn code(&self) -> String {
        match self {
            Self::Here(why) => why.code().to_string(),
            Self::Child { code, .. } => code.to_string(),
            Self::Cut(_) => "none".to_string(),
        }
    }

    pub(super) fn reason(&self) -> &str {
        match self {
            Self::Here(_) => "",
            Self::Child { reason, .. } | Self::Cut(reason) => reason,
        }
    }
}

/// The other side's text as a log line may hold it: on one line, with nothing
/// that could be a token, and cut short.
fn loggable(text: &str) -> String {
    let mut out = String::new();
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.len() >= TOKEN_CHARS {
            out.push_str("[redacted]");
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '/' | '=' | '.' | '~' | '%') {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(if c.is_control() { ' ' } else { c });
        }
    }
    flush(&mut run, &mut out);
    out.chars().take(REASON_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::{Close, CloseFrame, Ended, loggable};

    #[test]
    fn who_ended_it_is_told_apart_with_the_number() {
        let told = |ended: &Ended| {
            (
                ended.why(),
                ended.code(),
                ended.reason().to_string(),
                ended.here(),
            )
        };
        assert_eq!(
            told(&Ended::by_child(Some(CloseFrame {
                code: 1001,
                reason: "going away".into(),
            }))),
            (
                "closed by the child",
                "1001".into(),
                "going away".into(),
                None
            )
        );
        // A frame with no number is still a frame.
        assert_eq!(
            told(&Ended::by_child(None)),
            ("closed by the child", "1005".into(), String::new(), None)
        );
        assert_eq!(
            told(&Ended::cut("Connection reset")),
            ("cut off", "none".into(), "Connection reset".into(), None)
        );
        for close in Close::ALL {
            assert_eq!(
                told(&Ended::Here(close)),
                (
                    close.name(),
                    close.code().to_string(),
                    String::new(),
                    Some(close)
                )
            );
        }
    }

    #[test]
    fn the_other_sides_text_is_one_short_line_without_credentials() {
        assert_eq!(loggable("a\r\nb\u{1b}[31m"), "a  b [31m");
        assert_eq!(loggable(&"关 ".repeat(200)).chars().count(), 120);
        let secret = "k".repeat(43);
        assert_eq!(
            loggable(&format!("Bearer myr_{secret}, bye")),
            "Bearer [redacted], bye"
        );
        for secret in [
            "q83vASNFZ4mrze8BI0Vn/iasze8BI0VniavN7w==",
            "eyJhbGciOiJ.eyJzdWIiOiI.SflKxwRJSMe",
            "a1b2%2Bc3d4%2Fe5f6%3Dg7h8~",
        ] {
            assert_eq!(loggable(&format!("no {secret}")), "no [redacted]");
        }
        // Cut short only after that: no part of one is left at the end.
        let late = loggable(&format!("{} {secret}", "词".repeat(110)));
        assert!(!late.contains("kkk"), "{late}");
        assert_eq!(loggable("unsupported_protocol"), "unsupported_protocol");
    }
}
