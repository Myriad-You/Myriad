//! How strong a memory is: what she has brought to mind often and lately
//! comes easily; what she never thinks of fades, fast at first and then ever
//! more slowly. Forgetting follows a power of time, not an exponential
//! (Wixted & Ebbesen 1991), and each time a memory is recalled it is laid
//! down again. This is ACT-R's base-level activation, computed from what a
//! memory row already keeps: when it was made, how often it was recalled and
//! when last (Petrov 2006's approximation, one exact term for the last use).
//!
//! The decay 0.5 is ACT-R's usual default (uncertain; not checked against
//! her own recall).

use chrono::{DateTime, FixedOffset};

const DECAY: f64 = 0.5;
/// Anything younger counts as this old: the formula has no floor at zero.
const SHORTEST: f64 = 60.0;

/// Base-level activation of a memory made `lifetime` seconds ago, laid down
/// `uses` times in all (made, and recalled `uses - 1` times), last `since`
/// seconds ago.
pub fn base_level(uses: u32, lifetime: f64, since: f64) -> f64 {
    let lifetime = lifetime.max(SHORTEST);
    let since = since.clamp(SHORTEST, lifetime);
    let last = since.powf(-DECAY);
    if uses <= 1 || lifetime - since < 1.0 {
        return (last * f64::from(uses.max(1))).ln();
    }
    let earlier = f64::from(uses - 1) * (lifetime.powf(1.0 - DECAY) - since.powf(1.0 - DECAY))
        / ((1.0 - DECAY) * (lifetime - since));
    (last + earlier).ln()
}

/// How readily a memory comes to mind, in `0..1`, from its base level: a
/// memory fresh from being made or recalled is near the top.
pub fn readiness(base: f64) -> f64 {
    // Set so an untouched memory is ready about 0.9 after a day, a half
    // after a week, and under 0.1 after a year.
    const HALF_AT: f64 = -6.65;
    const SPREAD: f64 = 0.44;
    1.0 / (1.0 + ((HALF_AT - base) / SPREAD).exp())
}

/// How readily a stored memory comes to mind now.
pub fn of_row(
    created_at: DateTime<FixedOffset>,
    access_count: i32,
    last_accessed_at: Option<DateTime<FixedOffset>>,
    now: DateTime<FixedOffset>,
) -> f64 {
    let seconds = |at: DateTime<FixedOffset>| (now - at).num_seconds().max(0) as f64;
    let lifetime = seconds(created_at);
    let since = last_accessed_at.map(seconds).unwrap_or(lifetime);
    let uses = u32::try_from(access_count.max(0))
        .unwrap_or(0)
        .saturating_add(1);
    readiness(base_level(uses, lifetime, since))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: f64 = 3600.0;
    const DAY: f64 = 24.0 * HOUR;

    /// Least squares slope and fit of y on x.
    fn fit(points: &[(f64, f64)]) -> f64 {
        let n = points.len() as f64;
        let (mx, my) = points
            .iter()
            .fold((0.0, 0.0), |(x, y), (px, py)| (x + px / n, y + py / n));
        let (sxy, sxx, syy) = points.iter().fold((0.0, 0.0, 0.0), |(a, b, c), (x, y)| {
            (
                a + (x - mx) * (y - my),
                b + (x - mx).powi(2),
                c + (y - my).powi(2),
            )
        });
        (sxy * sxy) / (sxx * syy)
    }

    /// A memory never thought of again fades the way people forget: a
    /// straight line on log-log axes (a power law) fits how ready it is over
    /// a year much better than a straight line on log-linear ones (an
    /// exponential).
    #[test]
    fn an_untouched_memory_fades_as_a_power_of_time() {
        let ages: Vec<f64> = (0..60).map(|step| HOUR * 1.2f64.powi(step)).collect();
        let odds: Vec<(f64, f64)> = ages
            .iter()
            .map(|age| {
                let ready = readiness(base_level(1, *age, *age));
                (*age, (ready / (1.0 - ready)).ln())
            })
            .collect();
        let power = fit(&odds
            .iter()
            .map(|(age, y)| (age.ln(), *y))
            .collect::<Vec<_>>());
        let exponential = fit(&odds);
        assert!(power > 0.999, "power law r2 {power}");
        assert!(exponential < 0.8, "exponential r2 {exponential}");
        // Fast at first, then ever more slowly.
        let at = |age: f64| readiness(base_level(1, age, age));
        assert!(at(HOUR) - at(DAY) > at(30.0 * DAY) - at(60.0 * DAY));
        assert!(at(DAY) > 0.85);
        assert!((0.4..0.6).contains(&at(7.0 * DAY)));
        assert!(at(365.0 * DAY) < 0.1);
    }

    /// Thinking of something again lays it down again: a memory recalled a
    /// few times over a month is readier than one of the same age never
    /// recalled, and more so the more recently and often it came up.
    #[test]
    fn what_she_recalls_she_keeps() {
        let never = base_level(1, 60.0 * DAY, 60.0 * DAY);
        let once_long_ago = base_level(2, 60.0 * DAY, 45.0 * DAY);
        let once_lately = base_level(2, 60.0 * DAY, 2.0 * DAY);
        let often_lately = base_level(6, 60.0 * DAY, 2.0 * DAY);
        assert!(never < once_long_ago);
        assert!(once_long_ago < once_lately);
        assert!(once_lately < often_lately);
        assert!(readiness(often_lately) > 0.9 && readiness(never) < 0.3);
    }

    #[test]
    fn a_row_reads_its_own_history() {
        let now: DateTime<FixedOffset> = "2026-09-29T00:00:00+00:00".parse().unwrap();
        let made = now - chrono::Duration::days(30);
        let fresh = of_row(made, 3, Some(now - chrono::Duration::hours(1)), now);
        let stale = of_row(made, 0, None, now);
        assert!(fresh > 0.95 && stale < 0.4);
        // Nothing is readier than just made, and bad inputs stay in range.
        let just = of_row(now, 0, None, now);
        assert!(just > 0.99 && just <= 1.0);
        let odd = of_row(now + chrono::Duration::days(1), -4, Some(now), now);
        assert!((0.0..=1.0).contains(&odd));
    }
}
