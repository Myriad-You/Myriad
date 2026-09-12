use super::config::McpServerConfig;

pub(super) fn server_config(id: &str, stall: bool) -> McpServerConfig {
    McpServerConfig {
        id: id.into(),
        command: "/bin/sh".into(),
        args: vec!["-c".into(), include_str!("fixtures/server.sh").into()],
        env: std::collections::HashMap::from([(
            "MCP_TEST_STALL".into(),
            if stall { "1" } else { "0" }.into(),
        )]),
        enabled: true,
        auto_restart: false,
        max_restart_attempts: 0,
        trust_annotations: false,
    }
}
