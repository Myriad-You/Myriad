pub use myriad_agent_rules::{
    extract_json_array_from_ai_response, parse_rsshub_radar_rules, project_time_info, weekday_zh,
};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn project_time_info_uses_supplied_clock_not_hidden_now() {
        use chrono::{TimeZone, Utc};
        let now = Utc.with_ymd_and_hms(2026, 7, 31, 12, 30, 0).unwrap();
        let out = project_time_info(now, "Asia/Shanghai").expect("valid zone");
        assert_eq!(out["timezone"], "Asia/Shanghai");
        assert_eq!(out["year"], 2026);
        assert_eq!(out["month"], 7);
        assert_eq!(out["day"], 31);
        assert_eq!(out["hour"], 20);
        assert_eq!(out["minute"], 30);
        assert_eq!(out["weekday"], "Friday"); // 2026-07-31 20:30 +08 is Friday
        assert_eq!(out["timestamp"], now.timestamp());
        assert!(out["datetime"]
            .as_str()
            .unwrap()
            .starts_with("2026-07-31T20:30:00"));
        assert!(project_time_info(now, "Not/AZone").is_err());
        let utc = project_time_info(now, "UTC").expect("utc");
        assert_eq!(utc["hour"], 12);
        let offset = project_time_info(now, "UTC+8").expect("offset");
        assert_eq!(offset["hour"], 20);
    }
}
