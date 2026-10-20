//! 博客构建调度。同一时刻只有一次构建，触发后大约两秒再跑，跑完若又脏了再跑一次。

mod build;
mod content;
mod layout;
mod serve;
mod themes;

#[cfg(test)]
mod hugo_e2e;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use sea_orm::DatabaseConnection;
use serde::Serialize;
use tokio::sync::{Mutex, mpsc};

use crate::services::config_service::ConfigService;

pub use serve::{BlogTarget, content_type_for, resolve_blog_target, theme_not_found_page};
pub use themes::ThemeRecord;

static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();
static PENDING: AtomicBool = AtomicBool::new(false);

struct Engine {
    db: Mutex<Option<DatabaseConnection>>,
    state: Mutex<BuildState>,
    tx: mpsc::UnboundedSender<()>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<()>>>,
    files: Mutex<()>,
    started: AtomicBool,
}

#[derive(Clone)]
struct BuildState {
    status: &'static str,
    last_built_at: Option<String>,
    last_error: Option<String>,
    log_tail: Option<String>,
    hugo_available: bool,
    hugo_version: Option<String>,
}

impl Default for BuildState {
    fn default() -> Self {
        Self {
            status: "idle",
            last_built_at: None,
            last_error: None,
            log_tail: None,
            hugo_available: false,
            hugo_version: None,
        }
    }
}

#[derive(Serialize)]
pub struct SettingsBody {
    pub theme_id: String,
    pub site_title: String,
    pub description: String,
    pub params: serde_json::Value,
}

#[derive(Serialize)]
pub struct BlogSnapshot {
    pub settings: SettingsBody,
    pub themes: Vec<ThemeJson>,
    pub hugo: HugoJson,
    pub build: BuildJson,
}

#[derive(Serialize)]
pub struct ThemeJson {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub min_version: Option<String>,
    pub builtin: bool,
    pub installed_at: Option<String>,
    /// 深浅色怎么切；站长没确认过时 `confirmed` 为假，创作台会请站长看一眼。
    pub scheme: Option<themes::ThemeScheme>,
}

#[derive(Serialize)]
pub struct HugoJson {
    pub available: bool,
    pub version: Option<String>,
}

#[derive(Serialize)]
pub struct BuildJson {
    pub status: String,
    pub last_built_at: Option<String>,
    pub last_error: Option<String>,
    pub log_tail: Option<String>,
}

fn engine() -> Arc<Engine> {
    ENGINE
        .get_or_init(|| {
            let (tx, rx) = mpsc::unbounded_channel();
            Arc::new(Engine {
                db: Mutex::new(None),
                state: Mutex::new(BuildState::default()),
                tx,
                rx: Mutex::new(Some(rx)),
                files: Mutex::new(()),
                started: AtomicBool::new(false),
            })
        })
        .clone()
}

/// 发布、友链、设置或主题变了之后调用。没启动构建器时先记着，启动时补跑。
pub fn request_rebuild() {
    let Some(engine) = ENGINE.get() else {
        PENDING.store(true, Ordering::Release);
        return;
    };
    let _ = engine.tx.send(());
    if let Ok(mut state) = engine.state.try_lock()
        && state.status != "running"
    {
        state.status = "queued";
    }
}

pub fn start(db: DatabaseConnection) {
    let engine = engine();
    tokio::spawn(async move {
        {
            let mut slot = engine.db.lock().await;
            *slot = Some(db);
        }
        if engine.started.swap(true, Ordering::AcqRel) {
            if PENDING.swap(false, Ordering::AcqRel) {
                let _ = engine.tx.send(());
            }
            return;
        }
        engine.worker().await;
    });
}

impl Engine {
    async fn worker(self: Arc<Self>) {
        self.probe_hugo().await;
        let mut rx = self
            .rx
            .lock()
            .await
            .take()
            .expect("blog builder starts once");
        // 启动时的数据修补不发重建信号，后端换版本也会改变导出与后处理，所以启动总要检查一次。
        // 输入指纹没变时不跑 Hugo，见 `build::render_and_build`。
        PENDING.store(false, Ordering::Release);
        let _ = self.tx.send(());
        let mut armed = false;
        let sleep = tokio::time::sleep(Duration::from_secs(2));
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                message = rx.recv() => {
                    if message.is_none() {
                        break;
                    }
                    armed = true;
                    let mut state = self.state.lock().await;
                    if state.status != "running" {
                        state.status = "queued";
                    }
                    sleep.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(2));
                }
                _ = &mut sleep, if armed => {
                    armed = false;
                    self.build_once().await;
                }
            }
        }
    }

    async fn probe_hugo(&self) {
        let version = build::hugo_version_line().await;
        let mut state = self.state.lock().await;
        state.hugo_available = version.is_some();
        state.hugo_version = version;
        if !state.hugo_available && state.status == "idle" {
            state.status = "unavailable";
        }
    }

    async fn build_once(&self) {
        {
            let mut state = self.state.lock().await;
            state.status = "running";
            state.last_error = None;
        }
        let _files = self.files.lock().await;
        let db = self.db.lock().await.clone();
        let Some(db) = db else {
            self.finish_failed("Blog is not ready", "").await;
            return;
        };
        let settings = content::load_settings(&db).await;
        let site = content::load_site(&db, &settings).await;
        let posts = match content::load_posts(&db).await {
            Ok(posts) => posts,
            Err(message) => {
                self.finish_failed(message, "").await;
                return;
            }
        };
        let friends = match content::load_friends(&db).await {
            Ok(friends) => friends,
            Err(message) => {
                self.finish_failed(message, "").await;
                return;
            }
        };
        match build::render_and_build(&layout::layout(), &site, &settings, &posts, &friends).await {
            Ok(built) => {
                let mut state = self.state.lock().await;
                if built.reused {
                    tracing::debug!("blog inputs unchanged; kept the existing output");
                }
                state.status = "ok";
                state.last_built_at = Some(built.built_at);
                state.last_error = None;
                state.log_tail = Some(built.log_tail);
                if built.hugo_version.is_some() {
                    state.hugo_available = true;
                    state.hugo_version = built.hugo_version;
                }
            }
            Err(build::BuildProblem::Unavailable) => {
                let mut state = self.state.lock().await;
                state.status = "unavailable";
                state.hugo_available = false;
                state.hugo_version = None;
                state.last_error = Some("Hugo is not available".to_string());
                state.log_tail = None;
            }
            Err(build::BuildProblem::Failed { message, log_tail }) => {
                self.finish_failed(message, &log_tail).await;
            }
        }
    }

    async fn finish_failed(&self, message: &str, log_tail: &str) {
        let mut state = self.state.lock().await;
        state.status = "failed";
        state.last_error = Some(message.to_string());
        state.log_tail = if log_tail.is_empty() {
            None
        } else {
            Some(log_tail.to_string())
        };
    }
}

pub async fn snapshot(db: &DatabaseConnection) -> BlogSnapshot {
    let settings = content::load_settings(db).await;
    let themes = themes::list_themes(&layout::layout());
    let state = if let Some(engine) = ENGINE.get() {
        engine.state.lock().await.clone()
    } else {
        BuildState::default()
    };
    snapshot_from(settings, themes, &state)
}

fn snapshot_from(
    settings: myriad_blog::BlogSettings,
    themes: Vec<ThemeRecord>,
    state: &BuildState,
) -> BlogSnapshot {
    BlogSnapshot {
        settings: SettingsBody {
            theme_id: settings.theme_id,
            site_title: settings.site_title,
            description: settings.description,
            params: settings.params,
        },
        themes: themes.into_iter().map(theme_json).collect(),
        hugo: HugoJson {
            available: state.hugo_available,
            version: state.hugo_version.clone(),
        },
        build: BuildJson {
            status: state.status.to_string(),
            last_built_at: state.last_built_at.clone(),
            last_error: state.last_error.clone(),
            log_tail: state.log_tail.clone(),
        },
    }
}

fn theme_json(record: ThemeRecord) -> ThemeJson {
    ThemeJson {
        id: record.id,
        name: record.name,
        version: record.version,
        min_version: record.min_version,
        builtin: record.builtin,
        installed_at: record.installed_at,
        scheme: record.scheme,
    }
}

pub async fn save_settings(
    db: &DatabaseConnection,
    theme_id: &str,
    site_title: &str,
    description: &str,
    params: serde_json::Value,
) -> Result<BlogSnapshot, (&'static str, u16)> {
    if !params.is_object() {
        return Err(("params must be an object", 400));
    }
    let themes = themes::list_themes(&layout::layout());
    let canonical = themes
        .iter()
        .find(|theme| theme.id.eq_ignore_ascii_case(theme_id.trim()))
        .map(|theme| theme.id.clone())
        .ok_or(("Unknown theme", 400))?;
    let settings = myriad_blog::BlogSettings {
        theme_id: canonical,
        site_title: site_title.trim().to_string(),
        description: description.trim().to_string(),
        params,
    };
    let stored = myriad_blog::blog_settings_to_value(&settings);
    let previous = myriad_blog::blog_settings_to_value(&content::load_settings(db).await);
    if stored == previous {
        return Ok(snapshot(db).await);
    }
    ConfigService::new(db.clone())
        .update_config(myriad_blog::BLOG_SETTINGS_KEY, stored)
        .await
        .map_err(|_| ("Could not save blog settings", 500))?;
    if let Ok(loaded) = ConfigService::new(db.clone()).load_config().await {
        crate::services::config_service::install_dynamic_config(
            &crate::GLOBAL_DYNAMIC_CONFIG,
            loaded,
        )
        .await;
    }
    request_rebuild();
    Ok(snapshot(db).await)
}

/// 新装的主题要站长点「使用」才进构建；只有覆盖了正在用的主题时才重建。
pub async fn install_archive(
    db: &DatabaseConnection,
    bytes: Vec<u8>,
) -> Result<ThemeJson, (&'static str, u16)> {
    let layout = layout::layout();
    let engine = engine();
    let _guard = engine.files.lock().await;
    let record = tokio::task::spawn_blocking(move || themes::install_zip(&layout, &bytes))
        .await
        .map_err(|_| ("Could not store the theme", 500))?
        .map_err(|error| (error.message, error.status))?;
    drop(_guard);
    if content::load_settings(db)
        .await
        .theme_id
        .eq_ignore_ascii_case(&record.id)
    {
        request_rebuild();
    }
    Ok(theme_json(record))
}

pub async fn install_from_url(
    db: &DatabaseConnection,
    url: &str,
) -> Result<ThemeJson, (&'static str, u16)> {
    let url = url.trim();
    if url.is_empty() {
        return Err(("A theme URL is required", 400));
    }
    let fetched = crate::services::outbound_security::get_public_following_redirects(
        url,
        Duration::from_secs(60),
        Some("Myriad"),
    )
    .await
    .map_err(|_| ("Theme download failed", 400))?;
    if !fetched.response.status().is_success() {
        return Err(("Theme download failed", 400));
    }
    let bytes = crate::services::outbound_security::read_limited_body(
        fetched.response,
        myriad_blog::MAX_THEME_ARCHIVE_BYTES as usize,
    )
    .await
    .map_err(|_| ("Theme archive is too large", 413))?;
    install_archive(db, bytes).await
}

/// 站长确认或改了主题的深浅色切换办法。正在用的主题要重建：办法记在构建出的页面上。
pub async fn set_theme_scheme(
    db: &DatabaseConnection,
    id: &str,
    switch: myriad_blog::SchemeSwitch,
) -> Result<ThemeJson, (&'static str, u16)> {
    let engine = engine();
    let _guard = engine.files.lock().await;
    let record = themes::set_theme_scheme(&layout::layout(), id, switch)
        .map_err(|error| (error.message, error.status))?;
    drop(_guard);
    if content::load_settings(db)
        .await
        .theme_id
        .eq_ignore_ascii_case(&record.id)
    {
        request_rebuild();
    }
    Ok(theme_json(record))
}

pub async fn remove_theme(db: &DatabaseConnection, id: &str) -> Result<(), (&'static str, u16)> {
    let engine = engine();
    let _guard = engine.files.lock().await;
    let layout = layout::layout();
    let current = content::load_settings(db).await;
    let removing_current = current.theme_id.eq_ignore_ascii_case(id.trim());
    themes::uninstall_theme(&layout, id).map_err(|error| (error.message, error.status))?;
    if layout.hugo_cache.exists()
        && let Err(error) = std::fs::remove_dir_all(&layout.hugo_cache)
    {
        tracing::warn!(%error, "failed to clear the Hugo cache after theme removal");
    }
    if removing_current {
        let settings = myriad_blog::BlogSettings {
            theme_id: myriad_blog::DEFAULT_THEME_ID.to_string(),
            ..current
        };
        let stored = myriad_blog::blog_settings_to_value(&settings);
        ConfigService::new(db.clone())
            .update_config(myriad_blog::BLOG_SETTINGS_KEY, stored)
            .await
            .map_err(|_| ("Could not switch back to the default theme", 500))?;
        if layout.current_dir().exists() {
            if let Err(error) = std::fs::remove_dir_all(layout.current_dir()) {
                tracing::warn!(%error, "failed to delete blog output after theme removal");
            }
        }
    }
    drop(_guard);
    if removing_current {
        request_rebuild();
    }
    Ok(())
}

pub async fn build_view() -> BuildJson {
    let state = if let Some(engine) = ENGINE.get() {
        engine.state.lock().await.clone()
    } else {
        BuildState::default()
    };
    BuildJson {
        status: state.status.to_string(),
        last_built_at: state.last_built_at,
        last_error: state.last_error,
        log_tail: state.log_tail,
    }
}

pub fn output_dir() -> std::path::PathBuf {
    layout::layout().current_dir()
}
