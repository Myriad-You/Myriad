//! The site's one clock.
//!
//! Every wall-clock reading in the backend comes from here, never from
//! `Utc::now()` (clippy refuses it, see `backend/clippy.toml`): this host's
//! clock moved by the offset the site measured of it
//! (`services::server_clock`), so a host that is minutes behind does not
//! make the site minutes behind. SQL takes its time from here too, as a
//! parameter, never `now()`.
//!
//! The clock only goes forward. A correction that moves it forward is taken
//! at once; one that would move it back (a smaller correction, the
//! correction ending, the host's clock stepping back) is not: the clock runs
//! at half speed until it is where it should be. Without a correction it is
//! this host's clock, as it was.
//!
//! What it is not for: measuring durations (`Instant`), and file times the
//! file system stamps with the host's clock.

use std::sync::Mutex;

use chrono::{DateTime, Utc};

pub(crate) struct State {
    /// The host's clock at the last reading, microseconds.
    host: i64,
    /// What the last reading gave.
    out: i64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// This host's own clock: only for measuring it, and for the clock itself.
#[allow(clippy::disallowed_methods)]
pub fn host_micros() -> i64 {
    Utc::now().timestamp_micros()
}

/// One reading: the host's clock `host` moved by `offset` (both
/// microseconds), never before the last reading, and catching up at half
/// speed while it is ahead of where it should be.
pub(crate) fn step(last: &mut Option<State>, host: i64, offset: i64) -> i64 {
    let target = host.saturating_add(offset);
    let out = match last {
        None => target,
        Some(last) => {
            let elapsed = (host - last.host).max(0);
            target.max(last.out + elapsed / 2)
        }
    };
    *last = Some(State { host, out });
    out
}

/// Now, microseconds since the epoch.
pub fn now_micros() -> i64 {
    let offset = crate::services::server_clock::offset_micros();
    let host = host_micros();
    match STATE.lock() {
        Ok(mut state) => step(&mut state, host, offset),
        Err(poisoned) => step(&mut poisoned.into_inner(), host, offset),
    }
}

/// Now.
pub fn now() -> DateTime<Utc> {
    DateTime::from_timestamp_micros(now_micros()).unwrap_or_default()
}

/// Now as `SystemTime`, for what takes one.
pub fn now_system() -> std::time::SystemTime {
    std::time::UNIX_EPOCH + std::time::Duration::from_micros(now_micros().max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: i64 = 1_000_000;
    const T: i64 = 1_761_200_000 * S;

    #[test]
    fn a_correction_forward_is_taken_at_once() {
        let mut state = None;
        assert_eq!(step(&mut state, T, 0), T);
        // The site measured its host 10 minutes behind.
        assert_eq!(step(&mut state, T + S, 600 * S), T + S + 600 * S);
        assert_eq!(step(&mut state, T + 2 * S, 600 * S), T + 2 * S + 600 * S);
    }

    #[test]
    fn it_never_goes_back() {
        let mut state = None;
        let mut last = step(&mut state, T, 600 * S);
        // The correction ends (expired, or too far off to believe): it runs
        // at half speed until the host's clock is where it was.
        let mut host = T;
        let mut steps = 0;
        while last > host {
            host += 10 * S;
            let out = step(&mut state, host, 0);
            assert!(out >= last, "went back from {last} to {out}");
            assert!(out - last <= 10 * S);
            last = out;
            steps += 1;
        }
        assert_eq!(last, host, "caught up");
        assert!(steps >= 100, "slowly: {steps} steps of 10 s for 600 s");
        // Then it follows the host again.
        assert_eq!(step(&mut state, host + S, 0), host + S);
    }

    #[test]
    fn a_host_clock_stepping_back_does_not_take_it_back() {
        let mut state = None;
        let before = step(&mut state, T, 0);
        let after = step(&mut state, T - 30 * S, 0);
        assert_eq!(after, before);
        assert!(step(&mut state, T - 20 * S, 0) > before);
    }

    /// Clippy refuses the host's clock outside this module
    /// (`backend/clippy.toml`), and no source reads it directly.
    #[test]
    fn nothing_reads_the_hosts_clock_but_the_clock() {
        let config = include_str!("../clippy.toml");
        for path in [
            "chrono::Utc::now",
            "chrono::Local::now",
            "std::time::SystemTime::now",
        ] {
            assert!(config.contains(&format!("path = \"{path}\"")), "{path}");
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let this = root.join("clock.rs");
        let mut stack = vec![root];
        let mut found = Vec::new();
        let banned = ["Utc", "::now()"].concat();
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs")
                    && path != this
                    && std::fs::read_to_string(&path).unwrap().contains(&banned)
                {
                    found.push(path.display().to_string());
                }
            }
        }
        assert!(
            found.is_empty(),
            "the host's clock read directly in {found:?}"
        );
    }

    #[test]
    fn a_smaller_correction_converges_without_going_back() {
        let mut state = None;
        let mut last = step(&mut state, T, 600 * S);
        let mut host = T;
        for _ in 0..200 {
            host += 5 * S;
            let out = step(&mut state, host, 590 * S);
            assert!(out >= last);
            last = out;
        }
        assert_eq!(last, host + 590 * S);
    }
}
