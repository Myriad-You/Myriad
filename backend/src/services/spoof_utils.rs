//! 请求伪装工具模块
//!
//! 提供区域伪装功能，用于绕过地区限制
//! 支持中国、日本、美国等地区的 IP 和 UA 伪装

use rand::Rng;
use reqwest::header::{HeaderMap, HeaderValue};

/// 伪装配置
#[derive(Debug, Clone)]
pub struct SpoofConfig {
    /// 区域代码 (china, japan, us, etc.)
    pub region: String,
}

impl SpoofConfig {
    pub fn new(region: &str) -> Self {
        Self {
            region: region.to_lowercase(),
        }
    }
}

/// 伪装后的请求头
#[derive(Debug, Clone, Default)]
pub struct SpoofHeaders {
    pub x_forwarded_for: Option<String>,
    pub x_real_ip: Option<String>,
    pub user_agent: Option<String>,
    pub accept_language: Option<String>,
}

impl SpoofHeaders {
    /// 应用到 reqwest HeaderMap
    pub fn apply_to(&self, headers: &mut HeaderMap) {
        if let Some(ref xff) = self.x_forwarded_for {
            if let Ok(v) = HeaderValue::from_str(xff) {
                headers.insert("X-Forwarded-For", v);
            }
        }
        if let Some(ref xri) = self.x_real_ip {
            if let Ok(v) = HeaderValue::from_str(xri) {
                headers.insert("X-Real-IP", v);
            }
        }
        if let Some(ref ua) = self.user_agent {
            if let Ok(v) = HeaderValue::from_str(ua) {
                headers.insert("User-Agent", v);
            }
        }
        if let Some(ref al) = self.accept_language {
            if let Ok(v) = HeaderValue::from_str(al) {
                headers.insert("Accept-Language", v);
            }
        }
    }
}

/// 生成伪装请求头
pub fn generate_spoof_headers(config: &SpoofConfig) -> SpoofHeaders {
    match config.region.as_str() {
        "china" | "cn" => {
            generate_region_spoof(get_random_china_ip(), "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7")
        }
        "japan" | "jp" => {
            generate_region_spoof(get_random_japan_ip(), "ja-JP,ja;q=0.9,en-US;q=0.8,en;q=0.7")
        }
        "us" | "usa" | "america" => generate_region_spoof(get_random_us_ip(), "en-US,en;q=0.9"),
        "korea" | "kr" => {
            generate_region_spoof(get_random_korea_ip(), "ko-KR,ko;q=0.9,en-US;q=0.8,en;q=0.7")
        }
        "taiwan" | "tw" => generate_region_spoof(
            get_random_taiwan_ip(),
            "zh-TW,zh;q=0.9,en-US;q=0.8,en;q=0.7",
        ),
        "hongkong" | "hk" => generate_region_spoof(
            get_random_hongkong_ip(),
            "zh-HK,zh;q=0.9,en-US;q=0.8,en;q=0.7",
        ),
        _ => SpoofHeaders {
            user_agent: Some(get_random_common_ua().to_string()),
            accept_language: Some("en-US,en;q=0.9".to_string()),
            ..Default::default()
        },
    }
}

/// 通用区域伪装生成
fn generate_region_spoof(client_ip: String, accept_language: &str) -> SpoofHeaders {
    SpoofHeaders {
        x_forwarded_for: Some(client_ip.clone()),
        x_real_ip: Some(client_ip),
        user_agent: Some(get_random_common_ua().to_string()),
        accept_language: Some(accept_language.to_string()),
    }
}

// ============ IP 地址生成 ============

/// 生成随机的中国大陆 IP 地址
pub fn get_random_china_ip() -> String {
    let mut rng = rand::thread_rng();

    // 中国大陆主流运营商的真实 IP 段
    let china_ip_ranges = [
        // 中国电信
        ("58.20", 0..255, 0..255),
        ("58.21", 0..255, 0..255),
        ("59.41", 0..255, 0..255),
        ("60.12", 0..255, 0..255),
        ("61.128", 0..255, 0..255),
        ("116.21", 0..255, 0..255),
        ("218.4", 0..255, 0..255),
        // 中国联通
        ("112.24", 0..255, 0..255),
        ("112.25", 0..255, 0..255),
        ("113.12", 0..255, 0..255),
        ("124.160", 0..255, 0..255),
        ("221.192", 0..255, 0..255),
        // 中国移动
        ("111.13", 0..255, 0..255),
        ("111.19", 0..255, 0..255),
        ("117.131", 0..255, 0..255),
        ("223.64", 0..255, 0..255),
    ];

    let (prefix, range2, range3) = &china_ip_ranges[rng.gen_range(0..china_ip_ranges.len())];
    let third = rng.gen_range(range2.clone());
    let fourth = rng.gen_range(range3.clone());

    format!("{}.{}.{}", prefix, third, fourth)
}

/// 生成随机的日本 IP 地址
pub fn get_random_japan_ip() -> String {
    let mut rng = rand::thread_rng();

    let japan_ip_ranges = [
        ("133.1", 0..255, 0..255),
        ("133.2", 0..255, 0..255),
        ("202.32", 0..255, 0..255),
        ("210.130", 0..255, 0..255),
        ("126.1", 0..255, 0..255),
        ("153.126", 0..255, 0..255),
    ];

    let (prefix, range2, range3) = &japan_ip_ranges[rng.gen_range(0..japan_ip_ranges.len())];
    let third = rng.gen_range(range2.clone());
    let fourth = rng.gen_range(range3.clone());

    format!("{}.{}.{}", prefix, third, fourth)
}

/// 生成随机的美国 IP 地址
pub fn get_random_us_ip() -> String {
    let mut rng = rand::thread_rng();

    let us_ip_ranges = [
        ("24.1", 0..255, 0..255),
        ("24.2", 0..255, 0..255),
        ("66.87", 0..255, 0..255),
        ("174.16", 0..255, 0..255),
        ("71.41", 0..255, 0..255),
        ("75.139", 0..255, 0..255),
    ];

    let (prefix, range2, range3) = &us_ip_ranges[rng.gen_range(0..us_ip_ranges.len())];
    let third = rng.gen_range(range2.clone());
    let fourth = rng.gen_range(range3.clone());

    format!("{}.{}.{}", prefix, third, fourth)
}

/// 生成随机的韩国 IP 地址
pub fn get_random_korea_ip() -> String {
    let mut rng = rand::thread_rng();

    let korea_ip_ranges = [
        ("175.193", 0..255, 0..255),
        ("175.194", 0..255, 0..255),
        ("211.36", 0..255, 0..255),
        ("121.88", 0..255, 0..255),
    ];

    let (prefix, range2, range3) = &korea_ip_ranges[rng.gen_range(0..korea_ip_ranges.len())];
    let third = rng.gen_range(range2.clone());
    let fourth = rng.gen_range(range3.clone());

    format!("{}.{}.{}", prefix, third, fourth)
}

/// 生成随机的台湾 IP 地址
pub fn get_random_taiwan_ip() -> String {
    let mut rng = rand::thread_rng();

    let taiwan_ip_ranges = [
        ("36.224", 0..255, 0..255),
        ("36.225", 0..255, 0..255),
        ("61.216", 0..255, 0..255),
        ("114.32", 0..255, 0..255),
    ];

    let (prefix, range2, range3) = &taiwan_ip_ranges[rng.gen_range(0..taiwan_ip_ranges.len())];
    let third = rng.gen_range(range2.clone());
    let fourth = rng.gen_range(range3.clone());

    format!("{}.{}.{}", prefix, third, fourth)
}

/// 生成随机的香港 IP 地址
pub fn get_random_hongkong_ip() -> String {
    let mut rng = rand::thread_rng();

    let hk_ip_ranges = [
        ("202.40", 0..255, 0..255),
        ("202.41", 0..255, 0..255),
        ("113.252", 0..255, 0..255),
        ("223.16", 0..255, 0..255),
    ];

    let (prefix, range2, range3) = &hk_ip_ranges[rng.gen_range(0..hk_ip_ranges.len())];
    let third = rng.gen_range(range2.clone());
    let fourth = rng.gen_range(range3.clone());

    format!("{}.{}.{}", prefix, third, fourth)
}

// ============ User-Agent 生成 ============

/// 通用 User-Agent
pub fn get_random_common_ua() -> &'static str {
    let mut rng = rand::thread_rng();
    let user_agents = [
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:121.0) Gecko/20100101 Firefox/121.0",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:121.0) Gecko/20100101 Firefox/121.0",
        "Mozilla/5.0 (iPhone; CPU iPhone OS 17_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.1 Mobile/15E148 Safari/604.1",
        "Mozilla/5.0 (Linux; Android 13; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36",
    ];

    user_agents[rng.gen_range(0..user_agents.len())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_china_spoof() {
        let config = SpoofConfig::new("china");
        let headers = generate_spoof_headers(&config);

        assert!(headers.x_forwarded_for.is_some());
        assert!(headers.x_real_ip.is_some());
        assert!(headers.accept_language.is_some());
        assert!(headers.accept_language.unwrap().contains("zh-CN"));
    }

    #[test]
    fn test_japan_spoof() {
        let config = SpoofConfig::new("japan");
        let headers = generate_spoof_headers(&config);

        assert!(headers.accept_language.is_some());
        assert!(headers.accept_language.unwrap().contains("ja-JP"));
    }

    #[test]
    fn test_us_spoof() {
        let config = SpoofConfig::new("us");
        let headers = generate_spoof_headers(&config);

        assert!(headers.accept_language.is_some());
        assert!(headers.accept_language.unwrap().contains("en-US"));
    }
}
