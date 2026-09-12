//! Outbound HTTP error classification without a client crate.
//!
//! Classification runs on text with URLs stripped so query credentials never
//! participate in the label. Logging sites must record only endpoint identity,
//! this kind, and a credential-safe flag — never the raw client Display.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundHttpErrorKind {
    Timeout,
    Connect,
    Request,
    Other,
}

impl OutboundHttpErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Connect => "connect",
            Self::Request => "request",
            Self::Other => "other",
        }
    }
}

impl std::fmt::Display for OutboundHttpErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Drop `http(s)://…` spans and `for url (…)` so query material cannot leak
/// into classification or logs that reuse the stripped text.
pub fn strip_urls_from_error(detail: &str) -> String {
    let bytes = detail.as_bytes();
    let mut out = String::with_capacity(detail.len());
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = url_span_end(bytes, i) {
            out.push_str("[url]");
            i = end;
            continue;
        }
        let ch = detail[i..].chars().next().expect("valid utf-8 offset");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn url_span_end(bytes: &[u8], i: usize) -> Option<usize> {
    if starts_ignore_ascii_case(bytes, i, b"https://") {
        return Some(consume_url_body(bytes, i + 8));
    }
    if starts_ignore_ascii_case(bytes, i, b"http://") {
        return Some(consume_url_body(bytes, i + 7));
    }
    if starts_ignore_ascii_case(bytes, i, b"for url (") {
        let mut j = i + 9;
        while j < bytes.len() && bytes[j] != b')' {
            j += 1;
        }
        return Some(if j < bytes.len() { j + 1 } else { bytes.len() });
    }
    None
}

fn consume_url_body(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() || c == b')' || c == b'"' || c == b'\'' {
            break;
        }
        i += 1;
    }
    i
}

fn starts_ignore_ascii_case(bytes: &[u8], i: usize, needle: &[u8]) -> bool {
    let Some(end) = i.checked_add(needle.len()) else {
        return false;
    };
    if end > bytes.len() {
        return false;
    }
    bytes[i..end].eq_ignore_ascii_case(needle)
}

/// Classify after URLs are removed. Order: timeout, connect, request, other.
pub fn classify_outbound_http_error(detail: &str) -> OutboundHttpErrorKind {
    let stripped = strip_urls_from_error(detail);
    let lower = stripped.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        return OutboundHttpErrorKind::Timeout;
    }
    if lower.contains("error trying to connect")
        || lower.contains("connection refused")
        || lower.contains("could not connect")
        || lower.contains("dns error")
        || lower.contains("failed to lookup")
        || lower.contains("name or service not known")
    {
        return OutboundHttpErrorKind::Connect;
    }
    if lower.contains("error sending request")
        || lower.contains("builder error")
        || lower.contains("invalid url")
    {
        return OutboundHttpErrorKind::Request;
    }
    OutboundHttpErrorKind::Other
}

/// Scheme + host + path. Query and fragment are dropped so injected credentials
/// never become part of the logged identity.
pub fn outbound_endpoint_identity(url: &str) -> String {
    let without_frag = url.split_once('#').map(|(head, _)| head).unwrap_or(url);
    without_frag
        .split_once('?')
        .map(|(head, _)| head.to_string())
        .unwrap_or_else(|| without_frag.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_query_before_classification() {
        let detail = "error sending request for url (http://127.0.0.1:9/weather?appid=top-secret/value+plus): error trying to connect: tcp connect error";
        let stripped = strip_urls_from_error(detail);
        assert!(!stripped.contains("top-secret"));
        assert!(!stripped.contains("appid="));
        assert!(!stripped.contains("127.0.0.1"));
        assert_eq!(
            classify_outbound_http_error(detail),
            OutboundHttpErrorKind::Connect
        );
    }

    #[test]
    fn classifies_timeout_after_url_strip() {
        let detail = "error sending request for url (https://api.example/path?k=secret): operation timed out";
        assert_eq!(
            classify_outbound_http_error(detail),
            OutboundHttpErrorKind::Timeout
        );
    }

    #[test]
    fn classifies_request_without_connect_phrase() {
        let detail = "error sending request for url (http://example/x?token=abc)";
        assert_eq!(
            classify_outbound_http_error(detail),
            OutboundHttpErrorKind::Request
        );
    }

    #[test]
    fn endpoint_identity_drops_query_and_fragment() {
        assert_eq!(
            outbound_endpoint_identity("http://127.0.0.1:9/weather?q=tokyo&appid=secret#frag"),
            "http://127.0.0.1:9/weather"
        );
    }
}
