//! 博客导出的纯规则。
//!
//! 没有 I/O：不读库、不写文件、不跑 Hugo。后端拿「相对路径 → 内容」去落盘，
//! 构建出的 HTML 再交回 [`postprocess_html`]。

mod archive;
mod config;
mod cover;
mod inspect;
mod model;
mod postprocess;
mod render;
mod rewrite;
mod slug;
mod studio;
mod summary;
mod theme;
mod urlutil;

#[cfg(test)]
mod tests;

pub use archive::{
    archive_entry_path, github_archive_root, safe_archive_entries, safe_archive_path,
};
pub use config::{HUGO_THEMES_DIR, POSTS_PERMALINK, blog_base_url, render_hugo_config};
pub use cover::{cover_front_matter, covers_same_asset};
pub use inspect::{
    MAX_THEME_ARCHIVE_BYTES, MAX_THEME_ARCHIVE_ENTRIES, MAX_THEME_BYTES, MAX_THEME_FILES,
    ThemeFile, ThemeInfo, ThemeInspectError, inspect_theme, is_theme_archive_noise,
};
pub use model::{
    BLOG_SETTINGS_KEY, BlogPost, BlogSettings, BlogSite, DEFAULT_THEME_ID, FriendLink,
    blog_settings_from_value, blog_settings_to_value,
};
pub use postprocess::{
    BLOG_CSP, NOTES_CSS, NOTES_CSS_HREF, is_script_asset, is_stripped_asset, postprocess_html,
};
pub use render::{export_site_files, render_friends_file, render_home_file, render_post_file};
pub use rewrite::{first_content_image, prepare_content_html};
pub use slug::{
    BLOG_PREFIX, SLUG_MAX_CHARS, dedupe_slug, fallback_slug, note_blog_path, post_path,
    slugify_title, validate_slug,
};
pub use studio::{
    STUDIO_EMBED_ROOT, StudioIslandLabels, inject_studio_island, studio_frame_src, studio_island_html,
    studio_island_labels, studio_only_page,
};
pub use summary::{SUMMARY_MAX_CHARS, summarize};
pub use theme::{
    ThemeManifestError, ThemeMeta, default_theme_files, inspect_theme_manifest, sanitize_theme_id,
};
pub use urlutil::absolutize_root_url;
