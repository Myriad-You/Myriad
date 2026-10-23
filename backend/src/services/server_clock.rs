//! How far this host's clock is off, measured against the world.
//!
//! The site keeps time by its host's clock: a Docker container has no clock
//! of its own. A host clock that is minutes off does not look wrong from
//! here; every device looks wrong instead (refused as ahead,
//! `clock_ahead`), and reminders come early or late. So every process (web,
//! persona and federation workers: each answers its own routes) measures how
//! far its host's clock is off, at once at start, again within a minute when
//! that fails (backing off to [`EVERY`]), and every [`EVERY`] after: SNTP
//! (UDP 123) to each configured server, and the `Date` of an HTTPS answer
//! (certificate verified) for where UDP 123 is blocked.
//!
//! A measurement that is trusted, at most [`MAX_AGE`] old and at most
//! [`MAX_CORRECTION_MS`] off is the site's correction: [`crate::clock`], the
//! site's one clock, is this host's clock moved by it. Without one (could not
//! check, not trusted, too old, too far off) the clock is this host's, and
//! the log says why. The measurement is also `serverClock` in the device
//! hello and on the owner's devices page.
//!
//! Trusted is an HTTPS `Date` (the certificate vouches for who sent it), or
//! two SNTP servers that agree; one SNTP answer alone is unauthenticated UDP
//! and only shown. When nothing answers, the check is "could not check",
//! never an offset, and the log names what each source answered.
//!
//! HTTPS goes the way the site's other outbound requests go: through the
//! outbound proxy when one is set (never around it), directly otherwise.
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
/// How soon a check that failed is tried again (then doubling to [`EVERY`]).
pub const RETRY: Duration = Duration::from_secs(60);
/// How long a measurement corrects the clock.
pub const MAX_AGE: Duration = Duration::from_secs(45 * 60);
/// An offset beyond this is not believed: the clock is not moved by it, so a
/// forged time cannot pull the site far.
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
    /// By the site's clock, milliseconds since the epoch.
    pub checked_at_ms: i64,
    /// None: nothing answered (could not check).
    pub measured: Option<Measurement>,
    pub threshold_ms: i64,
    /// What each source that did not answer said (`sntp time.google.com: …`).
    pub failures: Vec<String>,
}

impl Check {
    /// Off by more than the threshold.
    pub fn wrong(&self) -> bool {
        self.measured
            .is_some_and(|measured| measured.offset_ms.abs() > self.threshold_ms)
    }

    /// The measurement the site's clock is corrected by: only a trusted one,
    /// `age` at most [`MAX_AGE`] old and at most [`MAX_CORRECTION_MS`] off.
    pub fn correction(&self, age: Duration) -> Option<Measurement> {
        self.applied(age).ok()
    }

    /// [`Check::correction`], or why there is none.
    pub fn applied(&self, age: Duration) -> Result<Measurement, &'static str> {
        let measured = self
            .measured
            .ok_or("could not check: no time source answered")?;
        if !measured.trusted {
            return Err(
                "the measurement is not trusted (one SNTP answer, or answers that disagree)",
            );
        }
        if age > MAX_AGE {
            return Err("the last good measurement is too old");
        }
        if measured.offset_ms.abs() > MAX_CORRECTION_MS {
            return Err("the offset is too far off to believe (over 3 hours)");
        }
        Ok(measured)
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
            "maxCorrectionMs": MAX_CORRECTION_MS,
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

/// The host's own clock: a measurement is of it, never of the corrected one.
fn now_micros() -> i64 {
    crate::clock::host_micros()
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
    let sent = now_micros() / 1000;
    let response = client
        .head(url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|error| reason(&error.without_url()))?;
    let received = now_micros() / 1000;
    let date = response
        .headers()
        .get(reqwest::header::DATE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "no Date header".to_string())?;
    let date = chrono::DateTime::parse_from_rfc2822(date)
        .map_err(|_| "Date header is not a date".to_string())?;
    Ok(https_offset_ms(date.timestamp(), sent, received))
}

/// An error with what caused it, as one line.
fn reason(error: &dyn std::error::Error) -> String {
    let mut line = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        line.push_str(": ");
        line.push_str(&cause.to_string());
        source = cause.source();
    }
    line
}

/// The client HTTPS `Date` is read with: through the outbound proxy when one
/// is set (fail closed, never around it), else directly, ignoring proxy
/// environment variables.
async fn https_client() -> Result<reqwest::Client, String> {
    use super::http_client::{ProxyConfig, create_client_no_proxy, create_client_with_proxy};
    let proxy = ProxyConfig::from_dynamic_config().await;
    if proxy.should_use_proxy() {
        create_client_with_proxy(&proxy)
            .map_err(|error| format!("outbound proxy: {}", reason(&error)))
    } else {
        create_client_no_proxy().map_err(|error| reason(&error))
    }
}

/// Check the clock once: every SNTP server at once, then, unless they
/// agree, the HTTPS addresses in order until one answers.
pub async fn check(settings: &Settings) -> Check {
    let client = https_client().await;
    check_with(settings, client).await
}

/// [`check`] with the HTTPS client given (or why there is none).
pub async fn check_with(settings: &Settings, client: Result<reqwest::Client, String>) -> Check {
    let mut failures = Vec::new();
    let queries = settings
        .ntp_servers
        .iter()
        .map(|server| async move { (server, sntp(server).await) });
    let mut sntp = Vec::new();
    for (server, answer) in futures::future::join_all(queries).await {
        match answer {
            Ok(offset) => sntp.push(offset),
            Err(error) => failures.push(format!("sntp {server}: {error}")),
        }
    }
    let mut https = None;
    if !settings.https_urls.is_empty()
        && combine(&sntp, None).is_none_or(|measured| !measured.trusted)
    {
        match &client {
            Err(error) => failures.push(format!("https: no client: {error}")),
            Ok(client) => {
                for url in &settings.https_urls {
                    match https_date(client, url).await {
                        Ok(offset) => {
                            https = Some(offset);
                            break;
                        }
                        Err(error) => failures.push(format!("https {url}: {error}")),
                    }
                }
            }
        }
    }
    Check {
        checked_at_ms: crate::clock::now_micros() / 1000,
        measured: combine(&sntp, https),
        threshold_ms: settings.threshold_ms,
        failures,
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

/// What the clock is corrected by now: the source while a correction holds.
static APPLIED: std::sync::Mutex<Option<Source>> = std::sync::Mutex::new(None);

/// How far [`crate::clock`] moves this host's clock now, microseconds: the
/// correction while one holds, else 0. Logs when that changes.
pub fn offset_micros() -> i64 {
    let last = last();
    let applied = match &last {
        Some((check, age)) => check.applied(*age),
        None => Err("no check has run yet"),
    };
    let now = applied.ok().map(|measured| measured.source);
    if let Ok(mut was) = APPLIED.lock()
        && *was != now
    {
        note_change(*was, &applied);
        *was = now;
    }
    applied.map_or(0, |measured| -measured.offset_ms.saturating_mul(1000))
}

/// Where the clock's time comes from now: `sntp` or `https` while it is
/// corrected, `clock` (this host's, as it is) otherwise.
pub fn source() -> &'static str {
    match last().and_then(|(check, age)| check.correction(age)) {
        Some(measured) => measured.source.name(),
        None => "clock",
    }
}

/// Say that the correction started, or ended and why.
fn note_change(was: Option<Source>, now: &Result<Measurement, &'static str>) {
    match now {
        Ok(measured) => tracing::info!(
            offset_ms = measured.offset_ms,
            source = measured.source.name(),
            "[clock] the site's clock now follows real time: this host's clock is {} {}",
            amount(measured.offset_ms),
            if measured.offset_ms < 0 {
                "behind"
            } else {
                "ahead"
            },
        ),
        Err(why) if was.is_some() => tracing::warn!(
            "[clock] the site's clock is this host's own again ({why}); it goes on from where \
             it was, slowly, never back"
        ),
        Err(_) => {}
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

/// Keep `check` as the last one, unless it has no trusted answer and a good
/// one still corrects the clock, and say what it found. Whether it was
/// trusted.
fn record(check: Check) -> bool {
    let trusted = check.measured.is_some_and(|measured| measured.trusted);
    if !check.failures.is_empty() && !trusted {
        tracing::warn!(
            failures = %check.failures.join("; "),
            "[clock] could not measure this host's clock against a trusted source; trying again \
             soon"
        );
    } else if !check.failures.is_empty() {
        tracing::debug!(failures = %check.failures.join("; "), "[clock] some time sources did not answer");
    }
    match check.measured {
        Some(measured) if check.wrong() => {
            let corrected = measured.trusted && measured.offset_ms.abs() <= MAX_CORRECTION_MS;
            tracing::warn!(
                offset_ms = measured.offset_ms,
                source = measured.source.name(),
                trusted = measured.trusted,
                "[clock] this host's clock is {} {} real time; {}",
                amount(measured.offset_ms),
                if measured.offset_ms < 0 {
                    "behind"
                } else {
                    "ahead of"
                },
                if corrected {
                    "the site corrects for it. Turning NTP on on the host fixes it at the root."
                } else {
                    "the site cannot correct for it (not trusted, or over 3 hours off): devices \
                     with the right time are refused and reminders are off. Turn NTP on on \
                     the host."
                },
            );
        }
        Some(measured) => tracing::debug!(
            offset_ms = measured.offset_ms,
            source = measured.source.name(),
            "[clock] this host's clock is right"
        ),
        None => {}
    }
    if let Ok(mut last) = LAST.write() {
        // A check without a trusted answer does not throw away a good
        // measurement that still corrects the clock; that ages out by
        // itself (MAX_AGE).
        let keep_old = keeps_old(
            trusted,
            last.as_ref().map(|last| (&last.check, last.at.elapsed())),
        );
        if !keep_old {
            *last = Some(Last {
                check,
                at: Instant::now(),
            });
        }
    }
    trusted
}

/// Whether a check that was `trusted` or not leaves `old` (and its age) as
/// the last one: only an untrusted one, while `old` still corrects the clock.
fn keeps_old(trusted: bool, old: Option<(&Check, Duration)>) -> bool {
    !trusted && old.is_some_and(|(old, age)| old.correction(age).is_some())
}

/// How long until the next check after `failures` failed ones in a row.
fn next_in(failures: u32) -> Duration {
    if failures == 0 {
        return EVERY;
    }
    RETRY.saturating_mul(1 << (failures - 1).min(8)).min(EVERY)
}

/// Check the clock now, again soon when that fails, and every [`EVERY`],
/// unless it is switched off. Every process does, for its own clock.
pub fn start() {
    let settings = Settings::from_env();
    if !settings.enabled {
        tracing::info!(
            "[clock] the clock check is off (CLOCK_CHECK_ENABLED); the site keeps this host's clock"
        );
        return;
    }
    if settings.ntp_servers.is_empty() && settings.https_urls.is_empty() {
        tracing::warn!(
            "[clock] the clock check has no time source configured; the site keeps this host's clock"
        );
        return;
    }
    let due = std::sync::Arc::new(std::sync::Mutex::new((Instant::now(), 0u32)));
    super::jobs::jobs().periodic("clock check", super::jobs::Every::new(RETRY), move || {
        let settings = settings.clone();
        let due = due.clone();
        async move {
            let failures = match due.lock() {
                Ok(due) if Instant::now() < due.0 => return,
                Ok(due) => due.1,
                Err(_) => 0,
            };
            let failures = if record(check(&settings).await) {
                0
            } else {
                failures + 1
            };
            if let Ok(mut due) = due.lock() {
                *due = (Instant::now() + next_in(failures), failures);
            }
        }
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
            failures: vec![],
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
                "maxCorrectionMs": MAX_CORRECTION_MS,
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
                "maxCorrectionMs": MAX_CORRECTION_MS,
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

    /// A one-request HTTP server answering with `Date` set `ahead_secs` ahead
    /// of this host's clock (real time on a host that is behind).
    async fn date_server(ahead_secs: i64) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf).await;
                let real = chrono::DateTime::from_timestamp_micros(now_micros()).unwrap()
                    + chrono::Duration::seconds(ahead_secs);
                let date = real.format("%a, %d %b %Y %H:%M:%S GMT");
                let answer = format!(
                    "HTTP/1.1 200 OK\r\nDate: {date}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(answer.as_bytes()).await;
            }
        });
        format!("http://{address}/")
    }

    fn only_https(url: String) -> Settings {
        Settings {
            enabled: true,
            ntp_servers: vec![],
            https_urls: vec![url],
            threshold_ms: 60_000,
        }
    }

    /// The host 10 minutes behind and an HTTPS time source that answers:
    /// the site's clock is right, a device that is right gets in, and one
    /// 6 minutes ahead of real time is still refused.
    #[tokio::test]
    async fn a_host_ten_minutes_behind_is_corrected() {
        let url = date_server(600).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let found = check_with(&only_https(url), Ok(client)).await;
        assert!(found.failures.is_empty(), "{:?}", found.failures);
        let measured = found.applied(Duration::from_secs(1)).unwrap();
        assert_eq!((measured.source, measured.trusted), (Source::Https, true));
        assert!(
            (measured.offset_ms + 600_000).abs() <= 1_500,
            "{measured:?}"
        );

        let host = now_micros();
        let real = host + 600 * 1_000_000;
        let mut state = None;
        let site = crate::clock::step(&mut state, host, -measured.offset_ms * 1000);
        assert!(
            (site - real).abs() <= 1_500_000,
            "the site's clock is real time"
        );

        let device = |micros: i64| {
            myriad_persona_sync::Rev::new((micros / 1000) as u64, 0, "dev_test").unwrap()
        };
        let hlc = myriad_persona_sync::hlc::check_ahead;
        assert!(
            hlc(&device(real), site).is_ok(),
            "a device that is right gets in"
        );
        assert!(hlc(&device(real), host).is_err(), "as it did not before");
        assert!(
            hlc(&device(real + 6 * 60 * 1_000_000), site).is_err(),
            "one 6 minutes ahead of real time is still refused"
        );
    }

    /// An outbound proxy that refuses and an SNTP server that is not there:
    /// could not check, and what each source said is kept for the log.
    #[tokio::test]
    async fn a_refusing_proxy_and_no_sntp_are_named() {
        let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = proxy.local_addr().unwrap();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            while let Ok((mut stream, _)) = proxy.accept().await {
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf).await;
                let _ = stream
                    .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
                    .await;
            }
        });
        let client = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(format!("http://{proxy_address}")).unwrap())
            .build()
            .unwrap();
        let settings = Settings {
            enabled: true,
            ntp_servers: vec!["127.0.0.1".into()],
            https_urls: vec!["https://www.cloudflare.com".into()],
            threshold_ms: 60_000,
        };
        let found = check_with(&settings, Ok(client)).await;
        assert_eq!(found.measured, None, "could not check is not an offset");
        assert_eq!(found.failures.len(), 2, "{:?}", found.failures);
        assert!(
            found.failures[0].starts_with("sntp 127.0.0.1: "),
            "{:?}",
            found.failures
        );
        assert!(
            found.failures[1].starts_with("https https://www.cloudflare.com: "),
            "{:?}",
            found.failures
        );
        assert_eq!(
            found.applied(Duration::ZERO),
            Err("could not check: no time source answered")
        );

        let without = check_with(
            &only_https("https://a.example".into()),
            Err("outbound proxy: bad url".into()),
        )
        .await;
        assert_eq!(
            without.failures,
            ["https: no client: outbound proxy: bad url"]
        );
    }

    #[test]
    fn without_a_correction_the_clock_is_the_hosts_and_the_log_says_why() {
        assert_eq!(
            check(-600_000, true).applied(MAX_AGE + Duration::from_secs(1)),
            Err("the last good measurement is too old")
        );
        assert!(check(-600_000, false).applied(Duration::ZERO).is_err());
        assert!(
            check(-(MAX_CORRECTION_MS + 1), true)
                .applied(Duration::ZERO)
                .is_err()
        );

        let logs = capture(|| {
            note_change(
                Some(Source::Https),
                &Err("the last good measurement is too old"),
            );
            note_change(None, &Err("no check has run yet"));
        });
        assert!(logs.contains("WARN"), "{logs}");
        assert!(
            logs.contains("this host's own again (the last good measurement is too old)"),
            "{logs}"
        );
        assert_eq!(
            logs.matches("WARN").count(),
            1,
            "nothing to say when it was not corrected: {logs}"
        );
    }

    #[test]
    fn a_check_without_a_trusted_answer_keeps_a_good_measurement() {
        let good = check(-600_000, true);
        assert!(keeps_old(false, Some((&good, Duration::from_secs(60)))));
        assert!(!keeps_old(true, Some((&good, Duration::from_secs(60)))));
        assert!(!keeps_old(
            false,
            Some((&good, MAX_AGE + Duration::from_secs(1)))
        ));
        assert!(!keeps_old(false, None));
    }

    #[test]
    fn a_failed_check_is_tried_again_soon() {
        assert_eq!(next_in(0), EVERY);
        assert_eq!(next_in(1), RETRY);
        assert_eq!(next_in(2), RETRY * 2);
        assert_eq!(next_in(4), RETRY * 8);
        assert_eq!(next_in(30), EVERY);
    }

    /// What `f` logs, as text.
    fn capture(f: impl FnOnce()) -> String {
        use std::sync::{Arc, Mutex};
        #[derive(Clone)]
        struct Buffer(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buffer = Buffer(Arc::new(Mutex::new(Vec::new())));
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, f);
        String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap()
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
