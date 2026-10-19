use std::path::Path;

use serde_json::{Value, json};

use crate::{
    BLOG_CSP, BLOG_PREFIX, BLOG_SETTINGS_KEY, BlogPost, BlogSettings, BlogSite, DEFAULT_THEME_ID,
    FriendLink, HUGO_THEMES_DIR, NOTES_CSS, NOTES_CSS_HREF, POSTS_PERMALINK, SLUG_MAX_CHARS,
    SUMMARY_MAX_CHARS, ThemeManifestError, absolutize_root_url, archive_entry_path, blog_base_url,
    blog_settings_from_value, blog_settings_to_value, cover_front_matter, covers_same_asset,
    dedupe_slug, default_theme_files, fallback_slug, first_content_image, github_archive_root,
    inspect_theme_manifest, is_script_asset, post_path, postprocess_html, prepare_content_html,
    render_friends_file, render_home_file, render_hugo_config, render_post_file,
    reveal_companion_island, safe_archive_entries, safe_archive_path, sanitize_theme_id,
    slugify_title, summarize, validate_slug,
};

fn site() -> BlogSite {
    BlogSite {
        origin: "https://example.com".to_string(),
        title: "站点标题".to_string(),
        description: "站点说明".to_string(),
        locale: "zh-cn".to_string(),
        params: json!({}),
    }
}

fn post() -> BlogPost {
    BlogPost {
        item_id: 42,
        slug: "hello-world".to_string(),
        title: "你好".to_string(),
        published_at: "2026-10-05T10:00:00+09:00".to_string(),
        updated_at: "2026-10-06T10:00:00+09:00".to_string(),
        content_html: "<p>正文</p>".to_string(),
        summary_source: String::new(),
        category: Some("随笔".to_string()),
        authors: vec!["站长".to_string(), "  ".to_string(), "乙".to_string()],
        cover: None,
        companion: false,
    }
}

#[test]
fn the_companion_loads_only_on_posts_that_have_one() {
    let mut note = post();
    let (_, file) = render_post_file(&site(), &note, DEFAULT_THEME_ID);
    assert!(
        file.contains("data-myriad-src=\"/embed/companion/42\""),
        "{file}"
    );
    assert!(!file.contains(" src=\"/embed/companion/42\""), "{file}");
    let revealed = reveal_companion_island(&file);
    assert!(
        revealed.contains("<iframe class=\"myriad-companion__frame\" src=\"/embed/companion/42\"")
    );
    assert_eq!(
        reveal_companion_island("<p>&lt;iframe&gt;</p>"),
        "<p>&lt;iframe&gt;</p>"
    );

    note.companion = true;
    let (_, file) = render_post_file(&site(), &note, DEFAULT_THEME_ID);
    assert!(
        file.contains("<iframe class=\"myriad-companion__frame\" src=\"/embed/companion/42\""),
        "{file}"
    );
    assert!(
        file.contains("hidden aria-hidden=\"true\" tabindex=\"-1\""),
        "{file}"
    );
    assert!(!file.contains("loading=\"lazy\" title=\"AI"), "{file}");
    assert!(NOTES_CSS.contains("mark.myriad-annot{"));
}

fn front_matter(file: &str) -> Value {
    serde_json::Deserializer::from_str(file)
        .into_iter::<Value>()
        .next()
        .expect("front matter")
        .expect("json")
}

#[test]
fn settings_round_trip_and_dirty_values() {
    assert_eq!(BLOG_SETTINGS_KEY, "blog_settings");
    assert_eq!(DEFAULT_THEME_ID, "myriad-default");

    let value = json!({
        "theme_id": " FixIt ",
        "site_title": " 站 ",
        "description": "说明",
        "params": {"subtitle": "你好", "n": 1},
        "ignored": true,
    });
    let settings = blog_settings_from_value(&value);
    assert_eq!(settings.theme_id, "FixIt");
    assert_eq!(settings.site_title, "站");
    assert_eq!(settings.description, "说明");
    assert_eq!(settings.params["subtitle"], "你好");
    assert_eq!(
        blog_settings_from_value(&blog_settings_to_value(&settings)),
        settings
    );

    let dirty = blog_settings_from_value(&json!("nope"));
    assert_eq!(dirty, BlogSettings::default());
    let dirty = blog_settings_from_value(&json!({
        "theme_id": 1,
        "site_title": ["x"],
        "description": null,
        "params": [1],
        "extra": true,
    }));
    assert_eq!(dirty.theme_id, DEFAULT_THEME_ID);
    assert_eq!(dirty.site_title, "");
    assert_eq!(dirty.description, "");
    assert_eq!(dirty.params, json!({}));

    let dirty = blog_settings_from_value(&json!({"theme_id": "../evil"}));
    assert_eq!(dirty.theme_id, DEFAULT_THEME_ID);

    let crafted = BlogSettings {
        theme_id: "../evil".to_string(),
        ..BlogSettings::default()
    };
    let read_back = blog_settings_from_value(&blog_settings_to_value(&crafted));
    assert_eq!(read_back.theme_id, DEFAULT_THEME_ID);
}

#[test]
fn slug_rules() {
    assert_eq!(
        slugify_title("Hello, World!").as_deref(),
        Some("hello-world")
    );
    assert_eq!(
        slugify_title("  Café Latte ").as_deref(),
        Some("cafe-latte")
    );
    assert_eq!(slugify_title("Straße").as_deref(), Some("strasse"));
    assert_eq!(slugify_title("Ｈｅｌｌｏ").as_deref(), Some("hello"));
    assert_eq!(slugify_title("foo---bar").as_deref(), Some("foo-bar"));
    assert_eq!(slugify_title("Hello 世界").as_deref(), Some("hello"));
    assert_eq!(slugify_title("A世界B").as_deref(), Some("a-b"));
    assert_eq!(slugify_title("你好").as_deref(), None);
    assert_eq!(slugify_title("---").as_deref(), None);
    assert_eq!(slugify_title("").as_deref(), None);
    assert_eq!(slugify_title("123").as_deref(), Some("123"));

    let slug = slugify_title(&"a".repeat(90)).unwrap();
    assert_eq!(slug.chars().count(), SLUG_MAX_CHARS);
    assert!(validate_slug(&slug));

    assert!(validate_slug("hello-world"));
    assert!(validate_slug("n12"));
    assert!(!validate_slug("Hello"));
    assert!(!validate_slug("-hello"));
    assert!(!validate_slug("hello-"));
    assert!(!validate_slug("hello--world"));
    assert!(!validate_slug(""));
    assert!(!validate_slug(&"a".repeat(81)));

    assert_eq!(fallback_slug(12), "n12");
    assert_eq!(post_path("hello"), "/blog/posts/hello/");
    assert_eq!(BLOG_PREFIX, "/blog");

    assert_eq!(dedupe_slug("hello", |_| false), "hello");
    assert_eq!(dedupe_slug("hello", |s| s == "hello"), "hello-2");
    assert_eq!(
        dedupe_slug("hello", |s| s == "hello" || s == "hello-2"),
        "hello-3"
    );
    let deduped = dedupe_slug(&"b".repeat(80), |_| true);
    assert!(validate_slug(&deduped), "{deduped}");
    assert!(deduped.chars().count() <= SLUG_MAX_CHARS);
    assert!(deduped.ends_with("-10000"));
}

#[test]
fn math_urls_and_comment_iframe() {
    let html = r##"<p>行内 <span class="math math-inline" data-tex="E=mc^2">E=mc^2</span>。</p>
<p><span class="math math-display" data-tex="\int_0^1 x^2 \, dx">\int_0^1 x^2 \, dx</span></p>
<p><span class="math math-inline" data-tex="\frac{">\frac{</span></p>
<p><a href="/library">内</a> <a href="//cdn.example/a">c</a> <a href="https://gohugo.io/">g</a> <a href="#top">t</a></p>
<img src="/media/a.jpg" srcset="/a.jpg 1x, https://cdn.example/b.jpg 2x">
<video poster="/p.jpg" src="/v.mp4"></video>"##;
    let out = prepare_content_html(html, "https://example.com/", 7);
    assert!(!out.contains("<html"), "{out}");
    assert!(out.contains("display=\"inline\""), "{out}");
    assert!(out.contains("display=\"block\""), "{out}");
    assert!(out.contains("data-tex="), "{out}");
    assert!(out.contains("https://example.com/library"), "{out}");
    assert!(out.contains("//cdn.example/a"), "{out}");
    assert!(out.contains("https://gohugo.io/"), "{out}");
    assert!(out.contains("#top"), "{out}");
    assert!(out.contains("https://example.com/media/a.jpg"), "{out}");
    assert!(out.contains("https://example.com/a.jpg"), "{out}");
    assert!(out.contains("https://cdn.example/b.jpg"), "{out}");
    assert!(out.contains("https://example.com/p.jpg"), "{out}");
    assert!(out.contains("https://example.com/v.mp4"), "{out}");
    assert!(out.contains("src=\"/embed/comments/7\""), "{out}");
    assert!(out.contains("data-embed-fit></iframe>"), "{out}");
    assert!(
        !out.contains("https://example.com/embed/comments/7"),
        "{out}"
    );
    assert_eq!(
        absolutize_root_url("//cdn.example/a", "https://example.com"),
        "//cdn.example/a"
    );
    assert_eq!(absolutize_root_url("/media/a.jpg", ""), "/media/a.jpg");
}

#[test]
fn summary_drops_tex_and_truncates_by_char() {
    let html = r#"<p>前文 <span class="math math-inline" data-tex="E=mc^2">E=mc^2</span> 后文</p><p>下一段</p>"#;
    let summary = summarize(html);
    assert_eq!(summary, "前文 后文 下一段");
    assert!(!summary.contains("mc"));
    assert_eq!(summarize("你好   世界"), "你好 世界");
    assert_eq!(summarize("<p>a &amp; b</p>"), "a & b");
    let long = "字".repeat(SUMMARY_MAX_CHARS + 20);
    assert_eq!(summarize(&long).chars().count(), SUMMARY_MAX_CHARS);
    assert!(summarize("   ").is_empty());
}

#[test]
fn summary_drops_footnote_markers_and_definitions() {
    let html = concat!(
        r##"<p>想什么。<sup class="footnote-reference"><a href="#note-fn-a">1</a></sup> 我信这句。</p>"##,
        r#"<div class="link-definition"><a href="https://example.com">[a] 链接</a></div>"#,
        r#"<div class="footnote-definition" id="note-fn-a"><sup class="footnote-definition-label">1</sup><p>出处</p></div>"#,
    );
    assert_eq!(summarize(html), "想什么。 我信这句。");
}

#[test]
fn cover_dedup_and_theme_keys() {
    let html = r#"<p><img src="/media/a.jpg" alt=""></p><img src="/media/b.jpg">"#;
    assert_eq!(first_content_image(html).as_deref(), Some("/media/a.jpg"));
    assert!(covers_same_asset(
        "https://example.com",
        "/media/a.jpg",
        html
    ));
    assert!(covers_same_asset(
        "https://example.com",
        "https://example.com/media/a.jpg",
        html
    ));
    assert!(!covers_same_asset(
        "https://example.com",
        "/media/b.jpg",
        html
    ));
    assert!(!covers_same_asset(
        "https://example.com",
        "https://example.com.evil/media/a.jpg",
        html
    ));

    let image = "https://example.com/cover.jpg";
    let papermod = cover_front_matter("hugo-PaperMod", image);
    assert_eq!(papermod["cover"]["image"], image);
    assert!(papermod.get("image").is_none());
    assert_eq!(papermod["images"][0], image);

    let stack = cover_front_matter("hugo-theme-stack", image);
    assert_eq!(stack["image"], image);
    assert!(stack.get("cover").is_none());

    let ananke = cover_front_matter("gohugo-theme-ananke", image);
    assert_eq!(ananke["featured_image"], image);
    let fixit = cover_front_matter("FixIt", image);
    assert_eq!(fixit["featured_image"], image);
    let blowfish = cover_front_matter("blowfish", image);
    assert_eq!(blowfish["featureimage"], image);

    let unknown = cover_front_matter(DEFAULT_THEME_ID, image);
    for key in ["image", "featured_image", "featureimage", "cover", "images"] {
        assert!(unknown.contains_key(key), "{key}");
    }
}

#[test]
fn post_file_uses_json_front_matter_and_skips_duplicate_cover() {
    let mut note = post();
    note.content_html = r#"<p><img src="/media/a.jpg" alt="封面"></p><p>正文</p>"#.to_string();
    note.cover = Some("/media/a.jpg".to_string());
    note.summary_source =
        r#"<p>摘要 <span class="math math-inline" data-tex="E=mc^2">E=mc^2</span> 完</p>"#
            .to_string();
    let (path, file) = render_post_file(&site(), &note, "hugo-PaperMod");
    assert_eq!(path, "content/posts/hello-world/index.html");
    assert!(file.starts_with('{'));
    let front = front_matter(&file);
    assert_eq!(front["title"], "你好");
    assert_eq!(front["date"], "2026-10-05T10:00:00+09:00");
    assert_eq!(front["lastmod"], "2026-10-06T10:00:00+09:00");
    assert_eq!(front["slug"], "hello-world");
    assert_eq!(front["summary"], "摘要 完");
    assert_eq!(front["description"], "摘要 完");
    assert_eq!(front["categories"][0], "随笔");
    assert_eq!(front["author"], "站长");
    assert_eq!(front["authors"][1], "乙");
    assert!(front.get("cover").is_none());
    assert!(front.get("image").is_none());
    assert!(file.contains("https://example.com/media/a.jpg"));
    assert!(file.contains("src=\"/embed/comments/42\""));

    note.cover = Some("/media/other.jpg".to_string());
    note.slug = "../etc/passwd".to_string();
    let (path, file) = render_post_file(&site(), &note, "hugo-theme-stack");
    assert_eq!(path, "content/posts/n42/index.html");
    let front = front_matter(&file);
    assert_eq!(front["slug"], "n42");
    assert_eq!(front["image"], "https://example.com/media/other.jpg");
    assert!(front.get("cover").is_none());
}

#[test]
fn home_and_friends_files() {
    let settings = BlogSettings {
        site_title: "站长的博客".to_string(),
        description: "一段 <说明>".to_string(),
        ..BlogSettings::default()
    };
    let (path, file) = render_home_file(&site(), &settings);
    assert_eq!(path, "content/_index.html");
    assert_eq!(front_matter(&file)["title"], "站长的博客");
    assert!(file.contains("&lt;说明&gt;"));

    let links = vec![
        FriendLink {
            name: "<脚本>".to_string(),
            url: "https://friend.example/?a=1&b=2".to_string(),
            description: "甲 & 乙".to_string(),
            icon: "/media/icon.png".to_string(),
        },
        FriendLink {
            name: "坏链".to_string(),
            url: "javascript:alert(1)".to_string(),
            description: String::new(),
            icon: String::new(),
        },
    ];
    let (path, file) = render_friends_file(&site(), &links);
    assert_eq!(path, "content/friends/index.html");
    assert_eq!(front_matter(&file)["title"], "友链");
    assert!(file.contains("&lt;脚本&gt;"));
    assert!(file.contains("a=1&amp;b=2"));
    assert!(file.contains("rel=\"noopener\""));
    assert!(file.contains("甲 &amp; 乙"));
    assert!(file.contains("https://example.com/media/icon.png"));
    assert!(file.contains("href=\"#\""));
    assert!(file.contains("class=\"myriad-friend-apply\" src=\"/embed/friend-apply\""));
    assert!(file.contains("title=\"友联申请\" data-embed-fit></iframe>"));
    assert!(!file.contains("javascript:"));
}

#[test]
fn hugo_config_ignores_owner_security() {
    let mut blog = site();
    blog.locale = String::new();
    blog.params = json!({"fromSite": true, "baseURL": "https://site.example"});
    let settings = BlogSettings {
        theme_id: DEFAULT_THEME_ID.to_string(),
        site_title: "站长的博客".to_string(),
        description: "说明".to_string(),
        params: json!({
            "security": {"enableInlineShortcodes": true, "exec": {"allow": ["^sh$"]}},
            "baseURL": "https://evil.example/",
            "themesDir": "/tmp",
            "theme": "evil",
            "permalinks": {"posts": "/pwned/"},
            "subtitle": "你好",
            "mainSections": ["notes"],
        }),
    };
    let text = render_hugo_config(&blog, &settings, "../evil");
    assert!(!text.contains("languageCode"), "{text}");
    let root: toml::Table = toml::from_str(&text).expect("toml");
    assert_eq!(root["baseURL"].as_str(), Some("https://example.com/blog/"));
    assert_eq!(
        blog_base_url("https://example.com/"),
        "https://example.com/blog/"
    );
    assert_eq!(root["title"].as_str(), Some("站长的博客"));
    assert_eq!(root["locale"].as_str(), Some("zh-cn"));
    assert_eq!(root["defaultContentLanguage"].as_str(), Some("zh-cn"));
    assert_eq!(root["hasCJKLanguage"].as_bool(), Some(true));
    assert_eq!(root["theme"].as_str(), Some(DEFAULT_THEME_ID));
    assert_eq!(root["themesDir"].as_str(), Some(HUGO_THEMES_DIR));
    assert_eq!(root["permalinks"]["posts"].as_str(), Some(POSTS_PERMALINK));
    assert_eq!(root["taxonomies"]["category"].as_str(), Some("categories"));
    assert_eq!(root["taxonomies"]["tag"].as_str(), Some("tags"));
    assert_eq!(root["taxonomies"]["author"].as_str(), Some("authors"));
    assert_eq!(root["menus"]["main"][0]["name"].as_str(), Some("友链"));
    assert_eq!(root["menus"]["main"][0]["url"].as_str(), Some("/friends/"));

    let security = root["security"].as_table().unwrap();
    assert_eq!(security["enableInlineShortcodes"].as_bool(), Some(false));
    assert_eq!(security["exec"]["allow"][0].as_str(), Some("^sass$"));
    assert_eq!(
        security["exec"]["osEnv"][0].as_str(),
        Some("(?i)^(PATH|HOME|TMPDIR)$")
    );
    assert_eq!(security["funcs"]["getenv"][0].as_str(), Some("^HUGO_"));
    assert_eq!(security["http"]["urls"].as_str(), Some("none"));
    let allow = security["allowContent"].as_array().unwrap();
    assert_eq!(allow[0].as_str(), Some("^text/html$"));
    assert_eq!(allow[1].as_str(), Some("^text/markdown$"));

    let params = root["params"].as_table().unwrap();
    assert!(params.get("security").is_none());
    assert!(params.get("baseURL").is_none());
    assert_eq!(params["subtitle"].as_str(), Some("你好"));
    assert_eq!(params["mainSections"][0].as_str(), Some("notes"));
    assert_eq!(params["fromSite"].as_bool(), Some(true));
    assert_eq!(params["description"].as_str(), Some("说明"));

    let text = render_hugo_config(&site(), &BlogSettings::default(), "hugo-PaperMod");
    let value: toml::Table = toml::from_str(&text).unwrap();
    assert_eq!(value["theme"].as_str(), Some("hugo-PaperMod"));
    assert_eq!(value["params"]["mainSections"][0].as_str(), Some("posts"));
    assert_eq!(value["title"].as_str(), Some("站点标题"));
}

#[test]
fn postprocess_strips_scripts_and_injects_css() {
    let html = r#"<!DOCTYPE html><html><head><title>t</title></head><body>
<script>alert(1)</script>
<script type="application/ld+json">{"@type":"Blog"}</script>
<script type="application/json; charset=utf-8">{}</script>
<a href=" javascript:alert(1)" onclick="evil()">x</a>
<img src="JAVASCRIPT:alert(1)" onerror="bad">
<math display="block"></math>
</body></html>"#;
    let out = postprocess_html(html);
    let lower = out.to_ascii_lowercase();
    assert!(!lower.contains(">alert(1)<"), "{out}");
    assert!(out.contains("application/ld+json"), "{out}");
    assert!(out.contains("application/json"), "{out}");
    assert!(!lower.contains("onclick"), "{out}");
    assert!(!lower.contains("onerror"), "{out}");
    assert!(!lower.contains("javascript:"), "{out}");
    assert!(out.contains("href=\"#\""), "{out}");
    assert!(out.contains("<math"), "{out}");
    assert!(out.contains(NOTES_CSS_HREF), "{out}");
    let head_end = lower.find("</head>").unwrap();
    assert!(lower[..head_end].contains(NOTES_CSS_HREF));
    assert!(NOTES_CSS.contains(".note-columns"));
    assert!(NOTES_CSS.contains(".myriad-comments"));
    assert!(NOTES_CSS.contains(".myriad-friend-apply{height:"));
    assert!(NOTES_CSS.contains("iframe[data-embed-scheme=dark]{color-scheme:dark}"));
    assert!(BLOG_CSP.contains("script-src 'none'"));
    assert!(BLOG_CSP.contains("frame-ancestors 'self'"));

    assert!(is_script_asset("assets/app.js"));
    assert!(is_script_asset("/blog/theme.mjs"));
    assert!(is_script_asset("APP.JS"));
    assert!(is_script_asset("app.js?v=1"));
    assert!(is_script_asset("app.mjs#x"));
    assert!(!is_script_asset("app.css"));
    assert!(!is_script_asset("app.json"));
    assert!(!is_script_asset("notes.css"));
}

#[test]
fn theme_manifest_id_and_archive_paths() {
    let meta = inspect_theme_manifest(
        r#"
        name = "PaperMod"
        min_version = "0.146.0"
        description = "clean"
        license = "MIT"
        tags = ["blog", 1, "minimal"]
        "#,
    )
    .unwrap();
    assert_eq!(meta.name, "PaperMod");
    assert_eq!(meta.min_version.as_deref(), Some("0.146.0"));
    assert_eq!(meta.tags, ["blog", "minimal"]);
    assert!(matches!(
        inspect_theme_manifest("min_version = \"0.1.0\""),
        Err(ThemeManifestError::MissingName)
    ));
    assert!(matches!(
        inspect_theme_manifest("["),
        Err(ThemeManifestError::Invalid(_))
    ));

    assert_eq!(
        sanitize_theme_id(" myriad-default ").as_deref(),
        Some("myriad-default")
    );
    assert_eq!(
        sanitize_theme_id("hugo-PaperMod").as_deref(),
        Some("hugo-PaperMod")
    );
    assert_eq!(sanitize_theme_id("FixIt").as_deref(), Some("FixIt"));
    assert!(sanitize_theme_id("../x").is_none());
    assert!(sanitize_theme_id("a/b").is_none());
    assert!(sanitize_theme_id(".hidden").is_none());
    assert!(sanitize_theme_id("foo..bar").is_none());
    assert!(sanitize_theme_id("").is_none());

    assert_eq!(
        safe_archive_path("layouts/baseof.html").unwrap(),
        Path::new("layouts/baseof.html")
    );
    assert_eq!(
        safe_archive_path("./layouts/a.html").unwrap(),
        Path::new("layouts/a.html")
    );
    assert_eq!(safe_archive_path("foo\\bar").unwrap(), Path::new("foo/bar"));
    assert!(safe_archive_path("/etc/passwd").is_none());
    assert!(safe_archive_path("C:/Windows/a").is_none());
    assert!(safe_archive_path("C:foo").is_none());
    assert!(safe_archive_path("../x").is_none());
    assert!(safe_archive_path("foo/../../etc/passwd").is_none());
    assert!(safe_archive_path("foo\\..\\bar").is_none());
    assert!(safe_archive_path("\\\\server\\share").is_none());
    assert!(safe_archive_path("").is_none());
    assert!(safe_archive_path("foo/bar\0baz").is_none());

    let github = [
        "hugo-PaperMod-master/",
        "hugo-PaperMod-master/theme.toml",
        "hugo-PaperMod-master/layouts/baseof.html",
    ];
    assert_eq!(
        github_archive_root(github).as_deref(),
        Some("hugo-PaperMod-master")
    );
    let paths = safe_archive_entries(github).unwrap();
    assert!(paths.iter().any(|path| path == Path::new("theme.toml")));
    assert!(
        paths
            .iter()
            .any(|path| path == Path::new("layouts/baseof.html"))
    );
    assert_eq!(
        archive_entry_path(
            "hugo-PaperMod-master/layouts/baseof.html",
            Some("hugo-PaperMod-master")
        )
        .unwrap(),
        Path::new("layouts/baseof.html")
    );

    assert!(github_archive_root(["theme.toml", "layouts/a.html"]).is_none());
    let flat = safe_archive_entries(["theme.toml", "layouts/a.html"]).unwrap();
    assert_eq!(flat[0], Path::new("theme.toml"));
    assert!(safe_archive_entries(["ok/a.txt", "../etc/passwd"]).is_none());
    assert!(safe_archive_entries(["repo/../../etc/passwd"]).is_none());
}

#[test]
fn default_theme_files_match_current_hugo_layout() {
    let files = default_theme_files();
    let required = [
        "theme.toml",
        "layouts/baseof.html",
        "layouts/home.html",
        "layouts/list.html",
        "layouts/single.html",
        "layouts/taxonomy.html",
        "layouts/term.html",
        "layouts/404.html",
        "static/css/style.css",
    ];
    for path in required {
        let content = files
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, content)| *content)
            .unwrap_or_else(|| panic!("missing {path}"));
        assert!(!content.trim().is_empty(), "{path}");
        assert!(safe_archive_path(path).is_some(), "{path}");
        assert!(!content.to_ascii_lowercase().contains("<script"));
    }
    let css = files
        .iter()
        .find(|(name, _)| *name == "static/css/style.css")
        .unwrap()
        .1;
    assert!(css.contains("Songti SC") || css.contains("Noto Serif CJK"));
    let theme_toml = files
        .iter()
        .find(|(name, _)| *name == "theme.toml")
        .unwrap()
        .1;
    let meta = inspect_theme_manifest(theme_toml).unwrap();
    assert!(!meta.name.is_empty());
    assert!(meta.min_version.is_some());
    let base = files
        .iter()
        .find(|(name, _)| *name == "layouts/baseof.html")
        .unwrap()
        .1;
    assert!(base.contains("block \"main\""));
    let single = files
        .iter()
        .find(|(name, _)| *name == "layouts/single.html")
        .unwrap()
        .1;
    assert!(single.trim_start().starts_with("{{ define \"main\" }}"));
    let missing = files
        .iter()
        .find(|(name, _)| *name == "layouts/404.html")
        .unwrap()
        .1;
    assert!(missing.contains("<html"));
    assert!(missing.contains("找不到页面"));
}
