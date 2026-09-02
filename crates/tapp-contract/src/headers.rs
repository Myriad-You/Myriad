//! HTTP header-name parsing for manifest credentials and inbound verify.

use std::str::FromStr;

pub fn parse_http_header_name(value: &str) -> Result<http::HeaderName, ()> {
    http::HeaderName::from_str(value).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_header_name_accepts_token_names() {
        assert_eq!(
            parse_http_header_name("X-Signature").unwrap().as_str(),
            "x-signature"
        );
        assert!(parse_http_header_name("not a header").is_err());
        assert!(parse_http_header_name("").is_err());
    }

    #[test]
    fn header_module_parses_with_http_crate() {
        let src = include_str!("headers.rs");
        let production = src.split("#[cfg(test)]").next().expect("source");
        assert!(production.contains("http::HeaderName"));
        assert!(!production.contains("reqwest::"));
    }
}
