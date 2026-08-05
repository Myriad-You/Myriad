use reqwest::{redirect::Policy, Client};
use serde::Serialize;
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::time::Duration;

const LOCATION_REQUEST_TIMEOUT_SECONDS: u64 = 5;
const LOCATION_RESPONSE_LIMIT: u64 = 64 * 1024;
const VERIFIED_DISTANCE_KM: f64 = 100.0;

#[derive(Clone, Debug)]
struct LocationObservation {
    source: &'static str,
    ip: IpAddr,
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
    country_code: String,
    latitude: f64,
    longitude: f64,
    asn: Option<String>,
    organization: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ServerLocationAssessment {
    pub status: &'static str,
    pub confidence: &'static str,
    pub reason: &'static str,
    pub public_ips: Vec<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub agreement_km: Option<f64>,
    pub asn: Option<String>,
    pub organization: Option<String>,
    pub sources: Vec<String>,
    pub app_proxy_bypassed: bool,
    pub method: &'static str,
}

fn clean_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(120).collect())
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_broadcast()
        && !ip.is_documentation()
        && !ip.is_unspecified()
        && !ip.is_multicast()
        && octets[0] != 0
        && !(octets[0] == 100 && (64..=127).contains(&octets[1]))
        && !(octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        && !(octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        && octets[0] < 240
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast()
        || (segments[0] == 0x2001 && segments[1] == 0x0db8))
}

fn valid_coordinates(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && longitude.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude)
}

async fn read_json(client: &Client, url: &str) -> Result<Value, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;

    if response.content_length().unwrap_or(0) > LOCATION_RESPONSE_LIMIT {
        return Err("Location response exceeded the size limit".to_string());
    }

    let body = response.bytes().await.map_err(|error| error.to_string())?;
    if body.len() as u64 > LOCATION_RESPONSE_LIMIT {
        return Err("Location response exceeded the size limit".to_string());
    }

    serde_json::from_slice(&body).map_err(|error| error.to_string())
}

fn parse_ipapi(value: Value) -> Result<LocationObservation, String> {
    if value.get("error").and_then(Value::as_bool) == Some(true) {
        return Err("ipapi.co rejected the lookup".to_string());
    }

    let ip = clean_string(value.get("ip"))
        .and_then(|value| IpAddr::from_str(&value).ok())
        .filter(|ip| is_public_ip(*ip))
        .ok_or_else(|| "ipapi.co returned an invalid public IP".to_string())?;
    let latitude = value
        .get("latitude")
        .and_then(Value::as_f64)
        .ok_or_else(|| "ipapi.co did not return latitude".to_string())?;
    let longitude = value
        .get("longitude")
        .and_then(Value::as_f64)
        .ok_or_else(|| "ipapi.co did not return longitude".to_string())?;
    if !valid_coordinates(latitude, longitude) {
        return Err("ipapi.co returned invalid coordinates".to_string());
    }

    let country_code = clean_string(value.get("country_code").or_else(|| value.get("country")))
        .filter(|value| value.len() == 2)
        .ok_or_else(|| "ipapi.co did not return a country code".to_string())?;

    Ok(LocationObservation {
        source: "ipapi.co",
        ip,
        city: clean_string(value.get("city")),
        region: clean_string(value.get("region")),
        country: clean_string(value.get("country_name")),
        country_code: country_code.to_uppercase(),
        latitude,
        longitude,
        asn: clean_string(value.get("asn")),
        organization: clean_string(value.get("org")),
    })
}

fn parse_ipwho(value: Value) -> Result<LocationObservation, String> {
    if value.get("success").and_then(Value::as_bool) != Some(true) {
        return Err("ipwho.is rejected the lookup".to_string());
    }

    let ip = clean_string(value.get("ip"))
        .and_then(|value| IpAddr::from_str(&value).ok())
        .filter(|ip| is_public_ip(*ip))
        .ok_or_else(|| "ipwho.is returned an invalid public IP".to_string())?;
    let latitude = value
        .get("latitude")
        .and_then(Value::as_f64)
        .ok_or_else(|| "ipwho.is did not return latitude".to_string())?;
    let longitude = value
        .get("longitude")
        .and_then(Value::as_f64)
        .ok_or_else(|| "ipwho.is did not return longitude".to_string())?;
    if !valid_coordinates(latitude, longitude) {
        return Err("ipwho.is returned invalid coordinates".to_string());
    }

    let country_code = clean_string(value.get("country_code"))
        .filter(|value| value.len() == 2)
        .ok_or_else(|| "ipwho.is did not return a country code".to_string())?;
    let connection = value.get("connection");
    let asn = connection
        .and_then(|value| value.get("asn"))
        .and_then(Value::as_u64)
        .map(|value| format!("AS{value}"))
        .or_else(|| connection.and_then(|value| clean_string(value.get("asn"))));
    let organization = connection
        .and_then(|value| clean_string(value.get("org")))
        .or_else(|| connection.and_then(|value| clean_string(value.get("isp"))));

    Ok(LocationObservation {
        source: "ipwho.is",
        ip,
        city: clean_string(value.get("city")),
        region: clean_string(value.get("region")),
        country: clean_string(value.get("country")),
        country_code: country_code.to_uppercase(),
        latitude,
        longitude,
        asn,
        organization,
    })
}

fn distance_km(left: &LocationObservation, right: &LocationObservation) -> f64 {
    let earth_radius_km = 6371.0;
    let lat_delta = (right.latitude - left.latitude).to_radians();
    let lon_delta = (right.longitude - left.longitude).to_radians();
    let left_lat = left.latitude.to_radians();
    let right_lat = right.latitude.to_radians();
    let haversine = (lat_delta / 2.0).sin().powi(2)
        + left_lat.cos() * right_lat.cos() * (lon_delta / 2.0).sin().powi(2);
    2.0 * earth_radius_km * haversine.sqrt().asin()
}

pub fn unavailable_assessment(app_proxy_bypassed: bool) -> ServerLocationAssessment {
    ServerLocationAssessment {
        status: "warning",
        confidence: "unavailable",
        reason: "unavailable",
        public_ips: Vec::new(),
        city: None,
        region: None,
        country: None,
        country_code: None,
        latitude: None,
        longitude: None,
        agreement_km: None,
        asn: None,
        organization: None,
        sources: Vec::new(),
        app_proxy_bypassed,
        method: "direct_https_consensus",
    }
}

fn assess_observations(
    observations: Vec<LocationObservation>,
    app_proxy_bypassed: bool,
) -> ServerLocationAssessment {
    let Some(primary) = observations.first() else {
        return unavailable_assessment(app_proxy_bypassed);
    };

    let mut public_ips = observations
        .iter()
        .map(|observation| observation.ip.to_string())
        .collect::<Vec<_>>();
    public_ips.sort();
    public_ips.dedup();
    let sources = observations
        .iter()
        .map(|observation| observation.source.to_string())
        .collect::<Vec<_>>();

    let (status, confidence, reason, agreement_km) = if let Some(secondary) = observations.get(1) {
        let distance = distance_km(primary, secondary);
        let verified = primary
            .country_code
            .eq_ignore_ascii_case(&secondary.country_code)
            && distance <= VERIFIED_DISTANCE_KM;
        (
            if verified { "ok" } else { "warning" },
            if verified { "high" } else { "low" },
            if verified { "verified" } else { "conflict" },
            Some((distance * 10.0).round() / 10.0),
        )
    } else {
        ("warning", "low", "single_source", None)
    };

    ServerLocationAssessment {
        status,
        confidence,
        reason,
        public_ips,
        city: primary.city.clone(),
        region: primary.region.clone(),
        country: primary.country.clone(),
        country_code: Some(primary.country_code.clone()),
        latitude: Some(primary.latitude),
        longitude: Some(primary.longitude),
        agreement_km,
        asn: primary.asn.clone(),
        organization: primary.organization.clone(),
        sources,
        app_proxy_bypassed,
        method: "direct_https_consensus",
    }
}

pub async fn inspect_server_location() -> ServerLocationAssessment {
    let proxy_config = super::http_client::ProxyConfig::from_dynamic_config().await;
    let app_proxy_bypassed = proxy_config.should_use_proxy();
    let client = match Client::builder()
        .timeout(Duration::from_secs(LOCATION_REQUEST_TIMEOUT_SECONDS))
        .connect_timeout(Duration::from_secs(3))
        .redirect(Policy::none())
        .no_proxy()
        .user_agent("Myriad-RuntimeDiagnostics/1.0")
        .build()
    {
        Ok(client) => client,
        Err(_) => return unavailable_assessment(app_proxy_bypassed),
    };

    let (ipapi_result, ipwho_result) = tokio::join!(
        async {
            read_json(&client, "https://ipapi.co/json/")
                .await
                .and_then(parse_ipapi)
        },
        async {
            read_json(&client, "https://ipwho.is/")
                .await
                .and_then(parse_ipwho)
        }
    );
    let observations = [ipapi_result, ipwho_result]
        .into_iter()
        .filter_map(Result::ok)
        .collect();

    assess_observations(observations, app_proxy_bypassed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(
        source: &'static str,
        ip: &str,
        country_code: &str,
        latitude: f64,
        longitude: f64,
    ) -> LocationObservation {
        LocationObservation {
            source,
            ip: ip.parse().unwrap(),
            city: Some("Tokyo".to_string()),
            region: Some("Tokyo".to_string()),
            country: Some("Japan".to_string()),
            country_code: country_code.to_string(),
            latitude,
            longitude,
            asn: Some("AS64500".to_string()),
            organization: Some("Example".to_string()),
        }
    }

    #[test]
    fn rejects_non_public_addresses() {
        assert!(!is_public_ip("127.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("10.0.0.1".parse().unwrap()));
        assert!(!is_public_ip("2001:db8::1".parse().unwrap()));
        assert!(is_public_ip("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn verifies_two_close_independent_observations() {
        let result = assess_observations(
            vec![
                observation("first", "8.8.8.8", "JP", 35.6762, 139.6503),
                observation("second", "2001:4860::1", "JP", 35.6895, 139.6917),
            ],
            true,
        );

        assert_eq!(result.status, "ok");
        assert_eq!(result.confidence, "high");
        assert_eq!(result.reason, "verified");
        assert!(result.app_proxy_bypassed);
    }

    #[test]
    fn flags_conflicting_locations() {
        let result = assess_observations(
            vec![
                observation("first", "8.8.8.8", "JP", 35.6762, 139.6503),
                observation("second", "1.1.1.1", "US", 37.7749, -122.4194),
            ],
            false,
        );

        assert_eq!(result.status, "warning");
        assert_eq!(result.reason, "conflict");
    }
}
