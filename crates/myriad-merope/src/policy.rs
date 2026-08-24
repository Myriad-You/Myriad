use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutonomyFrequency {
    Low,
    #[default]
    Normal,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrequencyProfile {
    pub min_deliberate_minutes: u32,
    pub min_speak_minutes: u32,
    pub max_proactive_speaks_per_day: u16,
    pub decay_multiplier: f64,
    pub min_check_minutes: u32,
    pub max_check_minutes: u32,
}

impl AutonomyFrequency {
    pub const fn profile(self) -> FrequencyProfile {
        match self {
            Self::Low => FrequencyProfile {
                min_deliberate_minutes: 25,
                min_speak_minutes: 60,
                max_proactive_speaks_per_day: 3,
                decay_multiplier: 0.6,
                min_check_minutes: 25,
                max_check_minutes: 120,
            },
            Self::Normal => FrequencyProfile {
                min_deliberate_minutes: 12,
                min_speak_minutes: 30,
                max_proactive_speaks_per_day: 8,
                decay_multiplier: 1.0,
                min_check_minutes: 12,
                max_check_minutes: 90,
            },
            Self::High => FrequencyProfile {
                min_deliberate_minutes: 8,
                min_speak_minutes: 15,
                max_proactive_speaks_per_day: 16,
                decay_multiplier: 1.3,
                min_check_minutes: 8,
                max_check_minutes: 60,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeropePolicy {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub do_not_disturb: bool,
    #[serde(default = "default_true")]
    pub show_thought: bool,
    #[serde(default)]
    pub autonomy_frequency: AutonomyFrequency,
    #[serde(default = "default_max_speaks")]
    pub max_proactive_speaks_per_day: u16,
    #[serde(default = "default_min_speak")]
    pub min_speak_interval_minutes: u32,
    #[serde(default = "default_min_deliberate")]
    pub min_deliberate_interval_minutes: u32,
    #[serde(default = "default_true")]
    pub collapsed_by_default: bool,
}

impl Default for MeropePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            do_not_disturb: false,
            show_thought: true,
            autonomy_frequency: AutonomyFrequency::Normal,
            max_proactive_speaks_per_day: default_max_speaks(),
            min_speak_interval_minutes: default_min_speak(),
            min_deliberate_interval_minutes: default_min_deliberate(),
            collapsed_by_default: true,
        }
    }
}

impl MeropePolicy {
    pub fn normalized(mut self) -> Self {
        let profile = self.autonomy_frequency.profile();
        self.max_proactive_speaks_per_day = self.max_proactive_speaks_per_day.clamp(0, 48);
        self.min_speak_interval_minutes = self.min_speak_interval_minutes.clamp(5, 24 * 60);
        self.min_deliberate_interval_minutes =
            self.min_deliberate_interval_minutes.clamp(5, 24 * 60);
        if self.max_proactive_speaks_per_day == 0 {
            self.do_not_disturb = true;
        }
        if self.min_speak_interval_minutes == 0 {
            self.min_speak_interval_minutes = profile.min_speak_minutes;
        }
        self
    }

    pub fn apply_frequency_defaults(&mut self) {
        let profile = self.autonomy_frequency.profile();
        self.max_proactive_speaks_per_day = profile.max_proactive_speaks_per_day;
        self.min_speak_interval_minutes = profile.min_speak_minutes;
        self.min_deliberate_interval_minutes = profile.min_deliberate_minutes;
    }
}

const fn default_true() -> bool {
    true
}

const fn default_max_speaks() -> u16 {
    8
}

const fn default_min_speak() -> u32 {
    30
}

const fn default_min_deliberate() -> u32 {
    12
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_profiles_match_product_contract() {
        let low = AutonomyFrequency::Low.profile();
        let normal = AutonomyFrequency::Normal.profile();
        let high = AutonomyFrequency::High.profile();
        assert_eq!(
            (low.min_deliberate_minutes, low.min_speak_minutes),
            (25, 60)
        );
        assert_eq!(normal.max_proactive_speaks_per_day, 8);
        assert_eq!(high.decay_multiplier, 1.3);
    }
}
