//! 有 `HUGO_BIN` 且 `/tmp/hugo-spike/themes/hugo-PaperMod` 在时才跑。
//! 证明：装上第三方主题 zip → Hugo 构建 → 输出没有主题脚本，有 notes.css 和评论 iframe。

use std::fs;
use std::io::Write;
use std::path::Path;

use myriad_blog::{BlogPost, BlogSettings, BlogSite, FriendLink};
use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::build::render_and_build;
use super::layout::BlogLayout;
use super::themes::install_zip;

#[tokio::test]
async fn papermod_zip_build_strips_scripts_and_keeps_notes_css() {
    let Ok(hugo) = std::env::var("HUGO_BIN") else {
        eprintln!("HUGO_BIN unset; skip blog hugo e2e");
        return;
    };
    if hugo.trim().is_empty() {
        eprintln!("HUGO_BIN empty; skip blog hugo e2e");
        return;
    }
    let source = Path::new("/tmp/hugo-spike/themes/hugo-PaperMod");
    if !source.join("theme.toml").is_file() {
        eprintln!("PaperMod theme is not on disk; skip blog hugo e2e");
        return;
    }

    let root =
        std::env::temp_dir().join(format!("myriad-blog-e2e-{}", uuid::Uuid::new_v4().simple()));
    let layout = BlogLayout {
        root: root.join("blog"),
        hugo_cache: root.join("cache"),
    };
    let packed = zip_theme(source);
    let theme = install_zip(&layout, &packed).expect("install PaperMod");
    let site = BlogSite {
        origin: "https://example.com".to_string(),
        title: "站点".to_string(),
        description: "说明".to_string(),
        locale: "zh-cn".to_string(),
        params: serde_json::json!({}),
        ..BlogSite::default()
    };
    let settings = BlogSettings {
        theme_id: theme.id.clone(),
        site_title: "站点".to_string(),
        description: "说明".to_string(),
        params: serde_json::json!({}),
    };
    let posts = [BlogPost {
        item_id: 7,
        slug: "hello".to_string(),
        title: "Hello".to_string(),
        published_at: "2026-10-05T10:00:00+09:00".to_string(),
        updated_at: "2026-10-05T10:00:00+09:00".to_string(),
        content_html: "<p>正文</p>".to_string(),
        summary_source: String::new(),
        category: None,
        authors: vec!["站长".to_string()],
        cover: None,
        companion: true,
        locale: String::new(),
    }];
    let friends = [FriendLink {
        name: "友站".to_string(),
        url: "https://friend.example/".to_string(),
        description: String::new(),
        icon: String::new(),
    }];
    let built = render_and_build(&layout, &site, &settings, &posts, &friends)
        .await
        .unwrap_or_else(|error| panic!("build failed: {error:?}"));
    assert!(!built.reused);
    let again = render_and_build(&layout, &site, &settings, &posts, &friends)
        .await
        .unwrap_or_else(|error| panic!("second build failed: {error:?}"));
    assert!(
        again.reused,
        "unchanged inputs should keep the existing output"
    );
    assert_eq!(again.built_at, built.built_at);
    let page =
        fs::read_to_string(layout.current_dir().join("posts/hello/index.html")).expect("post page");
    assert!(page.contains("/embed/comments/7"), "{page}");
    assert!(page.contains("src=\"/embed/companion/7\""), "{page}");
    assert!(page.contains(myriad_blog::NOTES_CSS_HREF), "{page}");
    assert!(
        layout.current_dir().join("_myriad/notes.css").is_file(),
        "notes.css missing"
    );
    assert_no_theme_script(&page);
    assert!(
        walk_has_no_script_asset(&layout.current_dir()),
        "theme script file survived"
    );
    let friends_page =
        fs::read_to_string(layout.current_dir().join("friends/index.html")).expect("friends page");
    assert!(
        friends_page.contains("/embed/friend-apply"),
        "{friends_page}"
    );
    fs::remove_dir_all(root).ok();
}

fn assert_no_theme_script(html: &str) {
    let lower = html.to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(index) = rest.find("<script") {
        let after = &rest[index..];
        let end = after.find('>').unwrap_or(after.len().saturating_sub(1));
        let open = &after[..=end.min(after.len().saturating_sub(1))];
        assert!(
            open.contains("application/ld+json") || open.contains("application/json"),
            "executable script left in the page: {open}"
        );
        rest = &after[end.saturating_add(1)..];
    }
}

fn walk_has_no_script_asset(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return true;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && !walk_has_no_script_asset(&path) {
            return false;
        }
        let name = path.to_string_lossy().replace('\\', "/");
        if myriad_blog::is_script_asset(&name) {
            return false;
        }
    }
    true
}

fn zip_theme(source: &Path) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(&mut cursor);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut stack = vec![source.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push(path);
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let rel = path.strip_prefix(source).unwrap();
            let name = format!(
                "PaperMod-master/{}",
                rel.to_string_lossy().replace('\\', "/")
            );
            writer.start_file(name, options).unwrap();
            writer.write_all(&fs::read(&path).unwrap()).unwrap();
        }
    }
    writer.finish().unwrap();
    cursor.into_inner()
}
