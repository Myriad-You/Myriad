//! Rescue CLI implementations. These do NOT require the updater HTTP API to be alive —
//! they operate on the configured state directory and Docker connection directly. Spec §16.3.

use std::path::PathBuf;
use std::process::Stdio;

use anyhow::{Context as _, Result};
use tracing::info;

use crate::docker::ROLLBACK_IMAGE_TAG;
use crate::snapshot::SnapshotManager;
use crate::state::StateDir;

pub struct Context {
    pub state: StateDir,
    pub compose_dir: PathBuf,
    pub env_file: PathBuf,
    pub pgdata: PathBuf,
    pub db_mode: crate::config::DbMode,
}

pub async fn status(ctx: &Context) -> Result<()> {
    let u = ctx.state.read_updater()?;
    let m = ctx.state.read_maintenance()?;
    let cur = ctx.state.read_current_job()?;
    let snaps = ctx.state.read_snapshots()?;
    let out = serde_json::json!({
        "updater_version": crate::self_version(),
        "db_mode": ctx.db_mode.as_str(),
        "pgdata_snapshot_enabled": ctx.db_mode.pgdata_snapshot_enabled(),
        "updater": u,
        "maintenance": m,
        "current_job": cur,
        "snapshots": snaps,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

pub async fn exit_maintenance(ctx: &Context, force: bool) -> Result<()> {
    if !force {
        anyhow::bail!("refusing without --force; this will discard maintenance state");
    }
    ctx.state.clear_maintenance()?;
    ctx.state.set_current_job(None)?;
    let line = "audit: rescue_exit_maintenance via=CLI";
    ctx.state.append_history(line)?;
    let _ = ctx.state.append_audit(line);
    info!("maintenance cleared");
    Ok(())
}

pub async fn rollback(ctx: &Context, snapshot_id: &str) -> Result<()> {
    let snapshots = ctx.state.read_snapshots()?;
    let meta = snapshots.items.iter().find(|s| s.id == snapshot_id);

    // Same last-known-good resolution as the worker path: snapshot source_version,
    // then updater.json.current_version. Never invent a hardcoded tag.
    let prev_tag = meta
        .and_then(|m| m.source_version.as_ref().map(|v| v.to_string()))
        .or_else(|| {
            ctx.state
                .read_updater()
                .ok()
                .and_then(|u| u.current_version.map(|v| v.to_string()))
        });

    if let Some(ref tag) = prev_tag {
        if let Err(e) = materialize_pinned_rollback_images(ctx, tag).await {
            tracing::warn!(
                err = %e,
                version = %tag,
                "rescue could not restore version refs from the local rollback slot"
            );
        }
    }

    info!(snapshot = snapshot_id, "rescue rollback: stopping services");
    compose_v2_or_v1(ctx, &["stop", "-t", "30", "frontend", "backend"]).await?;

    if ctx.db_mode.is_external() {
        info!("db_mode=external; skipping pgdata restore (tag-only rescue rollback)");
    } else {
        if meta.is_none() {
            anyhow::bail!("snapshot {snapshot_id} not present in snapshots.json");
        }
        crate::probe::filesystem::require_pgdata(&ctx.pgdata)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let snap = SnapshotManager {
            state: &ctx.state,
            pgdata: ctx.pgdata.clone(),
        };
        compose_v2_or_v1(ctx, &["stop", "-t", "60", "postgres"]).await?;
        snap.restore(snapshot_id).await?;
    }

    if let Some(ref tag) = prev_tag {
        let mut env = crate::env_file::EnvFile::load(&ctx.env_file)
            .context("load .env for MYRIAD_TAG restore")?;
        let before = env.get("MYRIAD_TAG").unwrap_or("").to_string();
        env.set("MYRIAD_TAG", tag)?;
        env.save()?;
        info!(from = %before, to = %tag, "rescue rollback: restored MYRIAD_TAG");
    } else {
        tracing::warn!(
            "rescue rollback: no source_version / current_version; leaving MYRIAD_TAG unchanged"
        );
    }

    if !ctx.db_mode.is_external() {
        compose_v2_or_v1(ctx, &["start", "postgres"]).await?;
    }
    compose_v2_or_v1(ctx, &["up", "-d", "--no-deps", "backend", "frontend"]).await?;

    if let Some(ref tag) = prev_tag {
        if let Ok(v) = crate::version::DeployTag::parse(tag) {
            let mut st = ctx.state.read_updater()?;
            st.current_version = Some(v);
            st.current_commit_sha = None;
            ctx.state.write_updater(&st)?;
        }
    }

    ctx.state.clear_maintenance()?;
    ctx.state.set_current_job(None)?;
    let hist = format!(
        "rescue rollback to snapshot {snapshot_id} (tag={}, db_mode={})",
        prev_tag.as_deref().unwrap_or("unchanged"),
        ctx.db_mode.as_str()
    );
    let audit = format!(
        "audit: rescue_rollback snapshot={snapshot_id} tag={} db_mode={}",
        prev_tag.as_deref().unwrap_or("unchanged"),
        ctx.db_mode.as_str()
    );
    ctx.state.append_history(&hist)?;
    let _ = ctx.state.append_audit(&audit);
    info!("rescue rollback complete");
    Ok(())
}

async fn materialize_pinned_rollback_images(ctx: &Context, version: &str) -> Result<()> {
    let updater = ctx.state.read_updater()?;
    if updater.rollback_version.as_ref().map(|v| v.as_str()) != Some(version) {
        return Ok(());
    }

    let env = crate::env_file::EnvFile::load(&ctx.env_file)?;
    let backend = env
        .get("BACKEND_IMAGE")
        .context("BACKEND_IMAGE missing; cannot restore pinned rollback image")?;
    let frontend = env
        .get("FRONTEND_IMAGE")
        .context("FRONTEND_IMAGE missing; cannot restore pinned rollback image")?;

    // Pair integrity: only materialize when BOTH components have *:myriad-rollback.
    let pair = [
        ("backend", backend.to_string()),
        ("frontend", frontend.to_string()),
    ];
    let mut missing_pins = Vec::new();
    for (component, repo) in &pair {
        let rollback_ref = format!("{repo}:{ROLLBACK_IMAGE_TAG}");
        if !docker_image_exists(&rollback_ref).await {
            missing_pins.push((*component).to_string());
        }
    }
    if !missing_pins.is_empty() {
        tracing::warn!(
            %version,
            missing = ?missing_pins,
            "incomplete local rollback slot; rescue will not materialize a split pair"
        );
        return Ok(());
    }

    for (component, repo) in &pair {
        let version_ref = format!("{repo}:{version}");
        if docker_image_exists(&version_ref).await {
            continue;
        }

        let rollback_ref = format!("{repo}:{ROLLBACK_IMAGE_TAG}");
        let status = tokio::process::Command::new("docker")
            .args(["image", "tag", &rollback_ref, &version_ref])
            .status()
            .await
            .context("spawn docker image tag")?;
        if !status.success() {
            anyhow::bail!(
                "docker image tag {rollback_ref} {version_ref} failed with status {status}"
            );
        }
        info!(
            %component,
            source = %rollback_ref,
            target = %version_ref,
            "rescue restored version ref from local rollback slot"
        );
    }

    Ok(())
}

async fn docker_image_exists(image_ref: &str) -> bool {
    tokio::process::Command::new("docker")
        .args(["image", "inspect", image_ref])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|status| status.success())
}

pub async fn diagnose(ctx: &Context, output: &PathBuf) -> Result<()> {
    let dir = tempfile::tempdir()?;
    let staging = dir.path().join("diagnostics");
    std::fs::create_dir_all(&staging)?;

    // Copy state files (excluding snapshots/, which can be huge).
    for entry in std::fs::read_dir(ctx.state.root())? {
        let entry = entry?;
        let name = entry.file_name();
        if name == "snapshots" {
            continue;
        }
        let target = staging.join(&name);
        if entry.file_type()?.is_dir() {
            copy_dir_recursively(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }

    // docker compose ps + docker images.
    let _ = run_capture(
        "docker",
        &[
            "images",
            "--format",
            "{{.Repository}}:{{.Tag}}\t{{.ID}}\t{{.Size}}",
        ],
    )
    .await
    .map(|s| std::fs::write(staging.join("docker-images.txt"), s));
    let _ = run_capture("docker", &["info"])
        .await
        .map(|s| std::fs::write(staging.join("docker-info.txt"), s));

    // tar.gz it.
    let status = tokio::process::Command::new("tar")
        .arg("-czf")
        .arg(output)
        .arg("-C")
        .arg(dir.path())
        .arg("diagnostics")
        .status()
        .await
        .context("spawn tar")?;
    if !status.success() {
        anyhow::bail!("tar exited with {status:?}");
    }
    println!("wrote {}", output.display());
    Ok(())
}

pub async fn forget_job(ctx: &Context) -> Result<()> {
    ctx.state.set_current_job(None)?;
    let line = "audit: rescue_forget_job via=CLI";
    ctx.state.append_history(line)?;
    let _ = ctx.state.append_audit(line);
    info!("current_job cleared");
    Ok(())
}

pub async fn clean_snapshots(ctx: &Context, keep: usize) -> Result<()> {
    let snap = SnapshotManager {
        state: &ctx.state,
        pgdata: ctx.pgdata.clone(),
    };
    let removed = snap.prune(keep)?;
    println!("removed snapshots: {removed:?}");
    Ok(())
}

async fn compose_v2_or_v1(ctx: &Context, args: &[&str]) -> Result<()> {
    let project = std::env::var("COMPOSE_PROJECT_NAME").unwrap_or_else(|_| "myriad".into());

    let v2 = tokio::process::Command::new("docker")
        .arg("compose")
        .arg("--env-file")
        .arg(&ctx.env_file)
        .arg("-p")
        .arg(&project)
        .args(args)
        .current_dir(&ctx.compose_dir)
        .status()
        .await;
    if let Ok(s) = v2 {
        if s.success() {
            return Ok(());
        }
    }
    let v1 = tokio::process::Command::new("docker-compose")
        .arg("--env-file")
        .arg(&ctx.env_file)
        .arg("-p")
        .arg(&project)
        .args(args)
        .current_dir(&ctx.compose_dir)
        .status()
        .await
        .context("spawn docker-compose")?;
    if !v1.success() {
        anyhow::bail!("compose {args:?} failed (status {v1:?})");
    }
    Ok(())
}

async fn run_capture(prog: &str, args: &[&str]) -> Result<Vec<u8>> {
    let out = tokio::process::Command::new(prog)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context(format!("spawn {prog}"))?;
    Ok(out.stdout)
}

fn copy_dir_recursively(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursively(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
