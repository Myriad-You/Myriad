//! `DATA_DIR/blog` 的目录约定。输出用目录改名切换，不靠符号链接。

use std::path::{Path, PathBuf};

use crate::services::data_paths::DataPaths;

#[derive(Clone, Debug)]
pub struct BlogLayout {
    pub root: PathBuf,
    pub hugo_cache: PathBuf,
}

impl BlogLayout {
    /// Hugo 在站点目录里运行，相对路径会被它按那里解析，所以一律转成绝对路径。
    pub fn from_data_paths(paths: &DataPaths) -> Self {
        Self {
            root: absolute(&paths.blog),
            hugo_cache: absolute(&paths.cache.join("blog-hugo")),
        }
    }

    pub fn themes_dir(&self) -> PathBuf {
        self.root.join("themes")
    }

    pub fn theme_dir(&self, id: &str) -> PathBuf {
        self.themes_dir().join(id)
    }

    pub fn work_dir(&self) -> PathBuf {
        self.root.join("work")
    }

    pub fn current_dir(&self) -> PathBuf {
        self.root.join("site").join("current")
    }

    /// 放在 `current` 外面，不会被 `/blog/` 当静态文件发出去。
    pub fn stamp_path(&self) -> PathBuf {
        self.root.join("site").join("stamp.json")
    }
}

pub fn layout() -> BlogLayout {
    BlogLayout::from_data_paths(crate::services::data_paths::paths())
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// 把 `fresh` 换成当前输出，然后删掉旧目录。失败时尽量把旧目录放回去。
pub fn publish_tree(current: &Path, fresh: &Path) -> Result<(), &'static str> {
    let Some(parent) = current.parent() else {
        return Err("Could not publish the blog");
    };
    std::fs::create_dir_all(parent).map_err(|_| "Could not publish the blog")?;
    // A unique name, not a time: the host's clock is enough.
    #[allow(clippy::disallowed_methods)]
    let retired = parent.join(format!(
        "retired-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    let had_current = current.exists();
    if had_current {
        std::fs::rename(current, &retired).map_err(|_| "Could not publish the blog")?;
    }
    if std::fs::rename(fresh, current).is_err() {
        if had_current {
            let _ = std::fs::rename(&retired, current);
        }
        return Err("Could not publish the blog");
    }
    if had_current {
        if let Err(error) = std::fs::remove_dir_all(&retired) {
            tracing::warn!(%error, "failed to delete the previous blog output");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_data_dirs_become_absolute() {
        let data = PathBuf::from("data");
        let cache = PathBuf::from("cache");
        let paths = DataPaths {
            root: data.clone(),
            phantasi: data.join("phantasi"),
            phantasi_icons: data.join("phantasi/icons"),
            tapps: data.join("tapps"),
            agent: data.join("agent"),
            cache: cache.clone(),
            cache_platforms: cache.join("platforms"),
            cache_raw: cache.join("raw"),
            cache_images: cache.join("images"),
            widget_fonts: data.join("site/widget-fonts"),
            media: data.join("media"),
            blog: data.join("blog"),
        };
        let layout = BlogLayout::from_data_paths(&paths);
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(layout.root, cwd.join("data/blog"));
        assert_eq!(layout.hugo_cache, cwd.join("cache/blog-hugo"));
        assert!(layout.work_dir().is_absolute());
    }

    #[test]
    fn publish_replaces_and_deletes_the_previous_tree() {
        let root = std::env::temp_dir().join(format!(
            "myriad-blog-publish-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let current = root.join("site/current");
        let fresh = root.join("site/fresh");
        std::fs::create_dir_all(current.join("old")).unwrap();
        std::fs::write(current.join("old/index.html"), "old").unwrap();
        std::fs::create_dir_all(&fresh).unwrap();
        std::fs::write(fresh.join("index.html"), "new").unwrap();

        publish_tree(&current, &fresh).unwrap();
        assert_eq!(
            std::fs::read_to_string(current.join("index.html")).unwrap(),
            "new"
        );
        assert!(!current.join("old").exists());
        assert!(!fresh.exists());
        let leftovers: Vec<_> = std::fs::read_dir(root.join("site"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("retired-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        std::fs::remove_dir_all(root).unwrap();
    }
}
