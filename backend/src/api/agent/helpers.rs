//! Agent API — helpers
use super::*;
use crate::error::HttpError;

// 辅助函数

/// 解析 user_id，返回标准化错误
pub(crate) fn parse_user_id(claims: &Claims) -> Result<i32, HttpError> {
    claims.durable_user_id().ok_or_else(|| {
        HttpError::from((
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Invalid user")),
        ))
    })
}

/// Existing pairings must remain revocable after Agent access is withdrawn.
pub(crate) fn parse_pairing_user_id(claims: &Claims) -> Result<i32, HttpError> {
    claims.durable_user_id().ok_or_else(|| {
        HttpError::from((
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Login required")),
        ))
    })
}

/// 解析 user_id 并校验 Agent 可见性/使用权限
pub(crate) async fn parse_user_id_with_agent_access(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<i32, HttpError> {
    let user_id = parse_user_id(claims)?;
    crate::services::agent::run::agent_access_gate(db, user_id).await?;
    Ok(user_id)
}

/// Global Agent management surfaces are restricted to the current administrator.
pub(crate) async fn require_current_admin(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<i32, HttpError> {
    let user_id = parse_user_id(claims)?;
    let is_admin = crate::services::agent::user_is_current_admin(db, user_id)
        .await
        .map_err(|error| HttpError(myriad_error::AppError::internal(error)))?;
    if !is_admin {
        return Err(HttpError::from((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Administrator access required", "code": "admin_required" })),
        )));
    }
    Ok(user_id)
}

#[cfg(test)]
mod agent_entry_gate_tests {
    #[test]
    fn parse_user_id_requires_positive_subject() {
        let src = include_str!("helpers.rs");
        let parse = src
            .split("pub(crate) fn parse_user_id(")
            .nth(1)
            .expect("parse_user_id");
        assert!(parse.contains("durable_user_id()"));
        let pairing = src
            .split("pub(crate) fn parse_pairing_user_id(")
            .nth(1)
            .expect("parse_pairing_user_id");
        assert!(pairing.contains("durable_user_id()"));
    }

    /// 取 `fn <name>(` 之后的一段源码，够覆盖签名和开头几行。
    fn head_of(source: &str, name: &str) -> String {
        let at = source
            .find(&format!("fn {name}("))
            .unwrap_or_else(|| panic!("{name} not found"));
        source[at..].chars().take(700).collect()
    }

    /// 能把一轮 Work 提交进 Agent 的 HTTP 入口，必须走带可见性判定的解析。
    ///
    /// `interrupt_session` 提交 `context: None` 即 Work；只解析 user_id 拦不住。
    #[test]
    fn every_work_entry_checks_module_visibility() {
        let cases: [(&str, &str, &str); 4] = [
            (
                "process.rs",
                include_str!("process.rs"),
                "start_process_run",
            ),
            ("presets.rs", include_str!("presets.rs"), "execute_preset"),
            (
                "heartbeat_mcp.rs",
                include_str!("heartbeat_mcp.rs"),
                "interrupt_session",
            ),
            (
                "intentions.rs",
                include_str!("intentions.rs"),
                "accept_intention",
            ),
        ];
        for (file, source, name) in cases {
            let head = head_of(source, name);
            assert!(
                head.contains("parse_user_id_with_agent_access"),
                "{file}::{name} 是 Work 入口，却没有过模块可见性这道门"
            );
        }
    }

    /// Channels start runs by user, without a login: the service entries
    /// must pass the same gate before anything runs.
    #[test]
    fn channel_run_entries_pass_the_same_gate() {
        let source = include_str!("../../services/agent/run/start.rs");
        for name in ["start_for_user", "answer_for_user"] {
            assert!(
                head_of(source, name).contains("agent_access_gate("),
                "run::{name} 是 Work 入口，却没有过模块可见性这道门"
            );
        }
        let gate = head_of(
            include_str!("helpers.rs"),
            "parse_user_id_with_agent_access",
        );
        assert!(
            gate.contains("agent_access_gate("),
            "HTTP 与通道要过同一道门"
        );
    }

    /// 能力过滤是按人给的，不是按档给的：非管理员照常进 Work，
    /// 每一步能不能落地由 `get_user_permissions` 说了算。
    #[test]
    fn work_entries_do_not_gate_on_role() {
        for (file, source) in [
            ("process.rs", include_str!("process.rs")),
            ("presets.rs", include_str!("presets.rs")),
            ("intentions.rs", include_str!("intentions.rs")),
            ("heartbeat_mcp.rs", include_str!("heartbeat_mcp.rs")),
        ] {
            assert!(
                !source.contains("interaction_mode_allowed"),
                "{file} 不该按身份分运行时路径；能力过滤在 get_user_permissions"
            );
        }
    }
}
use myriad_error::AppError;
