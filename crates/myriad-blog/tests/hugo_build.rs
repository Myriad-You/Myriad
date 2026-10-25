//! 有 `HUGO_BIN` 才跑。额外主题用 `HUGO_EXTRA_THEMES=id=/绝对路径,id2=/绝对路径`。
//! 子进程先清空环境，只留 `PATH`、`HOME`、`TMPDIR`。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use myriad_blog::{
    BlogPost, BlogSettings, BlogSite, DEFAULT_THEME_ID, FriendLink, NOTES_CSS_HREF,
    default_theme_files, postprocess_page, render_friends_file, render_home_file,
    render_hugo_config, render_list_titles, render_post_file,
};

#[test]
fn hugo_builds_exported_site() {
    let Ok(hugo) = std::env::var("HUGO_BIN") else {
        eprintln!("HUGO_BIN unset; skip hugo build");
        return;
    };
    if hugo.trim().is_empty() {
        eprintln!("HUGO_BIN empty; skip hugo build");
        return;
    }

    build_case(&hugo, DEFAULT_THEME_ID, None);
    if let Ok(spec) = std::env::var("HUGO_EXTRA_THEMES") {
        for item in spec
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let Some((id, path)) = item.split_once('=') else {
                panic!("HUGO_EXTRA_THEMES 条目要写成 id=/绝对路径，收到 {item}");
            };
            build_case(&hugo, id.trim(), Some(Path::new(path.trim())));
        }
    }
}

fn build_case(hugo: &str, theme_id: &str, theme_dir: Option<&Path>) {
    let root = std::env::temp_dir().join(format!(
        "myriad-blog-hugo-{}-{}",
        std::process::id(),
        theme_id.replace('/', "_")
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let keep = std::env::var_os("HUGO_KEEP").is_some();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        build_in(&root, hugo, theme_id, theme_dir);
    }));
    if outcome.is_err() && keep {
        eprintln!("保留构建目录 {}", root.display());
    }
    if !keep {
        let _ = fs::remove_dir_all(&root);
    }
    if let Err(err) = outcome {
        std::panic::resume_unwind(err);
    }
}

fn build_in(root: &Path, hugo: &str, theme_id: &str, theme_dir: Option<&Path>) {
    let site_dir = root.join("site");
    fs::create_dir_all(&site_dir).unwrap();
    let home = root.join("home");
    let tmp = root.join("tmp");
    let cache = root.join("cache");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&tmp).unwrap();
    fs::create_dir_all(&cache).unwrap();

    let blog = BlogSite {
        origin: "https://example.com".to_string(),
        title: "站点标题".to_string(),
        description: "一段说明".to_string(),
        locale: "zh-cn".to_string(),
        params: serde_json::json!({}),
        icon: "/favicon.webp".to_string(),
        image: String::new(),
        feed: "/api/phantasi/notes.xml".to_string(),
        noindex: false,
    };
    let settings = BlogSettings {
        theme_id: theme_id.to_string(),
        site_title: "站长的博客".to_string(),
        description: "一段说明".to_string(),
        params: serde_json::json!({
            "security": { "enableInlineShortcodes": true },
            "subtitle": "不会进安全配置",
        }),
    };
    let note = BlogPost {
        item_id: 42,
        slug: "math-note".to_string(),
        title: "公式测试".to_string(),
        published_at: "2026-10-05T10:00:00+09:00".to_string(),
        updated_at: "2026-10-05T10:00:00+09:00".to_string(),
        content_html:
            r#"<p>行内公式 <span class="math math-inline" data-tex="E=mc^2">E=mc^2</span>。</p>
<p><img src="/media/assets/abc123/cover.jpg" alt="封面"></p>"#
                .to_string(),
        summary_source: String::new(),
        category: Some("技术".to_string()),
        authors: vec!["站长".to_string()],
        cover: Some("/media/assets/abc123/cover.jpg".to_string()),
        locale: "zh-CN".to_string(),
    };
    let links = [FriendLink {
        name: "示例友站".to_string(),
        url: "https://friend.example/".to_string(),
        description: "友站说明".to_string(),
        icon: String::new(),
    }];

    let config = render_hugo_config(&blog, &settings, theme_id);
    fs::write(site_dir.join("hugo.toml"), config).unwrap();
    write_content(&site_dir, render_home_file(&blog, &settings));
    write_content(&site_dir, render_friends_file(&blog, &links));
    write_content(&site_dir, render_post_file(&blog, &note, theme_id));
    for file in render_list_titles(&blog) {
        write_content(&site_dir, file);
    }

    let theme_root = site_dir.join("themes").join(theme_id);
    if let Some(dir) = theme_dir {
        copy_dir(dir, &theme_root).unwrap_or_else(|err| {
            panic!("复制主题 {} 失败: {err}", dir.display());
        });
    } else {
        for (rel, content) in default_theme_files() {
            let path = theme_root.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }
    }

    let mut cmd = Command::new(hugo);
    cmd.env_clear()
        .env("PATH", child_path())
        .env("HOME", &home)
        .env("TMPDIR", &tmp)
        .current_dir(&site_dir)
        .arg("build")
        .arg("--source")
        .arg(&site_dir)
        .arg("--destination")
        .arg(site_dir.join("public"))
        .arg("--cacheDir")
        .arg(&cache)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd
        .output()
        .unwrap_or_else(|err| panic!("启动 Hugo 失败 ({theme_id}): {err}"));
    if !output.status.success() {
        panic!(
            "Hugo 构建失败 theme={theme_id} status={:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            output.status.code(),
            tail(&output.stdout),
            tail(&output.stderr)
        );
    }

    let page_path = site_dir.join("public/posts/math-note/index.html");
    assert!(
        page_path.is_file(),
        "theme={theme_id} 缺少文章页\n{}",
        list_html(&site_dir.join("public"))
    );
    let page = fs::read_to_string(&page_path).unwrap();
    assert!(
        page.contains("<math"),
        "theme={theme_id} 没有 MathML\n{page}"
    );
    assert!(
        page.contains("https://example.com/media/assets/abc123/cover.jpg"),
        "theme={theme_id} 媒体地址没有改成绝对地址\n{page}"
    );
    assert!(
        page.contains("/embed/comments/42"),
        "theme={theme_id} 缺少评论 iframe\n{page}"
    );
    assert!(
        page.contains("src=\"/embed/companion/42\""),
        "theme={theme_id} 缺少伴读岛\n{page}"
    );

    // Hugo 不出订阅和站点地图：订阅只走笔记 RSS，站点地图由主站给。
    let public = site_dir.join("public");
    assert!(
        !public.join("index.xml").exists(),
        "theme={theme_id} 还在出 RSS"
    );
    assert!(
        !public.join("sitemap.xml").exists(),
        "theme={theme_id} 还在出站点地图"
    );

    let cleaned = postprocess_page(&page, "posts/math-note/index.html", &blog);
    assert_eq!(
        cleaned.matches("rel=\"canonical\"").count(),
        1,
        "theme={theme_id} canonical 不是正好一个\n{cleaned}"
    );
    assert!(
        cleaned.contains("https://example.com/blog/posts/math-note/"),
        "theme={theme_id} canonical 不对\n{cleaned}"
    );
    assert!(
        cleaned.contains("href=\"https://example.com/api/phantasi/notes.xml\""),
        "theme={theme_id} 没挂笔记 RSS\n{cleaned}"
    );
    assert!(
        !cleaned.contains("index.xml\""),
        "theme={theme_id} 残留 Hugo 订阅链接\n{cleaned}"
    );
    assert!(!cleaned.contains("name=\"robots\""), "{cleaned}");
    assert!(cleaned.contains("<math"));
    assert!(cleaned.contains(NOTES_CSS_HREF));
    assert_no_active_scripts(&cleaned);
    let lower = cleaned.to_ascii_lowercase();
    assert!(!lower.contains("javascript:"));
    assert!(!lower.contains(" onclick"));

    if theme_id == DEFAULT_THEME_ID {
        assert!(page.contains("myriad-post"), "{page}");
        assert!(page.contains("<html lang=\"zh-CN\">"), "{page}");
        assert!(
            page.contains("<title>公式测试 · 站长的博客</title>"),
            "{page}"
        );
        assert!(
            page.contains("property=\"og:type\" content=\"article\""),
            "{page}"
        );
        assert!(
            page.contains("property=\"og:locale\" content=\"zh_CN\""),
            "{page}"
        );
        assert!(
            page.contains("property=\"og:image\" content=\"https://example.com/media/assets/abc123/cover.jpg\""),
            "{page}"
        );
        assert!(
            page.contains("rel=\"icon\" href=\"https://example.com/favicon.webp\""),
            "{page}"
        );
        assert!(page.contains("2026年10月5日"), "{page}");
        let ld = json_ld(&page);
        assert_eq!(ld["@type"], "BlogPosting", "{page}");
        assert_eq!(ld["headline"], "公式测试");
        assert_eq!(ld["url"], "https://example.com/blog/posts/math-note/");
        assert_eq!(ld["author"][0]["name"], "站长");
        assert_eq!(ld["inLanguage"], "zh-CN");
        assert_eq!(ld["articleSection"], "技术");
        assert!(
            ld["datePublished"]
                .as_str()
                .unwrap()
                .starts_with("2026-10-05T10:00:00")
        );
        let home = fs::read_to_string(site_dir.join("public/index.html"))
            .unwrap_or_else(|err| panic!("首页: {err}"));
        assert!(home.contains("class=\"myriad\""), "{home}");
        assert!(home.contains("/blog/friends/"), "{home}");
        assert!(home.contains("css/style.css"), "{home}");
        assert!(home.contains("<title>站长的博客</title>"), "{home}");
        assert_eq!(json_ld(&home)["@type"], "Blog", "{home}");
        assert!(
            home.contains("property=\"og:locale\" content=\"zh_CN\""),
            "{home}"
        );
        let posts = postprocess_page(
            &fs::read_to_string(public.join("posts/index.html")).unwrap(),
            "posts/index.html",
            &blog,
        );
        assert!(posts.contains("content=\"noindex, follow\""), "{posts}");
        assert!(
            posts.contains("<title>文章 · 站长的博客</title>"),
            "{posts}"
        );
        let missing = fs::read_to_string(public.join("404.html")).unwrap();
        assert!(missing.contains("<title>找不到页面</title>"), "{missing}");
        assert!(missing.contains("回到首页"), "{missing}");
        let friends = fs::read_to_string(site_dir.join("public/friends/index.html"))
            .unwrap_or_else(|err| panic!("友链页: {err}"));
        assert!(friends.contains("/embed/friend-apply"), "{friends}");
        assert!(friends.contains("示例友站"), "{friends}");
    }
}

fn json_ld(html: &str) -> serde_json::Value {
    let start = html
        .find("application/ld+json\">")
        .unwrap_or_else(|| panic!("没有 JSON-LD\n{html}"))
        + "application/ld+json\">".len();
    let end = start + html[start..].find("</script>").unwrap();
    serde_json::from_str(&html[start..end])
        .unwrap_or_else(|err| panic!("JSON-LD 解析失败 {err}\n{html}"))
}

fn write_content(site: &Path, (rel, body): (String, String)) {
    let path = site.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
}

fn child_path() -> String {
    let mut paths = Vec::new();
    if let Some(dir) = std::env::var_os("DART_SASS_DIR") {
        paths.push(PathBuf::from(dir));
    }
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(paths)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn assert_no_active_scripts(html: &str) {
    let lower = html.to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(index) = rest.find("<script") {
        let after = &rest[index..];
        let end = after.find('>').unwrap_or(after.len().saturating_sub(1));
        let open = &after[..=end.min(after.len().saturating_sub(1))];
        assert!(
            open.contains("application/ld+json") || open.contains("application/json"),
            "残留可执行脚本: {open}"
        );
        rest = &after[end.saturating_add(1)..];
    }
}

fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let count = text.chars().count();
    if count <= 2000 {
        text
    } else {
        text.chars().skip(count - 2000).collect()
    }
}

fn list_html(dir: &Path) -> String {
    let mut found = Vec::new();
    walk(dir, &mut found);
    found.join("\n")
}

fn walk(dir: &Path, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if found.len() >= 40 {
            return;
        }
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("html") {
            found.push(path.display().to_string());
        }
    }
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else if file_type.is_symlink() {
            let path = entry.path();
            if path.is_dir() {
                copy_dir(&path, &to)?;
            } else {
                fs::copy(&path, &to)?;
            }
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}
