#!/usr/bin/env python3
"""Rewrite report mock triples and inject search/ICU catalog updates."""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

ZH_TO_KEY = {
    "追番和投稿不在同一条线上": "bili.diverge",
    "名单偏追番": "bili.leansAnime",
    "最近在看投稿": "bili.watchingUploads",
    "公开区几乎是空的": "bili.empty",
    "名单上还挂着": "bili.stillListed",
    "最近在看": "bili.watching",
    "类型占比": "bili.mix",
    "时长堆在少数作品上": "steam.hoursPile",
    "库比最近在玩的名单大": "steam.libraryOutgrows",
    "吃掉最多终身分钟": "steam.mostMinutes",
    "类型气味偏": "steam.genreLeans",
    "库": "steam.library",
    "最近名单": "steam.recent",
    "仓库影响力不等于日历密度": "github.starsNotCalendar",
    "最高": "github.top",
    "日历有": "github.calendarHas",
    "天有提交": "github.activeDays",
    "语言栈头是": "github.topLanguage",
    "公开区还没有能闻的片": "yt.noPublic",
    "video_count 为 0，频道资料在、片子不在": "yt.videoCountZero",
    "冷启动空壳": "yt.coldShell",
    "冷启动号": "yt.coldStart",
    "订阅和均播要分开看": "yt.subsSplit",
    "均播约": "yt.avgViews",
    "订阅": "yt.subs",
    "最近标题": "yt.latest",
    "最近上传": "yt.uploaded",
    "有片可闻": "yt.hasVideosVibe",
    "停更沉寂": "yt.dormant",
    "有片": "yt.hasVideos",
    "曲风比数量诚实": "netease.genreHonest",
    "名单上反复出现": "netease.keepsShowing",
    "地域偏": "netease.regionLeans",
    "曲风偏": "netease.genreLeans",
    "关注名单比发帖诚实": "x.followsHonest",
    "账上": "x.posts",
    "帖": "x.postUnit",
    "关注里有": "x.follows",
    "近帖写": "x.recentPost",
    "沉浸观察": "x.watching",
    "有帖可闻": "x.hasPosts",
    "沉浸观察者": "x.observer",
    "脉冲发帖": "x.pulse",
    "关注样本": "x.followTag",
    "潜水观察者": "discord.lurker",
    "社群主理人": "discord.host",
    "圈子老炮": "discord.regular",
    "身份看自建数，不看服多": "discord.identityOwned",
    "自建": "discord.owned",
    "管理": "discord.admin",
    "名单上有": "discord.listed",
    "绑定": "discord.linked",
    "自己的服": "discord.ownServer",
    "加入的服": "discord.joined",
    "全成就": "xbox.complete",
    "平均完成": "xbox.avg",
    "近作": "xbox.recent",
    "全成就猎人": "xbox.hunter",
    "广撒网玩家": "xbox.wideNet",
    "白金": "psn.platinum",
    "平均进度": "psn.avgProgress",
    "白金收藏家": "psn.collector",
    "随缘奖杯党": "psn.casual",
    "想看比进度诚实": "catalog.wishlistOutruns",
    "列表偏做完": "catalog.leansFinished",
    "进度压过想看": "catalog.doneOutruns",
    "高分有": "catalog.highScore",
    "在追": "catalog.watching",
}

SEARCH_KEYWORDS = {
    "en-US": {
        "platforms": [
            "data",
            "stats",
            "platforms",
            "sources",
            "token",
            "api",
            "visitor",
            "analytics",
            "cache",
            "refresh",
        ],
        "connectedPlatforms": [
            "connected",
            "platforms",
            "auto refresh",
            "interval",
            "sources",
        ],
        "visitor": [
            "visitor",
            "analytics",
            "PV",
            "UV",
            "pageview",
            "event",
            "referrer",
            "analytics_enabled",
            "opt-out",
            "optout",
            "privacy",
        ],
        "aiUsage": [
            "ai",
            "usage",
            "token",
            "model",
            "ledger",
            "ai-usage",
            "ai_usage",
        ],
        "thirdParty": [
            "third-party",
            "ga",
            "ga4",
            "google analytics",
            "gtag",
            "umami",
            "ga_measurement_id",
            "umami_website_id",
            "umami_script_url",
            "analytics script",
        ],
        "data": ["data", "cache", "filter", "smart filter", "refresh"],
        "platformItem": ["platform", "source", "token", "api"],
        "ai": [
            "ai",
            "gemini",
            "openai",
            "api",
            "model",
            "image",
            "agent",
            "schedule",
            "skills",
            "memory",
            "heartbeat",
        ],
        "tripo": [
            "tripo",
            "3d",
            "glb",
            "gltf",
            "low poly",
            "rig",
            "animation",
            "bones",
        ],
        "basic": [
            "basic",
            "ui",
            "site",
            "theme",
            "background",
            "url",
            "domain",
            "base_url",
            "cors",
            "origin",
            "pwa",
            "service worker",
            "install",
            "offline",
            "pwa_enabled",
        ],
        "oauth": ["oauth", "github", "login", "auth"],
        "music": ["music", "playlist", "player", "netease", "qq music"],
        "network": [
            "proxy",
            "network",
            "gemini",
            "github",
            "api",
            "mirror",
            "socks",
        ],
        "notifications": [
            "notification",
            "toast",
            "browser",
            "arael",
            "brew",
            "tapp",
            "mcp",
            "aro",
        ],
        "advanced": [
            "advanced",
            "danger",
            "reset",
            "proxy",
            "import",
            "export",
            "health",
            "database",
            "storage",
            "task",
            "mcp",
            "model context protocol",
            "tool server",
        ],
        "mcp": [
            "mcp",
            "MCP",
            "model context protocol",
            "tool server",
            "stdio",
            "reload",
            "hot reload",
            "arael",
            "agent",
        ],
        "about": [
            "about",
            "version",
            "logo",
            "myriad",
            "updater",
            "update",
            "upgrade",
            "rollback",
            "snapshot",
        ],
        "permissions": [
            "permission",
            "elevated",
            "quota",
            "ai",
            "guest",
        ],
        "users": [
            "user",
            "users",
            "admin",
            "oauth",
            "account",
            "online",
            "register",
            "identity",
        ],
        "federation": [
            "federation",
            "trust",
            "allowlist",
            "block",
            "filter",
            "mfp",
            "aro",
        ],
        "modules": [
            "module",
            "library",
            "source",
            "platform",
            "category",
            "visibility",
            "hitokoto",
            "quote",
        ],
    },
    "zh-CN": {
        "platforms": [
            "数据及统计",
            "数据页",
            "平台",
            "接入平台",
            "数据源",
            "token",
            "api",
            "访客",
            "访问",
            "统计",
            "analytics",
            "visitor",
            "数据管理",
            "缓存",
            "刷新",
        ],
        "connectedPlatforms": [
            "接入平台",
            "数据平台",
            "自动刷新",
            "刷新频率",
            "平台",
            "数据源",
            "connected",
            "platforms",
        ],
        "visitor": [
            "访客",
            "统计",
            "PV",
            "UV",
            "visitor",
            "analytics",
            "页面",
            "访问分析",
            "pageview",
            "事件",
            "来源",
            "referrer",
            "开关",
            "启用",
            "analytics_enabled",
            "opt-out",
            "optout",
            "退出",
            "隐私",
        ],
        "aiUsage": [
            "ai",
            "usage",
            "token",
            "用量",
            "使用量",
            "模型",
            "model",
            "调用",
            "ledger",
            "ai-usage",
            "ai_usage",
        ],
        "thirdParty": [
            "第三方",
            "third-party",
            "第三方统计",
            "ga",
            "ga4",
            "google analytics",
            "gtag",
            "umami",
            "ga_measurement_id",
            "umami_website_id",
            "umami_script_url",
            "外部统计",
            "analytics script",
        ],
        "data": ["数据管理", "data", "缓存", "cache", "过滤", "智能过滤", "刷新"],
        "platformItem": ["平台", "数据源", "token", "api"],
        "ai": [
            "ai",
            "gemini",
            "openai",
            "api",
            "模型",
            "智能",
            "图片",
            "生成",
            "image",
            "agent",
            "定时",
            "定时任务",
            "技能",
            "记忆",
            "heartbeat",
            "skills",
            "memory",
        ],
        "tripo": [
            "tripo",
            "3d",
            "glb",
            "gltf",
            "low poly",
            "rig",
            "animation",
            "低模",
            "骨骼",
            "动作",
            "模型",
        ],
        "basic": [
            "basic",
            "基础",
            "ui",
            "站点",
            "主题",
            "背景",
            "样式",
            "theme",
            "url",
            "domain",
            "域名",
            "更换域名",
            "base_url",
            "cors",
            "origin",
            "pwa",
            "service worker",
            "安装",
            "install",
            "离线",
            "offline",
            "pwa_enabled",
        ],
        "oauth": ["oauth", "github", "登录", "auth", "认证"],
        "music": ["音乐", "music", "歌单", "播放器", "网易云", "qq音乐"],
        "network": [
            "proxy",
            "代理",
            "网络",
            "gemini",
            "github",
            "api",
            "镜像",
            "mirror",
            "socks",
            "network",
        ],
        "notifications": [
            "notification",
            "通知",
            "提醒",
            "toast",
            "browser",
            "arael",
            "brew",
            "tapp",
            "mcp",
            "aro",
        ],
        "advanced": [
            "advanced",
            "高级",
            "danger",
            "reset",
            "重置",
            "危险",
            "proxy",
            "代理",
            "导入",
            "导出",
            "运行",
            "诊断",
            "health",
            "database",
            "storage",
            "task",
            "mcp",
            "model context protocol",
            "工具服务器",
            "tool server",
        ],
        "mcp": [
            "mcp",
            "MCP",
            "model context protocol",
            "工具服务器",
            "tool server",
            "stdio",
            "reload",
            "热重载",
            "arael",
            "agent",
        ],
        "about": [
            "about",
            "关于",
            "版本",
            "version",
            "logo",
            "myriad",
            "updater",
            "更新",
            "update",
            "upgrade",
            "升级",
            "回滚",
            "rollback",
            "snapshot",
            "快照",
        ],
        "permissions": [
            "权限",
            "permission",
            "elevated",
            "下放",
            "配额",
            "quota",
            "ai",
            "游客",
            "guest",
        ],
        "users": [
            "用户",
            "user",
            "users",
            "管理员",
            "admin",
            "oauth",
            "账户",
            "account",
            "在线",
            "online",
            "注册",
            "register",
            "identity",
            "绑定",
        ],
        "federation": [
            "federation",
            "联邦",
            "trust",
            "信任",
            "allowlist",
            "白名单",
            "block",
            "封禁",
            "filter",
            "过滤",
            "mfp",
            "aro",
        ],
        "modules": [
            "模块",
            "module",
            "资料库",
            "library",
            "来源",
            "source",
            "平台",
            "分类",
            "可见性",
            "visibility",
            "登录用户",
            "管理员",
            "一言",
            "hitokoto",
            "quote",
        ],
    },
    "ja-JP": {
        "platforms": [
            "データ",
            "統計",
            "プラットフォーム",
            "ソース",
            "token",
            "api",
            "ビジター",
            "analytics",
            "キャッシュ",
            "更新",
        ],
        "connectedPlatforms": [
            "接続",
            "プラットフォーム",
            "自動更新",
            "間隔",
            "ソース",
            "connected",
        ],
        "visitor": [
            "ビジター",
            "統計",
            "PV",
            "UV",
            "visitor",
            "analytics",
            "ページ",
            "pageview",
            "イベント",
            "referrer",
            "analytics_enabled",
            "opt-out",
            "プライバシー",
        ],
        "aiUsage": [
            "ai",
            "usage",
            "token",
            "使用量",
            "モデル",
            "model",
            "ledger",
            "ai-usage",
        ],
        "thirdParty": [
            "第三者",
            "third-party",
            "ga",
            "ga4",
            "google analytics",
            "gtag",
            "umami",
            "外部統計",
        ],
        "data": ["データ管理", "data", "キャッシュ", "cache", "フィルタ", "更新"],
        "platformItem": ["プラットフォーム", "ソース", "token", "api"],
        "ai": [
            "ai",
            "gemini",
            "openai",
            "api",
            "モデル",
            "画像",
            "agent",
            "スキル",
            "記憶",
            "heartbeat",
        ],
        "tripo": [
            "tripo",
            "3d",
            "glb",
            "gltf",
            "low poly",
            "rig",
            "animation",
            "ボーン",
        ],
        "basic": [
            "basic",
            "基本",
            "ui",
            "サイト",
            "テーマ",
            "背景",
            "url",
            "domain",
            "ドメイン",
            "pwa",
            "install",
            "オフライン",
        ],
        "oauth": ["oauth", "github", "ログイン", "auth"],
        "music": ["音楽", "music", "プレイリスト", "プレーヤー", "netease"],
        "network": ["proxy", "プロキシ", "ネットワーク", "mirror", "socks"],
        "notifications": [
            "notification",
            "通知",
            "toast",
            "browser",
            "arael",
            "brew",
            "tapp",
            "mcp",
        ],
        "advanced": [
            "advanced",
            "上級",
            "reset",
            "リセット",
            "import",
            "export",
            "health",
            "database",
            "mcp",
        ],
        "mcp": [
            "mcp",
            "MCP",
            "model context protocol",
            "ツールサーバー",
            "stdio",
            "reload",
            "arael",
        ],
        "about": [
            "about",
            "バージョン",
            "version",
            "myriad",
            "updater",
            "更新",
            "rollback",
            "snapshot",
        ],
        "permissions": ["権限", "permission", "quota", "ai", "ゲスト"],
        "users": [
            "ユーザー",
            "user",
            "admin",
            "oauth",
            "アカウント",
            "online",
            "登録",
        ],
        "federation": [
            "federation",
            "連合",
            "trust",
            "allowlist",
            "block",
            "mfp",
            "aro",
        ],
        "modules": [
            "モジュール",
            "library",
            "ソース",
            "カテゴリ",
            "visibility",
            "hitokoto",
        ],
    },
}


def rewrite_mock() -> None:
    path = ROOT / "backend/src/api/reports/mock.rs"
    text = path.read_text(encoding="utf-8")
    pattern = re.compile(
        r't\(\s*locale,\s*"([^"]+)",\s*"(?:[^"\\]|\\.)*?",\s*"(?:[^"\\]|\\.)*?"\s*\)',
        re.S,
    )
    missing = []

    def repl(match: re.Match[str]) -> str:
        zh = match.group(1)
        key = ZH_TO_KEY.get(zh)
        if not key:
            missing.append(zh)
            return match.group(0)
        return f't(locale, "{key}")'

    new, count = pattern.subn(repl, text)
    if missing:
        raise SystemExit(f"unmapped mock strings: {missing}")
    new = new.replace(
        "use super::locale::pick;\nuse crate::services::smart_filter::{ContentAnalysis, SmartFilteredData};\n\n"
        "fn t<'a>(locale: &str, zh: &'a str, ja: &'a str, en: &'a str) -> &'a str {\n"
        "    pick(locale, zh, ja, en)\n"
        "}\n",
        "use crate::services::smart_filter::{ContentAnalysis, SmartFilteredData};\n\n"
        "fn t(locale: &str, key: &str) -> String {\n"
        "    crate::i18n::reports(locale, key)\n"
        "}\n",
    )
    new = new.replace(
        ".map(|item| format!(\"{} {}\", item.count, category_label(&item.category)))",
        ".map(|item| format!(\"{} {}\", item.count, category_label(locale, &item.category)))",
    )
    new = new.replace(
        "fn category_label(\n"
        "    category: &crate::services::content_databases::anime_database::ContentCategory,\n"
        ") -> &'static str {\n"
        "    use crate::services::content_databases::anime_database::ContentCategory;\n"
        "    match category {\n"
        "        ContentCategory::Anime => \"番\",\n"
        "        ContentCategory::TvSeries => \"剧\",\n"
        "        ContentCategory::Movie => \"电影\",\n"
        "    }\n"
        "}\n",
        "fn category_label(\n"
        "    locale: &str,\n"
        "    category: &crate::services::content_databases::anime_database::ContentCategory,\n"
        ") -> String {\n"
        "    use crate::services::content_databases::anime_database::ContentCategory;\n"
        "    match category {\n"
        "        ContentCategory::Anime => t(locale, \"bili.catAnime\"),\n"
        "        ContentCategory::TvSeries => t(locale, \"bili.catTv\"),\n"
        "        ContentCategory::Movie => t(locale, \"bili.catMovie\"),\n"
        "    }\n"
        "}\n",
    )
    leftover = pattern.findall(new)
    if leftover:
        raise SystemExit(f"leftover mock triples: {leftover}")
    path.write_text(new, encoding="utf-8")
    print(f"rewrote {count} mock t() calls")


def convert_keywords(src: dict, converter) -> dict:
    if not converter:
        return src
    return {key: [converter.convert(item) for item in values] for key, values in src.items()}


def inject_search_keywords() -> None:
    try:
        from opencc import OpenCC

        converter = OpenCC("s2twp")
    except Exception:
        converter = None
    packs = {
        "en-US": SEARCH_KEYWORDS["en-US"],
        "zh-CN": SEARCH_KEYWORDS["zh-CN"],
        "ja-JP": SEARCH_KEYWORDS["ja-JP"],
        "zh-TW": convert_keywords(SEARCH_KEYWORDS["zh-CN"], converter),
    }
    for locale, keywords in packs.items():
        path = ROOT / f"frontend/src/i18n/config.{locale}.json"
        data = json.loads(path.read_text(encoding="utf-8"))
        data["searchKeywords"] = keywords
        path.write_text(
            json.dumps(data, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        print(f"injected searchKeywords into {path.name}")


def patch_icu() -> None:
    replacements = {
        ROOT / "frontend/src/i18n/errors.en-US.json": {
            "noticeBrewNewItems": "{name} · {n, plural, one {# new item} other {# new items}}",
            "noticeBrewNewItemsBody": "{n, plural, one {# new item found} other {# new items found}}",
        },
        ROOT / "frontend/src/i18n/brew.en-US.json": {
            "articlesCount": "{count, plural, one {# article} other {# articles}}",
            "totalArticles": "{count, plural, one {# article} other {# articles}}",
            "starredCount": "{count, plural, one {# starred} other {# starred}}",
            "selectedCount": "{count, plural, one {# selected} other {# selected}}",
            "loadedAllArticles": "— All {count, plural, one {# article} other {# articles}} loaded —",
        },
        ROOT / "frontend/src/i18n/config.en-US.json": {
            ("analytics", "daysN"): "{n, plural, one {# day} other {# days}}",
            ("analytics", "rangeDaysSelected"): "{n, plural, one {# day} other {# days}}",
        },
        ROOT / "frontend/src/i18n/tapp.en-US.json": {
            "storeUpdatedDaysAgo": "Updated {n, plural, one {# day} other {# days}} ago",
        },
        ROOT / "frontend/src/i18n/en-US.json": {
            ("analytics", "daysN"): "{n, plural, one {# day} other {# days}}",
            ("analytics", "chartAria"): "Views and unique visitors over the last {n, plural, one {# day} other {# days}}",
        },
    }
    extra = {
        ROOT / "frontend/src/i18n/config.en-US.json": (
            "privateTappInstallPresetNShort",
            "{n, plural, one {# day} other {# days}}",
        )
    }
    for path, mapping in replacements.items():
        data = json.loads(path.read_text(encoding="utf-8"))
        for key, value in mapping.items():
            if isinstance(key, tuple):
                target = data
                for part in key[:-1]:
                    target = target[part]
                target[key[-1]] = value
            else:
                data[key] = value
        path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"patched ICU in {path.name}")
    for path, (key, value) in extra.items():
        data = json.loads(path.read_text(encoding="utf-8"))
        if key in data:
            data[key] = value
            path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            print(f"patched {key} in {path.name}")


def main() -> None:
    rewrite_mock()
    inject_search_keywords()
    patch_icu()


if __name__ == "__main__":
    main()
