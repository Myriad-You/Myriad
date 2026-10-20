//! 跑 Hugo，再对输出做后处理。子进程环境只留 PATH、HOME、TMPDIR 和缓存目录。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use myriad_blog::{
    BlogPost, BlogSettings, BlogSite, FriendLink, NOTES_CSS, export_site_files, is_script_asset,
    postprocess_html,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::layout::BlogLayout;
use super::themes::{copy_theme_into, ensure_default_theme};

const HUGO_TIMEOUT: Duration = Duration::from_secs(120);
const LOG_TAIL_CHARS: usize = 4_000;
const HTML_BYTE_CAP: usize = 20 * 1024 * 1024;

pub struct BuiltSite {
    pub log_tail: String,
    pub hugo_version: Option<String>,
    pub built_at: String,
    /// 输入指纹跟上次成功构建一致，没跑 Hugo，沿用现有输出。
    pub reused: bool,
}

/// 上次成功构建的输入指纹。`built_at` 和 `log_tail` 一起存，沿用输出时状态面板还能显示原来那次。
#[derive(Serialize, Deserialize)]
struct BuildStamp {
    fingerprint: String,
    built_at: String,
    log_tail: String,
}

#[derive(Debug)]
pub enum BuildProblem {
    Unavailable,
    Failed {
        message: &'static str,
        log_tail: String,
    },
}

pub async fn hugo_version_line() -> Option<String> {
    let child = hugo_command(&[])
        .arg("version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let finished = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output()).await;
    let output = finished.ok()?.ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().find(|line| !line.trim().is_empty())?;
    Some(scrub_log(line).chars().take(200).collect())
}

pub async fn render_and_build(
    layout: &BlogLayout,
    site: &BlogSite,
    settings: &BlogSettings,
    posts: &[BlogPost],
    friends: &[FriendLink],
) -> Result<BuiltSite, BuildProblem> {
    ensure_default_theme(layout).map_err(|_| BuildProblem::Failed {
        message: "Could not install the default theme",
        log_tail: String::new(),
    })?;
    let theme_id = myriad_blog::sanitize_theme_id(&settings.theme_id)
        .unwrap_or_else(|| myriad_blog::DEFAULT_THEME_ID.to_string());
    if !layout.theme_dir(&theme_id).join("theme.toml").is_file()
        && theme_id != myriad_blog::DEFAULT_THEME_ID
    {
        return Err(BuildProblem::Failed {
            message: "Selected theme is not installed",
            log_tail: String::new(),
        });
    }

    let work = layout.work_dir();
    if work.exists() {
        fs::remove_dir_all(&work).map_err(|_| failed("Could not prepare the build", ""))?;
    }
    let site_dir = work.join("site");
    let home = work.join("home");
    let tmp = work.join("tmp");
    let public = site_dir.join("public");
    for dir in [&site_dir, &home, &tmp, &layout.hugo_cache] {
        fs::create_dir_all(dir).map_err(|_| failed("Could not prepare the build", ""))?;
    }
    for (rel, body) in export_site_files(site, settings, posts, friends) {
        let path = site_dir.join(&rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| failed("Could not prepare the build", ""))?;
        }
        fs::write(path, body).map_err(|_| failed("Could not prepare the build", ""))?;
    }
    copy_theme_into(layout, &theme_id, &site_dir.join("themes").join(&theme_id))
        .map_err(|_| failed("Could not prepare the theme", ""))?;

    let version = hugo_version_line().await;
    let fingerprint = version
        .as_deref()
        .and_then(|version| input_fingerprint(&site_dir, version).ok());
    if let Some(fingerprint) = fingerprint.as_deref()
        && let Some(stamp) = read_stamp(&layout.stamp_path())
        && stamp.fingerprint == fingerprint
        && layout.current_dir().join("index.html").is_file()
    {
        let _ = fs::remove_dir_all(&work);
        return Ok(BuiltSite {
            log_tail: stamp.log_tail,
            hugo_version: version,
            built_at: stamp.built_at,
            reused: true,
        });
    }

    let log = match run_hugo(&site_dir, &public, &home, &tmp, &layout.hugo_cache).await {
        Ok(log) => log,
        Err(problem) => {
            let _ = fs::remove_dir_all(&work);
            return Err(problem);
        }
    };
    let scheme = super::themes::theme_scheme_switch(layout, &theme_id);
    if let Err(message) = postprocess_output(&public, scheme.as_ref()) {
        let _ = fs::remove_dir_all(&work);
        return Err(failed(message, &log));
    }
    super::layout::publish_tree(&layout.current_dir(), &public).map_err(|message| {
        let _ = fs::remove_dir_all(&work);
        failed(message, &log)
    })?;
    let _ = fs::remove_dir_all(&work);
    let built_at = chrono::Utc::now().to_rfc3339();
    if let Some(fingerprint) = fingerprint {
        write_stamp(
            &layout.stamp_path(),
            &BuildStamp {
                fingerprint,
                built_at: built_at.clone(),
                log_tail: log.clone(),
            },
        );
    }
    Ok(BuiltSite {
        log_tail: log,
        hugo_version: version,
        built_at,
        reused: false,
    })
}

/// Hugo 的全部输入：准备好的站点目录（导出内容、配置、主题拷贝）、Hugo 版本、Dart Sass，
/// 以及后端可执行文件本身。后处理规则和 notes.css 都编在可执行文件里，换了版本就要重跑。
fn input_fingerprint(site_dir: &Path, hugo_version: &str) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"myriad-blog-build/1\0");
    hasher.update(executable_identity().as_bytes());
    hasher.update(b"\0");
    hasher.update(hugo_version.as_bytes());
    hasher.update(b"\0");
    hasher.update(
        std::env::var("DART_SASS_DIR")
            .unwrap_or_default()
            .as_bytes(),
    );
    hasher.update(b"\0");
    hash_tree(site_dir, site_dir, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

fn executable_identity() -> String {
    let Ok(path) = std::env::current_exe() else {
        return String::new();
    };
    let Ok(meta) = fs::metadata(&path) else {
        return path.to_string_lossy().into_owned();
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{}:{}:{modified}", path.to_string_lossy(), meta.len())
}

fn hash_tree(root: &Path, dir: &Path, hasher: &mut Sha256) -> std::io::Result<()> {
    let mut entries = fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort();
    for path in entries {
        let meta = fs::symlink_metadata(&path)?;
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        if meta.is_dir() {
            hasher.update(b"d\0");
            hasher.update(rel.as_bytes());
            hasher.update(b"\0");
            hash_tree(root, &path, hasher)?;
        } else if meta.is_file() {
            let body = fs::read(&path)?;
            hasher.update(b"f\0");
            hasher.update(rel.as_bytes());
            hasher.update(b"\0");
            hasher.update((body.len() as u64).to_le_bytes());
            hasher.update(&body);
        }
    }
    Ok(())
}

fn read_stamp(path: &Path) -> Option<BuildStamp> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn write_stamp(path: &Path, stamp: &BuildStamp) {
    let Ok(body) = serde_json::to_vec(stamp) else {
        return;
    };
    if let Err(error) = fs::write(path, body) {
        tracing::warn!(%error, "failed to record the blog build stamp");
    }
}

async fn run_hugo(
    site: &Path,
    public: &Path,
    home: &Path,
    tmp: &Path,
    cache: &Path,
) -> Result<String, BuildProblem> {
    let mut child = hugo_command(&[("HOME", home), ("TMPDIR", tmp), ("HUGO_CACHEDIR", cache)])
        .current_dir(site)
        .arg("build")
        .arg("--source")
        .arg(site)
        .arg("--destination")
        .arg(public)
        .arg("--cacheDir")
        .arg(cache)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                BuildProblem::Unavailable
            } else {
                failed("Could not start Hugo", "")
            }
        })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_task = tokio::spawn(async move { read_capped(stdout).await });
    let stderr_task = tokio::spawn(async move { read_capped(stderr).await });
    let waited = tokio::time::timeout(HUGO_TIMEOUT, child.wait()).await;
    let (status, timed_out) = match waited {
        Ok(Ok(status)) => (Some(status), false),
        Ok(Err(_)) => (None, false),
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            (None, true)
        }
    };
    let stdout = stdout_task.await.unwrap_or_default();
    let stderr = stderr_task.await.unwrap_or_default();
    let log = tail(&scrub_log(&format!("{stdout}{stderr}")));
    let Some(status) = status else {
        return Err(failed(
            if timed_out {
                "Hugo timed out"
            } else {
                "Hugo did not finish"
            },
            &log,
        ));
    };
    if !status.success() {
        return Err(failed("Hugo build failed", &log));
    }
    Ok(log)
}

fn hugo_command(extra: &[(&str, &Path)]) -> Command {
    let bin = std::env::var("HUGO_BIN").unwrap_or_else(|_| "hugo".to_string());
    let bin = if bin.trim().is_empty() {
        "hugo".to_string()
    } else {
        bin
    };
    let mut command = Command::new(bin);
    command.env_clear().env("PATH", hugo_path());
    for (key, value) in extra {
        command.env(key, value);
    }
    command
}

fn hugo_path() -> String {
    match std::env::var("DART_SASS_DIR") {
        Ok(dir) if !dir.trim().is_empty() => {
            format!("{}:/usr/bin:/bin", dir.trim().trim_end_matches('/'))
        }
        _ => "/usr/bin:/bin".to_string(),
    }
}

async fn read_capped<R>(pipe: Option<R>) -> String
where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(mut pipe) = pipe else {
        return String::new();
    };
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8 * 1024];
    loop {
        match pipe.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if buf.len() < 64 * 1024 {
                    let room = 64 * 1024 - buf.len();
                    buf.extend_from_slice(&chunk[..n.min(room)]);
                }
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn postprocess_output(
    public: &Path,
    scheme: Option<&myriad_blog::SchemeSwitch>,
) -> Result<(), &'static str> {
    let mut files = Vec::new();
    collect_files(public, public, &mut files)?;
    for path in files {
        let rel = path
            .strip_prefix(public)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if is_script_asset(&rel) {
            fs::remove_file(&path).map_err(|_| "Could not clean the built site")?;
            continue;
        }
        let kind = path
            .symlink_metadata()
            .map_err(|_| "Could not clean the built site")?;
        if kind.file_type().is_symlink() {
            fs::remove_file(&path).map_err(|_| "Could not clean the built site")?;
            continue;
        }
        if !is_html(&path) {
            continue;
        }
        let bytes = fs::read(&path).map_err(|_| "Could not clean the built site")?;
        if bytes.len() > HTML_BYTE_CAP {
            return Err("A generated page is too large");
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let cleaned = postprocess_html(&text);
        // 主题怎么切深浅色记在 `<html>` 上，返回页面时后端照着替它切。
        let cleaned = match scheme {
            Some(scheme) => myriad_blog::mark_page_scheme(&cleaned, scheme),
            None => cleaned,
        };
        fs::write(&path, cleaned).map_err(|_| "Could not clean the built site")?;
    }
    let css = public.join("_myriad").join("notes.css");
    if let Some(parent) = css.parent() {
        fs::create_dir_all(parent).map_err(|_| "Could not clean the built site")?;
    }
    fs::write(css, NOTES_CSS).map_err(|_| "Could not clean the built site")?;
    Ok(())
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), &'static str> {
    let entries = fs::read_dir(dir).map_err(|_| "Could not clean the built site")?;
    for entry in entries {
        let entry = entry.map_err(|_| "Could not clean the built site")?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| "Could not clean the built site")?;
        if meta.file_type().is_symlink() {
            out.push(path);
            continue;
        }
        if meta.is_dir() {
            if path.starts_with(root) {
                collect_files(root, &path, out)?;
            }
        } else if meta.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn is_html(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "html" | "htm"
    )
}

fn failed(message: &'static str, log: &str) -> BuildProblem {
    BuildProblem::Failed {
        message,
        log_tail: tail(&scrub_log(log)),
    }
}

fn tail(text: &str) -> String {
    let count = text.chars().count();
    if count <= LOG_TAIL_CHARS {
        return text.to_string();
    }
    text.chars().skip(count - LOG_TAIL_CHARS).collect()
}

/// 日志里如果出现 `SECRET=...` 这种赋值，把值抹掉。路径可以留着。
pub fn scrub_log(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        if let Some(index) = line.find('=') {
            let key = line[..index].trim();
            let lower = key.to_ascii_lowercase();
            if lower.contains("secret")
                || lower.contains("password")
                || lower.contains("token")
                || lower.contains("api_key")
                || lower.contains("database_url")
            {
                out.push_str(key);
                out.push_str("=[redacted]");
                if line.ends_with('\n') {
                    out.push('\n');
                }
                continue;
            }
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubber_redacts_assignments_and_keeps_ordinary_lines() {
        let cleaned =
            scrub_log("build ok\nDATABASE_URL=postgres://user:secret@db/app\nPATH=/bin\n");
        assert!(cleaned.contains("build ok"));
        assert!(cleaned.contains("DATABASE_URL=[redacted]"));
        assert!(!cleaned.contains("postgres://"));
        assert!(cleaned.contains("PATH=/bin"));
    }

    #[test]
    fn fingerprint_follows_file_contents_paths_and_hugo_version() {
        let root = std::env::temp_dir().join(format!(
            "myriad-blog-fingerprint-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(root.join("content/posts")).unwrap();
        fs::write(root.join("hugo.toml"), "title = 'a'").unwrap();
        fs::write(root.join("content/posts/a.md"), "one").unwrap();
        let first = input_fingerprint(&root, "hugo v0.168.0").unwrap();
        assert_eq!(first, input_fingerprint(&root, "hugo v0.168.0").unwrap());
        assert_ne!(first, input_fingerprint(&root, "hugo v0.169.0").unwrap());

        fs::write(root.join("content/posts/a.md"), "two").unwrap();
        let edited = input_fingerprint(&root, "hugo v0.168.0").unwrap();
        assert_ne!(first, edited);

        fs::rename(
            root.join("content/posts/a.md"),
            root.join("content/posts/b.md"),
        )
        .unwrap();
        assert_ne!(edited, input_fingerprint(&root, "hugo v0.168.0").unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
