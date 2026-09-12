//! Development image signatures live in the image registry, so private GitHub
//! source access is not needed. Cosign 2.4.1's exit code 10 alone means unsigned;
//! network, certificate, claim and transparency-log failures always fail closed.

use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::process::Command;

use crate::error::{Result, UpdaterError};

#[derive(Debug, PartialEq)]
pub enum DevSignature {
    Unsigned,
    Verified { commit_sha: String },
}

pub async fn verify(
    image_repo: &str,
    digest: &str,
    source_repo: &str,
    component: &str,
    expected_sha: &str,
) -> Result<DevSignature> {
    verify_with_command(
        Command::new("cosign"),
        image_repo,
        digest,
        source_repo,
        component,
        expected_sha,
    )
    .await
}

async fn verify_with_command(
    mut command: Command,
    image_repo: &str,
    digest: &str,
    source_repo: &str,
    component: &str,
    expected_sha: &str,
) -> Result<DevSignature> {
    require_digest(digest)?;
    let identity = format!(
        "^https://github\\.com/{}/\\.github/workflows/docker-publish\\.yml@refs/heads/(main|preview|beta)$",
        regex::escape(source_repo)
    );
    command
        .args([
            "verify",
            "--output=json",
            "--certificate-identity-regexp",
            &identity,
            "--certificate-oidc-issuer",
            "https://token.actions.githubusercontent.com",
            "-a",
            &format!("component={component}"),
            &format!("{image_repo}@{digest}"),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(120), command.output())
        .await
        .map_err(|_| UpdaterError::Precondition("cosign dev verification timed out".into()))?
        .map_err(|_| {
            UpdaterError::Precondition("could not execute cosign dev verification".into())
        })?;
    interpret_output(
        output.status.code(),
        &output.stdout,
        digest,
        component,
        expected_sha,
    )
}

pub(crate) fn require_digest(digest: &str) -> Result<()> {
    if digest.strip_prefix("sha256:").is_some_and(|s| {
        s.len() == 64
            && s.bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    }) {
        Ok(())
    } else {
        Err(UpdaterError::Precondition(
            "expected a registry sha256 digest".into(),
        ))
    }
}

fn interpret_output(
    code: Option<i32>,
    stdout: &[u8],
    digest: &str,
    component: &str,
    expected_sha: &str,
) -> Result<DevSignature> {
    match code {
        Some(10) => return Ok(DevSignature::Unsigned),
        Some(0) => {},
        // Do not echo raw tool output, which can contain registry credentials/URLs.
        _ => return Err(UpdaterError::Precondition(format!("cosign dev signature verification failed (exit {code:?}); unsigned fallback is forbidden"))),
    }
    let claims: Vec<Value> = serde_json::from_slice(stdout)
        .map_err(|_| UpdaterError::Precondition("invalid cosign verification output".into()))?;
    for claim in claims {
        let sha = claim
            .pointer("/optional/git_sha")
            .and_then(Value::as_str)
            .unwrap_or("");
        if claim
            .pointer("/critical/image/docker-manifest-digest")
            .and_then(Value::as_str)
            == Some(digest)
            && claim.pointer("/optional/component").and_then(Value::as_str) == Some(component)
            && sha.len() == 40
            && sha
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            && (7..=40).contains(&expected_sha.len())
            && sha.starts_with(expected_sha)
        {
            return Ok(DevSignature::Verified {
                commit_sha: sha.into(),
            });
        }
    }
    Err(UpdaterError::Precondition(
        "signed dev image does not match requested commit, component or digest".into(),
    ))
}

/// Both components must name the exact same full commit, even when the request
/// uses a short SHA. An unsigned half can only use the explicit legacy path.
pub fn pair_commit(backend: &DevSignature, frontend: &DevSignature) -> Result<Option<String>> {
    match (backend, frontend) {
        (DevSignature::Verified { commit_sha: a }, DevSignature::Verified { commit_sha: b }) => {
            if a != b {
                return Err(UpdaterError::Precondition(
                    "signed frontend/backend commits differ".into(),
                ));
            }
            Ok(Some(a.clone()))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn claims(digest: &str, sha: &str, component: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([{"critical":{"image":{"docker-manifest-digest":digest}},"optional":{"git_sha":sha,"component":component}}])).unwrap()
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn cosign_process_uses_digest_workflow_identity_and_classifies_exit_codes() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("cosign");
        let args = dir.path().join("args");
        let sha = "a".repeat(40);
        let digest = format!("sha256:{}", "b".repeat(64));
        let output = String::from_utf8(claims(&digest, &sha, "backend")).unwrap();
        for code in [0, 10, 1, 12, 13] {
            std::fs::write(
                &executable,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nprintf '%s' '{}'\nexit {code}\n",
                    args.display(),
                    output
                ),
            )
            .unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let result = verify_with_command(
                Command::new(&executable),
                "docker.io/org/backend",
                &digest,
                "Myriad-You/Myriad",
                "backend",
                &sha[..7],
            )
            .await;
            match code {
                0 => assert_eq!(
                    result.unwrap(),
                    DevSignature::Verified {
                        commit_sha: sha.clone()
                    }
                ),
                10 => assert_eq!(result.unwrap(), DevSignature::Unsigned),
                _ => assert!(result.is_err()),
            }
            let arguments = std::fs::read_to_string(&args).unwrap();
            assert!(arguments.contains(&format!("docker.io/org/backend@{digest}")));
            assert!(arguments.contains("docker-publish\\.yml@refs/heads/(main|preview|beta)$"));
            assert!(arguments.contains(
                "--certificate-oidc-issuer\nhttps://token.actions.githubusercontent.com"
            ));
            assert!(arguments.contains("component=backend"));
        }
    }

    #[test]
    fn only_missing_signature_exit_allows_legacy_confirmation() {
        assert_eq!(
            interpret_output(Some(10), b"", "", "", "").unwrap(),
            DevSignature::Unsigned
        );
        for code in [None, Some(1), Some(11), Some(12), Some(13)] {
            assert!(interpret_output(code, b"no signatures found", "", "", "").is_err());
        }
        assert!(interpret_output(Some(0), b"[]", "", "", "").is_err());
        assert!(interpret_output(Some(0), b"bad json", "", "", "").is_err());
    }
    #[test]
    fn verified_claim_binds_full_sha_component_and_digest() {
        let sha = "a".repeat(40);
        let digest = format!("sha256:{}", "b".repeat(64));
        let bytes = claims(&digest, &sha, "backend");
        assert_eq!(
            interpret_output(Some(0), &bytes, &digest, "backend", &sha[..7]).unwrap(),
            DevSignature::Verified {
                commit_sha: sha.clone()
            }
        );
        for (d, c, s) in [
            ("sha256:other", "backend", sha.as_str()),
            (digest.as_str(), "frontend", sha.as_str()),
            (digest.as_str(), "backend", "bbbbbbb"),
        ] {
            assert!(interpret_output(Some(0), &bytes, d, c, s).is_err());
        }
        assert!(interpret_output(
            Some(0),
            &claims(&digest, &sha[..7], "backend"),
            &digest,
            "backend",
            &sha[..7]
        )
        .is_err());
    }
    #[test]
    fn component_pair_requires_full_commit_equality() {
        let a = DevSignature::Verified {
            commit_sha: format!("abcdef0{}", "a".repeat(33)),
        };
        let b = DevSignature::Verified {
            commit_sha: format!("abcdef0{}", "b".repeat(33)),
        };
        assert!(pair_commit(&a, &b).is_err());
        assert!(pair_commit(&a, &a).unwrap().is_some());
        assert_eq!(pair_commit(&a, &DevSignature::Unsigned).unwrap(), None);
    }
}
