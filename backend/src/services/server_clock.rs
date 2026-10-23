//! The site's own clock, checked against the world.
//!
//! The site keeps time by its host's clock: a Docker container has no clock
//! of its own. A host clock that is minutes off does not look wrong from
//! here; every device looks wrong instead. A device whose clock is right is
//! refused as ahead (`clock_ahead`), and reminders come early or late. So the
//! web process measures how far its clock is off, once at start and every
//! [`EVERY`] after: SNTP (UDP 123) to each configured server, and the `Date`
//! of an HTTPS answer (certificate verified) for where UDP 123 is blocked.
//! Nothing here sets the clock. The deployment's time-sync container does
//! that, when it can.
//!
//! What a measurement is for:
//! - a warning in the log when the clock is off by more than the threshold;
//! - the one comparison of a device's clock with the site's
//!   (`middleware::sync_device`): it is made against real time, this host's
//!   clock moved by the offset, while a measurement is [`Check::correction`]
//!   (trusted, at most [`MAX_AGE`] old, at most [`MAX_CORRECTION_MS`] off).
//!   Everything else the site does keeps its own clock: two clocks in one
//!   process would be worse than one that is wrong;
//! - `serverClock` in the device hello and on the owner's devices page.
//!
//! Trusted is an HTTPS `Date` (the certificate vouches for who sent it), or
//! two SNTP servers that agree; one SNTP answer alone is unauthenticated UDP
//! and only shown. When nothing answers, the check is "could not check",
//! never an offset.
//!
//! Settings (environment; see `docs/deployment/DOCKER_DEPLOYMENT.md`):
//! `CLOCK_CHECK_ENABLED` (default on), `CLOCK_CHECK_NTP_SERVERS` and
//! `CLOCK_CHECK_HTTPS_URLS` (comma separated, empty for none),
//! `CLOCK_CHECK_THRESHOLD_SECS` (default 60).

use std::sync::RwLock;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

/// How often the clock is checked.
pub const EVERY: Duration = Duration::from_secs(15 * 60);
/// How long a measurement is used for device clocks.
pub const MAX_AGE: Duration = Duration::from_secs(45 * 60);
/// An offset beyond this is not believed: the site's clock is not used for
/// device clocks with it, so a forged time cannot pull it far.
pub const MAX_CORRECTION_MS: i64 = 3 * 60 * 60 * 1000;
/// How far two SNTP answers may differ and still agree.
pub const AGREE_MS: i64 = 1000;
/// Default threshold for "the site's clock is wrong".
pub const DEFAULT_THRESHOLD_SECS: i64 = 60;
const DEFAULT_NTP_SERVERS: &str = "time.cloudflare.com,time.google.com,pool.ntp.org";
const DEFAULT_HTTPS_URLS: &str = "https://www.cloudflare.com,https://www.google.com";
/// One query's timeout.
const TIMEOUT: Duration = Duration::from_secs(4);

/// Seconds from 1900 (NTP) to 1970 (Unix).
const NTP_UNIX_SECS: i64 = 2_208_988_800;

/// Where the offset came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Sntp,
    Https,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::Sntp => "sntp",
            Self::Https => "https",
        }
    }
}

/// How far the site's clock is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measurement {
    /// The site's clock minus real time: negative when the site is behind.
    pub offset_ms: i64,
    pub source: Source,
    /// An HTTPS `Date`, or SNTP servers that agree.
    pub trusted: bool,
}

/// One check of the clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// By the site's own clock, milliseconds since the epoch.
    pub checked_at_ms: i64,
    /// None: nothing answered (could not check).
    pub measured: Option<Measurement>,
    pub threshold_ms: i64,
}

impl Check {
    /// Off by more than the threshold.
    pub fn wrong(&self) -> bool {
        self.measured
            .is_some_and(|measured| measured.offset_ms.abs() > self.threshold_ms)
    }

    /// The offset the site moves its clock by when it compares a device's
    /// clock with real time: only a trusted measurement, `age` at most
    /// [`MAX_AGE`] old and at most [`MAX_CORRECTION_MS`] off.
    pub fn correction(&self, age: Duration) -> Option<Measurement> {
        self.measured.filter(|measured| {
            measured.trusted && age <= MAX_AGE && measured.offset_ms.abs() <= MAX_CORRECTION_MS
        })
    }

    /// `serverClock` as the device hello and the devices page give it.
    pub fn shown(&self, age: Duration) -> Value {
        let measured = self.measured;
        json!({
            "checkedAt": self.checked_at_ms,
            "offsetMs": measured.map(|measured| measured.offset_ms),
            "source": measured.map(|measured| measured.source),
            "trusted": measured.is_some_and(|measured| measured.trusted),
            "thresholdMs": self.threshold_ms,
            "wrong": self.wrong(),
            "corrected": self.correction(age).is_some(),
        })
    }
}

/// How the check runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub ntp_servers: Vec<String>,
    /// Only `https://` addresses: the `Date` is trusted for the certificate.
    pub https_urls: Vec<String>,
    pub threshold_ms: i64,
}

impl Settings {
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let enabled = lookup("CLOCK_CHECK_ENABLED").is_none_or(|raw| {
            !matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        });
        let list = |key: &str, default: &str| -> Vec<String> {
            lookup(key)
                .unwrap_or_else(|| default.to_string())
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        };
        let https_urls = list("CLOCK_CHECK_HTTPS_URLS", DEFAULT_HTTPS_URLS)
            .into_iter()
            .filter(|url| {
                let https = url.to_ascii_lowercase().starts_with("https://");
                if !https {
                    tracing::warn!(%url, "[clock] CLOCK_CHECK_HTTPS_URLS takes https:// addresses only; skipped");
                }
                https
            })
            .collect();
        let threshold_secs = lookup("CLOCK_CHECK_THRESHOLD_SECS")
            .and_then(|raw| raw.trim().parse::<i64>().ok())
            .filter(|secs| *secs > 0)
            .unwrap_or(DEFAULT_THRESHOLD_SECS);
        Self {
            enabled,
            ntp_servers: list("CLOCK_CHECK_NTP_SERVERS", DEFAULT_NTP_SERVERS),
            https_urls,
            threshold_ms: threshold_secs.saturating_mul(1000),
        }
    }
}

/// What the answers say together. SNTP servers that agree (two or more
/// within [`AGREE_MS`] of their median) are trusted and the most precise;
/// else an HTTPS `Date`, trusted; else one SNTP answer (or several that do
/// not agree), shown but not trusted.
pub fn combine(sntp: &[i64], https: Option<i64>) -> Option<Measurement> {
    let median = median(sntp);
    if let Some(median) = median {
        let agreeing = sntp
            .iter()
            .filter(|offset| (**offset - median).abs() <= AGREE_MS)
            .count();
        if agreeing >= 2 {
            return Some(Measurement {
                offset_ms: median,
                source: Source::Sntp,
                trusted: true,
            });
        }
    }
    if let Some(offset_ms) = https {
        return Some(Measurement {
            offset_ms,
            source: Source::Https,
            trusted: true,
        });
    }
    median.map(|offset_ms| Measurement {
        offset_ms,
        source: Source::Sntp,
        trusted: false,
    })
}

fn median(values: &[i64]) -> Option<i64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    match sorted.len() {
        0 => None,
        n if n % 2 == 1 => Some(sorted[n / 2]),
        n => Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2),
    }
}

/// The site's clock minus real time, from one SNTP exchange (all in
/// microseconds since the Unix epoch): sent at `t1` and received at `t4` by
/// the site's clock, received at `t2` and answered at `t3` by the server's.
pub fn sntp_offset_ms(t1: i64, t2: i64, t3: i64, t4: i64) -> i64 {
    // RFC 4330: the server's clock minus ours is ((t2 - t1) + (t3 - t4)) / 2.
    let server_minus_site = ((t2 - t1) + (t3 - t4)) / 2;
    (-server_minus_site as f64 / 1000.0).round() as i64
}

/// The site's clock minus real time, from an HTTPS `Date` (`date_secs`,
/// truncated to the second) of an answer sent at `sent_ms` and received at
/// `received_ms` by the site's clock.
pub fn https_offset_ms(date_secs: i64, sent_ms: i64, received_ms: i64) -> i64 {
    let midway = sent_ms + (received_ms - sent_ms) / 2;
    // The server's second began up to a second before it wrote the header.
    midway - (date_secs * 1000 + 500)
}

/// An NTP timestamp (seconds since 1900 in the high 32 bits, a binary
/// fraction in the low 32) for `unix_micros`.
fn ntp_timestamp(unix_micros: i64) -> u64 {
    let secs = unix_micros.div_euclid(1_000_000) + NTP_UNIX_SECS;
    let micros = unix_micros.rem_euclid(1_000_000) as u64;
    ((secs as u64 & 0xffff_ffff) << 32) | ((micros << 32) / 1_000_000)
}

/// Unix microseconds for an NTP timestamp. Seconds with the high bit clear
/// are in the next era (from 2036), as RFC 4330 reads them.
fn unix_micros(ntp: u64) -> i64 {
    let secs = (ntp >> 32) as i64;
    let secs = if secs & 0x8000_0000 == 0 {
        secs + (1 << 32)
    } else {
        secs
    };
    let micros = (((ntp & 0xffff_ffff) * 1_000_000) >> 32) as i64;
    (secs - NTP_UNIX_SECS) * 1_000_000 + micros
}

/// An SNTP v4 client request whose transmit timestamp is `t1`.
pub fn sntp_request(t1_micros: i64) -> [u8; 48] {
    let mut packet = [0u8; 48];
    // LI 0, version 4, mode 3 (client).
    packet[0] = 0x23;
    packet[40..48].copy_from_slice(&ntp_timestamp(t1_micros).to_be_bytes());
    packet
}

/// The server's receive and transmit times (Unix microseconds) from its
/// reply to `request`, or why the reply is not one.
pub fn sntp_reply(reply: &[u8], request: &[u8; 48]) -> Result<(i64, i64), &'static str> {
    if reply.len() < 48 {
        return Err("short reply");
    }
    let mode = reply[0] & 0x07;
    if mode != 4 {
        return Err("not a server reply");
    }
    if reply[0] >> 6 == 3 {
        return Err("server is not synchronized");
    }
    let stratum = reply[1];
    if stratum == 0 || stratum > 15 {
        return Err("server refused (kiss of death) or is not synchronized");
    }
    // The originate timestamp echoes ours: a reply to this request.
    if reply[24..32] != request[40..48] {
        return Err("reply is not to this request");
    }
    let stamp = |at: usize| u64::from_be_bytes(reply[at..at + 8].try_into().unwrap_or([0; 8]));
    let (received, transmitted) = (stamp(32), stamp(40));
    if transmitted == 0 {
        return Err("reply has no transmit time");
    }
    Ok((unix_micros(received), unix_micros(transmitted)))
}

fn now_micros() -> i64 {
    chrono::Utc::now().timestamp_micros()
}

/// One SNTP query to `server` (port 123).
async fn sntp(server: &str) -> Result<i64, String> {
    use tokio::net::UdpSocket;
    use tokio::time::timeout;
    let addr = timeout(TIMEOUT, tokio::net::lookup_host((server, 123)))
        .await
        .map_err(|_| "lookup timed out".to_string())?
        .map_err(|error| format!("lookup failed: {error}"))?
        .next()
        .ok_or_else(|| "no address".to_string())?;
    let socket = UdpSocket::bind(if addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })
    .await
    .map_err(|error| error.to_string())?;
    socket
        .connect(addr)
        .await
        .map_err(|error| error.to_string())?;
    let t1 = now_micros();
    let request = sntp_request(t1);
    socket
        .send(&request)
        .await
        .map_err(|error| error.to_string())?;
    let mut buf = [0u8; 128];
    let n = timeout(TIMEOUT, socket.recv(&mut buf))
        .await
        .map_err(|_| "no reply (UDP 123 may be blocked)".to_string())?
        .map_err(|error| error.to_string())?;
    let t4 = now_micros();
    let (t2, t3) = sntp_reply(&buf[..n], &request).map_err(str::to_string)?;
    Ok(sntp_offset_ms(t1, t2, t3, t4))
}

/// One HTTPS `HEAD` to `url`, read for its `Date`.
async fn https_date(client: &reqwest::Client, url: &str) -> Result<i64, String> {
    let sent = chrono::Utc::now().timestamp_millis();
    let response = client
        .head(url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|error| error.without_url().to_string())?;
    let received = chrono::Utc::now().timestamp_millis();
    let date = response
        .headers()
        .get(reqwest::header::DATE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "no Date header".to_string())?;
    let date = chrono::DateTime::parse_from_rfc2822(date)
        .map_err(|_| "Date header is not a date".to_string())?;
    Ok(https_offset_ms(date.timestamp(), sent, received))
}

/// Check the clock once: every SNTP server at once, then the HTTPS
/// addresses in order until one answers.
pub async fn check(settings: &Settings) -> Check {
    let queries = settings.ntp_servers.iter().map(|server| async move {
        let answer = sntp(server).await;
        if let Err(error) = &answer {
            tracing::debug!(%server, %error, "[clock] SNTP query failed");
        }
        answer.ok()
    });
    let sntp: Vec<i64> = futures::future::join_all(queries)
        .await
        .into_iter()
        .flatten()
        .collect();
    let mut https = None;
    if combine(&sntp, None).is_none_or(|measured| !measured.trusted) {
        let client = super::http_client::get_global_client().await;
        for url in &settings.https_urls {
            match https_date(&client, url).await {
                Ok(offset) => {
                    https = Some(offset);
                    break;
                }
                Err(error) => tracing::debug!(%url, %error, "[clock] HTTPS Date failed"),
            }
        }
    }
    Check {
        checked_at_ms: chrono::Utc::now().timestamp_millis(),
        measured: combine(&sntp, https),
        threshold_ms: settings.threshold_ms,
    }
}

struct Last {
    check: Check,
    at: Instant,
}

static LAST: RwLock<Option<Last>> = RwLock::new(None);

/// The last check and how old it is; None while the check is off or has
/// not run yet.
pub fn last() -> Option<(Check, Duration)> {
    LAST.read()
        .ok()?
        .as_ref()
        .map(|last| (last.check.clone(), last.at.elapsed()))
}

/// `serverClock`: the last check, or null while the check is off or has
/// not run yet.
pub fn shown() -> Value {
    last().map_or(Value::Null, |(check, age)| check.shown(age))
}

/// Real time as the site best knows it, in Unix microseconds, for comparing
/// a device's clock: this host's clock moved by a [`Check::correction`],
/// else this host's clock as it is. Also which (`clock`, `sntp`, `https`).
pub fn real_now_micros() -> (i64, &'static str) {
    real_now_at(now_micros(), last())
}

/// [`real_now_micros`] for this host's clock at `host_micros` and the last
/// check (with its age).
pub fn real_now_at(host_micros: i64, last: Option<(Check, Duration)>) -> (i64, &'static str) {
    match last.and_then(|(check, age)| check.correction(age)) {
        Some(measured) => (
            host_micros - measured.offset_ms.saturating_mul(1000),
            measured.source.name(),
        ),
        None => (host_micros, "clock"),
    }
}

/// "10 min 27 s" for the log.
fn amount(ms: i64) -> String {
    let secs = (ms.abs() + 500) / 1000;
    match (secs / 3600, secs % 3600 / 60, secs % 60) {
        (0, 0, s) => format!("{s} s"),
        (0, m, s) => format!("{m} min {s} s"),
        (h, m, _) => format!("{h} h {m} min"),
    }
}

/// Keep `check` as the last one and say what it found.
fn record(check: Check) {
    let was_wrong = last().map(|(check, _)| check.wrong());
    match check.measured {
        Some(measured) if check.wrong() => {
            let way = if measured.offset_ms < 0 {
                "behind"
            } else {
                "ahead of"
            };
            tracing::warn!(
                offset_ms = measured.offset_ms,
                source = measured.source.name(),
                trusted = measured.trusted,
                "[clock] the site's clock is {} {way} real time. Devices with the right time \
                 are refused as ahead and reminders are off. A Docker container keeps its \
                 host's clock: set it right on the host (turn NTP on; on Docker Desktop, \
                 restart Docker Desktop). The deployment's time-sync service sets it when it \
                 can; it needs SYS_TIME, which OpenVZ/LXC hosts do not give.",
                amount(measured.offset_ms),
            );
            if !(measured.trusted && measured.offset_ms.abs() <= MAX_CORRECTION_MS) {
                tracing::warn!(
                    "[clock] the measurement is not trusted enough to judge device clocks by \
                     (one SNTP answer, or too far off): devices are compared with the site's \
                     own clock"
                );
            }
        }
        Some(measured) => {
            if was_wrong != Some(false) {
                tracing::info!(
                    offset_ms = measured.offset_ms,
                    source = measured.source.name(),
                    "[clock] the site's clock is right"
                );
            }
        }
        None => {
            if was_wrong.is_none() || was_wrong == Some(true) {
                tracing::info!(
                    "[clock] could not check the site's clock: no SNTP server or HTTPS address \
                     answered"
                );
            }
        }
    }
    if let Ok(mut last) = LAST.write() {
        *last = Some(Last {
            check,
            at: Instant::now(),
        });
    }
}

/// Check the clock now and every [`EVERY`], unless it is switched off.
pub fn start() {
    let settings = Settings::from_env();
    if !settings.enabled {
        tracing::info!("[clock] the clock check is off (CLOCK_CHECK_ENABLED)");
        return;
    }
    if settings.ntp_servers.is_empty() && settings.https_urls.is_empty() {
        tracing::info!("[clock] the clock check has no time source configured");
        return;
    }
    super::jobs::jobs().periodic("clock check", super::jobs::Every::new(EVERY), move || {
        let settings = settings.clone();
        async move { record(check(&settings).await) }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: i64 = 1_761_200_000_000_000;

    fn check(offset_ms: i64, trusted: bool) -> Check {
        Check {
            checked_at_ms: AT / 1000,
            measured: Some(Measurement {
                offset_ms,
                source: if trusted { Source::Https } else { Source::Sntp },
                trusted,
            }),
            threshold_ms: 60_000,
        }
    }

    #[test]
    fn sntp_offset_is_the_sites_clock_minus_real_time() {
        // The site is 627 s behind; 40 ms each way, 2 ms at the server.
        let behind = 627_000_000;
        let t1 = AT;
        let t2 = AT + behind + 40_000;
        let t3 = t2 + 2_000;
        let t4 = t1 + 82_000;
        assert_eq!(sntp_offset_ms(t1, t2, t3, t4), -627_000);
        // Ahead by 90 s.
        let t2 = AT - 90_000_000 + 40_000;
        assert_eq!(sntp_offset_ms(t1, t2, t2 + 2_000, t4), 90_000);
    }

    #[test]
    fn https_offset_reads_the_date_from_the_middle_of_its_second() {
        let real_ms = AT / 1000;
        // Sent 300 ms before, received 300 ms after real time, on a clock
        // 627 s behind; the server wrote the second that began 200 ms before.
        let site = real_ms - 627_000;
        let offset = https_offset_ms((real_ms - 200) / 1000, site - 300, site + 300);
        assert!((offset + 627_000).abs() <= 500, "{offset}");
    }

    #[test]
    fn sntp_packets_round_trip() {
        let request = sntp_request(AT);
        assert_eq!(request[0], 0x23);
        let mut reply = [0u8; 48];
        reply[0] = 0x24; // LI 0, version 4, mode 4
        reply[1] = 2;
        reply[24..32].copy_from_slice(&request[40..48]);
        reply[32..40].copy_from_slice(&ntp_timestamp(AT + 10).to_be_bytes());
        reply[40..48].copy_from_slice(&ntp_timestamp(AT + 20).to_be_bytes());
        let (received, transmitted) = sntp_reply(&reply, &request).unwrap();
        assert!((received - (AT + 10)).abs() <= 1);
        assert!((transmitted - (AT + 20)).abs() <= 1);

        let mut other = reply;
        other[24] ^= 1;
        assert!(
            sntp_reply(&other, &request).is_err(),
            "another request's reply"
        );
        let mut kiss = reply;
        kiss[1] = 0;
        assert!(sntp_reply(&kiss, &request).is_err(), "kiss of death");
        let mut client = reply;
        client[0] = 0x23;
        assert!(sntp_reply(&client, &request).is_err(), "not a server");
        let mut alarm = reply;
        alarm[0] = 0xe4;
        assert!(sntp_reply(&alarm, &request).is_err(), "unsynchronized");
        assert!(sntp_reply(&reply[..47], &request).is_err());
    }

    #[test]
    fn ntp_eras() {
        // 2036-02-07 06:28:16 UTC is where the first era ends.
        let era_end = (1i64 << 32) - NTP_UNIX_SECS;
        for at in [0, AT, era_end * 1_000_000 + 5, (era_end + 3600) * 1_000_000] {
            assert!((unix_micros(ntp_timestamp(at)) - at).abs() <= 1, "{at}");
        }
    }

    #[test]
    fn answers_are_trusted_when_they_agree_or_come_over_https() {
        assert_eq!(
            combine(&[], None),
            None,
            "nothing answered: could not check"
        );
        let one = combine(&[-627_000], None).unwrap();
        assert_eq!((one.offset_ms, one.trusted), (-627_000, false));
        let agree = combine(&[-627_010, -626_900, 5_000], None).unwrap();
        assert_eq!(
            (agree.offset_ms, agree.source, agree.trusted),
            (-626_900, Source::Sntp, true)
        );
        let apart = combine(&[-627_000, 0], None).unwrap();
        assert!(!apart.trusted, "two that do not agree");
        let https = combine(&[-627_000], Some(-627_400)).unwrap();
        assert_eq!(
            (https.offset_ms, https.source, https.trusted),
            (-627_400, Source::Https, true)
        );
    }

    #[test]
    fn the_threshold_says_when_the_clock_is_wrong() {
        assert!(check(-627_000, true).wrong());
        assert!(check(61_000, false).wrong());
        assert!(!check(-60_000, true).wrong());
        assert!(!check(500, true).wrong());
        let unchecked = Check {
            measured: None,
            ..check(0, true)
        };
        assert!(!unchecked.wrong(), "could not check is not an offset");
    }

    #[test]
    fn only_a_trusted_fresh_believable_measurement_corrects() {
        let fresh = Duration::from_secs(60);
        assert!(check(-627_000, true).correction(fresh).is_some());
        assert!(
            check(-627_000, false).correction(fresh).is_none(),
            "untrusted"
        );
        assert!(
            check(-627_000, true)
                .correction(MAX_AGE + Duration::from_secs(1))
                .is_none(),
            "too old"
        );
        assert!(
            check(-(MAX_CORRECTION_MS + 1), true)
                .correction(fresh)
                .is_none(),
            "too far off to believe"
        );
    }

    #[test]
    fn server_clock_shape() {
        let fresh = Duration::from_secs(60);
        assert_eq!(
            check(-627_000, true).shown(fresh),
            json!({
                "checkedAt": AT / 1000,
                "offsetMs": -627_000,
                "source": "https",
                "trusted": true,
                "thresholdMs": 60_000,
                "wrong": true,
                "corrected": true,
            })
        );
        let unchecked = Check {
            measured: None,
            ..check(0, true)
        };
        assert_eq!(
            unchecked.shown(fresh),
            json!({
                "checkedAt": AT / 1000,
                "offsetMs": null,
                "source": null,
                "trusted": false,
                "thresholdMs": 60_000,
                "wrong": false,
                "corrected": false,
            })
        );
    }

    #[test]
    fn settings_from_the_environment() {
        let none = Settings::from_lookup(|_| None);
        assert!(none.enabled);
        assert_eq!(none.ntp_servers.len(), 3);
        assert_eq!(none.https_urls.len(), 2);
        assert_eq!(none.threshold_ms, 60_000);
        let set = Settings::from_lookup(|key| {
            match key {
                "CLOCK_CHECK_ENABLED" => Some(" Off "),
                "CLOCK_CHECK_NTP_SERVERS" => Some(""),
                "CLOCK_CHECK_HTTPS_URLS" => Some("http://plain.example, https://a.example "),
                "CLOCK_CHECK_THRESHOLD_SECS" => Some("120"),
                _ => None,
            }
            .map(str::to_string)
        });
        assert!(!set.enabled);
        assert!(set.ntp_servers.is_empty());
        assert_eq!(set.https_urls, ["https://a.example"]);
        assert_eq!(set.threshold_ms, 120_000);
        let bad = Settings::from_lookup(|key| {
            (key == "CLOCK_CHECK_THRESHOLD_SECS").then(|| "-5".to_string())
        });
        assert_eq!(bad.threshold_ms, 60_000);
    }

    #[test]
    fn amounts_for_the_log() {
        assert_eq!(amount(-627_400), "10 min 27 s");
        assert_eq!(amount(45_000), "45 s");
        assert_eq!(amount(3 * 3600 * 1000 + 61_000), "3 h 1 min");
    }

    /// Against the real servers (network): `cargo test -p myriad-backend
    /// server_clock::tests::live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live() {
        let settings = Settings::from_lookup(|_| None);
        for server in &settings.ntp_servers {
            println!("sntp {server}: {:?}", sntp(server).await);
        }
        let client = reqwest::Client::new();
        for url in &settings.https_urls {
            println!("https {url}: {:?}", https_date(&client, url).await);
        }
        let found = super::check(&settings).await;
        println!("{found:?}");
        assert!(found.measured.is_some());
    }
}
