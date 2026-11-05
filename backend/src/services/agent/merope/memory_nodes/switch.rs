//! Whether her memory goes through her nodes: one switch, three states.
//!
//! - `off`: today's topic path (`agent_topic_nodes`); nothing here is
//!   written or read. The default.
//! - `shadow`: nodes are written as well, she still reads the old path, so
//!   the two can be compared before nodes reach what she says.
//! - `on`: she reads her nodes.
//!
//! It is a site setting (`DynamicConfig::merope_memory_nodes`, a row of
//! `configurations` seeded `off` like every default and read back by
//! `parse_config`), applied each time the configuration is installed
//! (`config_service::install_dynamic_config`: at start, after a save or a
//! restore, on the workers' reload), so changing it needs no restart. Turning
//! it back to `off` is the way back: the memory rows and the old topic
//! tables are written all along, the node tables only added to.
//!
//! `MEROPE_MEMORY_NODES` in the environment overrides it, for tests and
//! benches only.

use std::sync::atomic::{AtomicU8, Ordering};

/// The environment override, for tests and benches.
pub(crate) const ENV: &str = "MEROPE_MEMORY_NODES";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Off,
    Shadow,
    On,
}

impl Mode {
    /// The setting's value; anything else is not a mode.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "shadow" => Some(Self::Shadow),
            "on" => Some(Self::On),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Shadow => "shadow",
            Self::On => "on",
        }
    }

    /// Whether node rows are written.
    pub(crate) fn writes(self) -> bool {
        self != Self::Off
    }

    /// Whether she reads her nodes instead of the old path.
    pub(crate) fn reads(self) -> bool {
        self == Self::On
    }

    fn code(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Shadow => 1,
            Self::On => 2,
        }
    }

    fn of_code(code: u8) -> Self {
        match code {
            1 => Self::Shadow,
            2 => Self::On,
            _ => Self::Off,
        }
    }
}

/// The mode for an override and a setting: the override when it is one,
/// else the setting, else off (an empty or unknown setting reads as off).
pub(crate) fn resolve(env: Option<&str>, configured: &str) -> Mode {
    env.and_then(Mode::parse)
        .or_else(|| Mode::parse(configured))
        .unwrap_or(Mode::Off)
}

static CONFIGURED: AtomicU8 = AtomicU8::new(0);

/// Takes the setting from a configuration being installed.
pub(crate) fn apply_config(configured: &str) {
    let mode = resolve(None, configured);
    let before = Mode::of_code(CONFIGURED.swap(mode.code(), Ordering::SeqCst));
    if before != mode {
        tracing::info!(mode = mode.as_str(), "[memory-nodes] switch set");
    }
}

// Tests run side by side in one process, each on its own thread: each may
// set its own mode, so one test's switch never reaches another's.
#[cfg(test)]
thread_local! {
    static TEST_MODE: std::cell::Cell<Option<Mode>> = const { std::cell::Cell::new(None) };
}

/// This test's mode, until set back to `None`.
#[cfg(test)]
pub(crate) fn set_for_test(mode: Option<Mode>) {
    TEST_MODE.with(|held| held.set(mode));
}

/// The mode now.
pub(crate) fn mode() -> Mode {
    #[cfg(test)]
    if let Some(mode) = TEST_MODE.with(std::cell::Cell::get) {
        return mode;
    }
    let configured = Mode::of_code(CONFIGURED.load(Ordering::SeqCst));
    resolve(std::env::var(ENV).ok().as_deref(), configured.as_str())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::config::DynamicConfig;
    use crate::services::config_service::ConfigService;

    #[test]
    fn the_switch_is_off_until_it_is_set() {
        assert_eq!(
            resolve(None, &DynamicConfig::default().merope_memory_nodes),
            Mode::Off
        );
        // A database with no row for it, or a value that is not a mode.
        let unset = ConfigService::parse_config(HashMap::new());
        assert_eq!(resolve(None, &unset.merope_memory_nodes), Mode::Off);
        assert_eq!(resolve(None, ""), Mode::Off);
        assert_eq!(resolve(None, "maybe"), Mode::Off);
        assert!(!Mode::Off.writes() && !Mode::Off.reads());
    }

    #[test]
    fn the_setting_is_read_back_and_the_override_wins() {
        let stored = ConfigService::parse_config(HashMap::from([(
            "merope_memory_nodes".to_string(),
            serde_json::json!("shadow"),
        )]));
        assert_eq!(resolve(None, &stored.merope_memory_nodes), Mode::Shadow);
        assert!(Mode::Shadow.writes() && !Mode::Shadow.reads());
        assert_eq!(resolve(Some("on"), "shadow"), Mode::On);
        assert_eq!(resolve(Some(" ON "), "off"), Mode::On);
        // An override that is not a mode leaves the setting.
        assert_eq!(resolve(Some("yes"), "shadow"), Mode::Shadow);
    }
}
