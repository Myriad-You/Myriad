#!/usr/bin/env python3
"""Generate backend i18n catalogs and Traditional Chinese frontend packs."""

from __future__ import annotations

import json
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FRONTEND_I18N = ROOT / "frontend/src/i18n"
GUIDES = ROOT / "frontend/src/components/settings/guides"
BACKEND_I18N = ROOT / "backend/i18n"

REPORTS = {
    "bili.diverge": {
        "zh-CN": "追番和投稿不在同一条线上",
        "zh-TW": "追番和投稿不在同一條線上",
        "ja-JP": "追番と投稿は別線",
        "en-US": "Watching and uploads diverge",
    },
    "bili.leansAnime": {
        "zh-CN": "名单偏追番",
        "zh-TW": "名單偏追番",
        "ja-JP": "リストは追番寄り",
        "en-US": "List leans anime",
    },
    "bili.watchingUploads": {
        "zh-CN": "最近在看投稿",
        "zh-TW": "最近在看投稿",
        "ja-JP": "最近は投稿を見ている",
        "en-US": "Watching uploads lately",
    },
    "bili.empty": {
        "zh-CN": "公开区几乎是空的",
        "zh-TW": "公開區幾乎是空的",
        "ja-JP": "公開区はほぼ空",
        "en-US": "Public shelf is almost empty",
    },
    "bili.stillListed": {
        "zh-CN": "名单上还挂着",
        "zh-TW": "名單上還掛著",
        "ja-JP": "リストに残る",
        "en-US": "Still listed",
    },
    "bili.watching": {
        "zh-CN": "最近在看",
        "zh-TW": "最近在看",
        "ja-JP": "最近見ている",
        "en-US": "Watching",
    },
    "bili.mix": {
        "zh-CN": "类型占比",
        "zh-TW": "類型占比",
        "ja-JP": "ジャンル比",
        "en-US": "Mix",
    },
    "bili.catAnime": {
        "zh-CN": "番",
        "zh-TW": "番",
        "ja-JP": "アニメ",
        "en-US": "anime",
    },
    "bili.catTv": {
        "zh-CN": "剧",
        "zh-TW": "劇",
        "ja-JP": "ドラマ",
        "en-US": "series",
    },
    "bili.catMovie": {
        "zh-CN": "电影",
        "zh-TW": "電影",
        "ja-JP": "映画",
        "en-US": "film",
    },
    "steam.hoursPile": {
        "zh-CN": "时长堆在少数作品上",
        "zh-TW": "時長堆在少數作品上",
        "ja-JP": "時間は少数作に偏る",
        "en-US": "Hours pile on a few titles",
    },
    "steam.libraryOutgrows": {
        "zh-CN": "库比最近在玩的名单大",
        "zh-TW": "庫比最近在玩的名單大",
        "ja-JP": "ライブラリは最近の名簿より大きい",
        "en-US": "Library outgrows the recent list",
    },
    "steam.mostMinutes": {
        "zh-CN": "吃掉最多终身分钟",
        "zh-TW": "吃掉最多終身分鐘",
        "ja-JP": "が生涯分を最も食う",
        "en-US": " takes the most lifetime minutes",
    },
    "steam.genreLeans": {
        "zh-CN": "类型气味偏",
        "zh-TW": "類型氣味偏",
        "ja-JP": "ジャンルは",
        "en-US": "Genre leans ",
    },
    "steam.library": {
        "zh-CN": "库",
        "zh-TW": "庫",
        "ja-JP": "庫",
        "en-US": "Library",
    },
    "steam.recent": {
        "zh-CN": "最近名单",
        "zh-TW": "最近名單",
        "ja-JP": "最近の名簿",
        "en-US": "recent",
    },
    "github.starsNotCalendar": {
        "zh-CN": "仓库影响力不等于日历密度",
        "zh-TW": "倉庫影響力不等於日曆密度",
        "ja-JP": "星とカレンダー密度は別物",
        "en-US": "Stars are not calendar density",
    },
    "github.top": {
        "zh-CN": "最高",
        "zh-TW": "最高",
        "ja-JP": "最大",
        "en-US": "top",
    },
    "github.calendarHas": {
        "zh-CN": "日历有",
        "zh-TW": "日曆有",
        "ja-JP": "カレンダー",
        "en-US": "Calendar has",
    },
    "github.activeDays": {
        "zh-CN": "天有提交",
        "zh-TW": "天有提交",
        "ja-JP": "日コミットあり",
        "en-US": "active days",
    },
    "github.topLanguage": {
        "zh-CN": "语言栈头是",
        "zh-TW": "語言棧頭是",
        "ja-JP": "言語の頭は",
        "en-US": "Top language",
    },
    "yt.noPublic": {
        "zh-CN": "公开区还没有能闻的片",
        "zh-TW": "公開區還沒有能聞的片",
        "ja-JP": "公開区に嗅げる動画がない",
        "en-US": "No public videos to read",
    },
    "yt.videoCountZero": {
        "zh-CN": "video_count 为 0，频道资料在、片子不在",
        "zh-TW": "video_count 為 0，頻道資料在、片子不在",
        "ja-JP": "video_count は 0、資料だけある",
        "en-US": "video_count is 0; channel exists, videos do not",
    },
    "yt.coldShell": {
        "zh-CN": "冷启动空壳",
        "zh-TW": "冷啟動空殼",
        "ja-JP": "コールドスタート",
        "en-US": "Cold start shell",
    },
    "yt.coldStart": {
        "zh-CN": "冷启动号",
        "zh-TW": "冷啟動號",
        "ja-JP": "コールドスタート",
        "en-US": "Cold start",
    },
    "yt.subsSplit": {
        "zh-CN": "订阅和均播要分开看",
        "zh-TW": "訂閱和均播要分開看",
        "ja-JP": "登録と平均再生は別物",
        "en-US": "Subs and average views split",
    },
    "yt.avgViews": {
        "zh-CN": "均播约",
        "zh-TW": "均播約",
        "ja-JP": "平均再生",
        "en-US": "Avg views",
    },
    "yt.subs": {
        "zh-CN": "订阅",
        "zh-TW": "訂閱",
        "ja-JP": "登録",
        "en-US": "subs",
    },
    "yt.latest": {
        "zh-CN": "最近标题",
        "zh-TW": "最近標題",
        "ja-JP": "最近のタイトル",
        "en-US": "Latest",
    },
    "yt.uploaded": {
        "zh-CN": "最近上传",
        "zh-TW": "最近上傳",
        "ja-JP": "最近の投稿",
        "en-US": "Uploaded",
    },
    "yt.hasVideosVibe": {
        "zh-CN": "有片可闻",
        "zh-TW": "有片可聞",
        "ja-JP": "動画あり",
        "en-US": "Has videos",
    },
    "yt.dormant": {
        "zh-CN": "停更沉寂",
        "zh-TW": "停更沉寂",
        "ja-JP": "更新停止",
        "en-US": "Dormant",
    },
    "yt.hasVideos": {
        "zh-CN": "有片",
        "zh-TW": "有片",
        "ja-JP": "動画あり",
        "en-US": "Has videos",
    },
    "netease.genreHonest": {
        "zh-CN": "曲风比数量诚实",
        "zh-TW": "曲風比數量誠實",
        "ja-JP": "曲調は本数より正直",
        "en-US": "Genre is more honest than count",
    },
    "netease.keepsShowing": {
        "zh-CN": "名单上反复出现",
        "zh-TW": "名單上反覆出現",
        "ja-JP": "名簿に繰り返す",
        "en-US": "Keeps showing",
    },
    "netease.regionLeans": {
        "zh-CN": "地域偏",
        "zh-TW": "地域偏",
        "ja-JP": "地域は",
        "en-US": "Region leans ",
    },
    "netease.genreLeans": {
        "zh-CN": "曲风偏",
        "zh-TW": "曲風偏",
        "ja-JP": "ジャンルは",
        "en-US": "Genre leans ",
    },
    "x.followsHonest": {
        "zh-CN": "关注名单比发帖诚实",
        "zh-TW": "關注名單比發帖誠實",
        "ja-JP": "フォローは投稿より正直",
        "en-US": "Follows are more honest than posts",
    },
    "x.posts": {
        "zh-CN": "账上",
        "zh-TW": "帳上",
        "ja-JP": "投稿",
        "en-US": "Posts",
    },
    "x.postUnit": {
        "zh-CN": "帖",
        "zh-TW": "帖",
        "ja-JP": "",
        "en-US": "",
    },
    "x.follows": {
        "zh-CN": "关注里有",
        "zh-TW": "關注裡有",
        "ja-JP": "フォローに",
        "en-US": "Follows",
    },
    "x.recentPost": {
        "zh-CN": "近帖写",
        "zh-TW": "近帖寫",
        "ja-JP": "近投稿は",
        "en-US": "Recent post",
    },
    "x.watching": {
        "zh-CN": "沉浸观察",
        "zh-TW": "沉浸觀察",
        "ja-JP": "観察に没入",
        "en-US": "Watching",
    },
    "x.hasPosts": {
        "zh-CN": "有帖可闻",
        "zh-TW": "有帖可聞",
        "ja-JP": "投稿あり",
        "en-US": "Has posts",
    },
    "x.observer": {
        "zh-CN": "沉浸观察者",
        "zh-TW": "沉浸觀察者",
        "ja-JP": "没入観察者",
        "en-US": "Observer",
    },
    "x.pulse": {
        "zh-CN": "脉冲发帖",
        "zh-TW": "脈衝發帖",
        "ja-JP": "パルス投稿",
        "en-US": "Pulse poster",
    },
    "x.followTag": {
        "zh-CN": "关注样本",
        "zh-TW": "關注樣本",
        "ja-JP": "フォロー標本",
        "en-US": "Follow",
    },
    "discord.lurker": {
        "zh-CN": "潜水观察者",
        "zh-TW": "潛水觀察者",
        "ja-JP": "潜水観察者",
        "en-US": "Lurker",
    },
    "discord.host": {
        "zh-CN": "社群主理人",
        "zh-TW": "社群主理人",
        "ja-JP": "コミュニティ主",
        "en-US": "Community host",
    },
    "discord.regular": {
        "zh-CN": "圈子老炮",
        "zh-TW": "圈子老砲",
        "ja-JP": "古参",
        "en-US": "Circle regular",
    },
    "discord.identityOwned": {
        "zh-CN": "身份看自建数，不看服多",
        "zh-TW": "身分看自建數，不看服多",
        "ja-JP": "身分は自作数で見る",
        "en-US": "Identity is owned servers, not count",
    },
    "discord.owned": {
        "zh-CN": "自建",
        "zh-TW": "自建",
        "ja-JP": "自作",
        "en-US": "Owned",
    },
    "discord.admin": {
        "zh-CN": "管理",
        "zh-TW": "管理",
        "ja-JP": "管理",
        "en-US": "admin",
    },
    "discord.listed": {
        "zh-CN": "名单上有",
        "zh-TW": "名單上有",
        "ja-JP": "名簿に",
        "en-US": "Listed",
    },
    "discord.linked": {
        "zh-CN": "绑定",
        "zh-TW": "綁定",
        "ja-JP": "連携",
        "en-US": "Linked",
    },
    "discord.ownServer": {
        "zh-CN": "自己的服",
        "zh-TW": "自己的服",
        "ja-JP": "自分の鯖",
        "en-US": "Own server",
    },
    "discord.joined": {
        "zh-CN": "加入的服",
        "zh-TW": "加入的服",
        "ja-JP": "参加した鯖",
        "en-US": "Joined",
    },
    "discord.sizeHuge": {
        "zh-CN": "万人广场",
        "zh-TW": "萬人廣場",
        "ja-JP": "万人広場",
        "en-US": "100k+",
    },
    "discord.sizeLarge": {
        "zh-CN": "万人级",
        "zh-TW": "萬人級",
        "ja-JP": "万人級",
        "en-US": "10k+",
    },
    "discord.sizeMid": {
        "zh-CN": "千人圈",
        "zh-TW": "千人圈",
        "ja-JP": "千人圏",
        "en-US": "1k+",
    },
    "discord.sizeSmall": {
        "zh-CN": "小圈子",
        "zh-TW": "小圈子",
        "ja-JP": "小規模",
        "en-US": "small",
    },
    "discord.takeOwner": {
        "zh-CN": "自建·",
        "zh-TW": "自建·",
        "ja-JP": "自作·",
        "en-US": "Owner · ",
    },
    "discord.takeAdmin": {
        "zh-CN": "掌舵·",
        "zh-TW": "掌舵·",
        "ja-JP": "運営·",
        "en-US": "Admin · ",
    },
    "discord.takeMod": {
        "zh-CN": "协管·",
        "zh-TW": "協管·",
        "ja-JP": "モデ·",
        "en-US": "Mod · ",
    },
    "discord.takeMember": {
        "zh-CN": "常驻·",
        "zh-TW": "常駐·",
        "ja-JP": "常駐·",
        "en-US": "Member · ",
    },
    "discord.takeOwnServer": {
        "zh-CN": "自建领地",
        "zh-TW": "自建領地",
        "ja-JP": "自作サーバー",
        "en-US": "Own server",
    },
    "discord.takeAdminSeat": {
        "zh-CN": "管理席位",
        "zh-TW": "管理席位",
        "ja-JP": "管理者",
        "en-US": "Admin seat",
    },
    "discord.takeModSeat": {
        "zh-CN": "协管席位",
        "zh-TW": "協管席位",
        "ja-JP": "モデ席",
        "en-US": "Mod seat",
    },
    "discord.takePartnered": {
        "zh-CN": "官方合作服",
        "zh-TW": "官方合作服",
        "ja-JP": "公式提携",
        "en-US": "Partnered",
    },
    "discord.takeVerified": {
        "zh-CN": "认证大服",
        "zh-TW": "認證大服",
        "ja-JP": "認証サーバー",
        "en-US": "Verified",
    },
    "discord.takeCommunity": {
        "zh-CN": "社区服常驻",
        "zh-TW": "社區服常駐",
        "ja-JP": "コミュニティ常駐",
        "en-US": "Community stay",
    },
    "discord.takeMemberPlain": {
        "zh-CN": "社区成员",
        "zh-TW": "社區成員",
        "ja-JP": "メンバー",
        "en-US": "Member",
    },
    "xbox.greens": {
        "zh-CN": "认绿光不认时长",
        "zh-TW": "認綠光不認時長",
        "ja-JP": "緑は見る、時間は見ない",
        "en-US": "Greens, not hours",
    },
    "xbox.complete": {
        "zh-CN": "全成就",
        "zh-TW": "全成就",
        "ja-JP": "コンプ",
        "en-US": "Complete",
    },
    "xbox.avg": {
        "zh-CN": "平均完成",
        "zh-TW": "平均完成",
        "ja-JP": "平均達成",
        "en-US": "avg",
    },
    "xbox.recent": {
        "zh-CN": "近作",
        "zh-TW": "近作",
        "ja-JP": "近作",
        "en-US": "Recent",
    },
    "xbox.hunter": {
        "zh-CN": "全成就猎人",
        "zh-TW": "全成就獵人",
        "ja-JP": "実績コンプ勢",
        "en-US": "Completion hunter",
    },
    "xbox.wideNet": {
        "zh-CN": "广撒网玩家",
        "zh-TW": "廣撒網玩家",
        "ja-JP": "広く浅く",
        "en-US": "Wide net",
    },
    "xbox.deepCompleter": {
        "zh-CN": "深度攻略型",
        "zh-TW": "深度攻略型",
        "ja-JP": "攻略勢",
        "en-US": "Deep completer",
    },
    "xbox.gsCollector": {
        "zh-CN": "GS收藏家",
        "zh-TW": "GS收藏家",
        "ja-JP": "GSコレクター",
        "en-US": "GS collector",
    },
    "psn.cabinet": {
        "zh-CN": "认奖杯柜不认时长",
        "zh-TW": "認獎盃櫃不認時長",
        "ja-JP": "トロフィー棚は見る、時間は見ない",
        "en-US": "Cabinet, not hours",
    },
    "psn.platinum": {
        "zh-CN": "白金",
        "zh-TW": "白金",
        "ja-JP": "プラチナ",
        "en-US": "Platinum",
    },
    "psn.avgProgress": {
        "zh-CN": "平均进度",
        "zh-TW": "平均進度",
        "ja-JP": "平均進捗",
        "en-US": "Avg progress",
    },
    "psn.recent": {
        "zh-CN": "近作",
        "zh-TW": "近作",
        "ja-JP": "近作",
        "en-US": "Recent",
    },
    "psn.collector": {
        "zh-CN": "白金收藏家",
        "zh-TW": "白金收藏家",
        "ja-JP": "プラチナ収集家",
        "en-US": "Platinum collector",
    },
    "psn.casual": {
        "zh-CN": "随缘奖杯党",
        "zh-TW": "隨緣獎盃黨",
        "ja-JP": "気まま勢",
        "en-US": "Casual trophies",
    },
    "psn.storyCompleter": {
        "zh-CN": "单机通关派",
        "zh-TW": "單機通關派",
        "ja-JP": "単機クリア派",
        "en-US": "Story completer",
    },
    "psn.trophyHunter": {
        "zh-CN": "深度奖杯党",
        "zh-TW": "深度獎盃黨",
        "ja-JP": "トロフィー勢",
        "en-US": "Trophy hunter",
    },
    "catalog.wishlistOutruns": {
        "zh-CN": "想看比进度诚实",
        "zh-TW": "想看比進度誠實",
        "ja-JP": "見たいは進捗より正直",
        "en-US": "Wishlist outruns done",
    },
    "catalog.leansFinished": {
        "zh-CN": "列表偏做完",
        "zh-TW": "列表偏做完",
        "ja-JP": "リストは完了寄り",
        "en-US": "List leans finished",
    },
    "catalog.doneOutruns": {
        "zh-CN": "进度压过想看",
        "zh-TW": "進度壓過想看",
        "ja-JP": "進捗が見たいを上回る",
        "en-US": "Done outruns wishlist",
    },
    "catalog.highScore": {
        "zh-CN": "高分有",
        "zh-TW": "高分有",
        "ja-JP": "高得点に",
        "en-US": "High score",
    },
    "catalog.watching": {
        "zh-CN": "在追",
        "zh-TW": "在追",
        "ja-JP": "視聴中",
        "en-US": "Watching",
    },
    "missingPlatformData": {
        "zh-CN": "该平台还没有可用数据。请先成功抓取后再生成报告。",
        "zh-TW": "該平台還沒有可用資料。請先成功抓取後再產生報告。",
        "ja-JP": "このプラットフォームのデータがまだありません。先に取得してからレポートを生成してください。",
        "en-US": "This platform has no usable data yet. Fetch it first, then generate the report.",
    },
    "generateNone": {
        "zh-CN": "未能生成报告。请确保已获取平台数据。",
        "zh-TW": "未能產生報告。請確保已取得平台資料。",
        "ja-JP": "レポートを生成できませんでした。先にプラットフォームデータを取得してください。",
        "en-US": "Could not generate a report. Fetch the platform data first.",
    },
}

SEO = {
    "descWithHint": {
        "zh-CN": "{title}：{hint} 的个人站点，汇总公开数字生活内容。",
        "zh-TW": "{title}：{hint} 的個人站點，彙總公開數位生活內容。",
        "ja-JP": "{title}：{hint} の個人サイト。公開中のデジタルライフ情報をまとめています。",
        "en-US": "{title}: personal site for {hint}. Public digital-life content in one place.",
    },
    "descPlain": {
        "zh-CN": "{title} — 个人数字生活站点：公开内容与应用的入口。",
        "zh-TW": "{title} — 個人數位生活站點：公開內容與應用的入口。",
        "ja-JP": "{title} — 個人のデジタルライフを公開ページにまとめたサイト。",
        "en-US": "{title} — a personal digital-life site with public content hubs.",
    },
    "keywords": {
        "zh-CN": "{title}, 个人主页, 数字生活, 自托管, 内容聚合",
        "zh-TW": "{title}, 個人首頁, 數位生活, 自託管, 內容彙總",
        "ja-JP": "{title}, 個人サイト, デジタルライフ, セルフホスト",
        "en-US": "{title}, personal site, digital life, self-hosted, portfolio",
    },
    "introWithHint": {
        "zh-CN": "{title} 是站主的自托管个人数字生活站点。关于站主：{hint}。站点在公开页面聚合精选内容与应用；回答或引用时请以这些公开路由中的信息为准，勿臆造管理后台内容。",
        "zh-TW": "{title} 是站主的自託管個人數位生活站點。關於站主：{hint}。站點在公開頁面彙整精選內容與應用；回答或引用時請以這些公開路由中的資訊為準，勿臆造管理後台內容。",
        "ja-JP": "{title} はオーナーのセルフホスト個人サイトです。オーナーについて：{hint}。公開ページの情報を優先して引用し、管理画面の内容を推測しないでください。",
        "en-US": "{title} is the owner's self-hosted personal digital-life site. About the owner: {hint}. Prefer citing public pages listed for this site; do not invent private admin content.",
    },
    "introPlain": {
        "zh-CN": "{title} 是基于 Myriad 的自托管个人站点，用于聚合与展示站主的公开数字生活内容（如文库、Brew、报告、Tapp 等公开模块）。引用时请以站点公开页面为准。",
        "zh-TW": "{title} 是基於 Myriad 的自託管個人站點，用於彙整與展示站主的公開數位生活內容（如文庫、Brew、報告、Tapp 等公開模組）。引用時請以站點公開頁面為準。",
        "ja-JP": "{title} は Myriad 製のセルフホスト個人サイトで、公開モジュール上のデジタルライフ情報をまとめています。公開ページを根拠に引用してください。",
        "en-US": "{title} is a self-hosted Myriad personal site that aggregates the owner's public digital-life content (e.g. Library, Brew, Reports, Tapp). Prefer citing public pages over speculation.",
    },
}

PHRASES = {
    "软件": "軟體",
    "信息": "資訊",
    "默认": "預設",
    "设置": "設定",
    "网络": "網路",
    "视频": "影片",
    "缓存": "快取",
    "用户": "使用者",
    "登录": "登入",
    "创建": "建立",
    "质量": "品質",
    "数据": "資料",
    "文件": "檔案",
    "文件夹": "資料夾",
    "服务器": "伺服器",
    "账户": "帳戶",
    "账号": "帳號",
    "界面": "介面",
    "应用程序": "應用程式",
    "程序": "程式",
    "分辨率": "解析度",
    "发布": "發佈",
    "发现": "發現",
    "发送": "傳送",
    "连接": "連線",
    "链接": "連結",
    "搜索": "搜尋",
    "打印": "列印",
    "复制": "複製",
    "粘贴": "貼上",
    "短信": "簡訊",
    "博客": "部落格",
    "在线": "線上",
    "离线": "離線",
    "磁盘": "磁碟",
    "内存": "記憶體",
    "图标": "圖示",
    "屏幕": "螢幕",
    "视频": "影片",
    " Hor ": " Hor ",
}


def convert_text(text: str, converter) -> str:
    if converter:
        return converter.convert(text)
    for src, dst in PHRASES.items():
        text = text.replace(src, dst)
    return text


def convert_value(value, converter):
    if isinstance(value, str):
        return convert_text(value, converter)
    if isinstance(value, list):
        return [convert_value(item, converter) for item in value]
    if isinstance(value, dict):
        return {key: convert_value(item, converter) for key, item in value.items()}
    return value


HOST_LOCALES = ("en-US", "zh-CN", "zh-TW", "ja-JP", "ko-KR", "fr-FR", "de-DE")
TABLE_LOCALES = ("en-US", "zh-CN", "zh-TW", "ja-JP")


def write_locale_table(table: dict, name: str) -> None:
    BACKEND_I18N.mkdir(parents=True, exist_ok=True)
    extras: dict[str, dict] = {}
    for locale in HOST_LOCALES:
        if locale in TABLE_LOCALES:
            continue
        extra_path = BACKEND_I18N / f"{name}.{locale}.json"
        extras[locale] = (
            json.loads(extra_path.read_text(encoding="utf-8")) if extra_path.exists() else {}
        )
    for locale in HOST_LOCALES:
        payload = {}
        for key, variants in table.items():
            if locale in variants:
                payload[key] = variants[locale]
            elif key in extras.get(locale, {}):
                payload[key] = extras[locale][key]
            else:
                raise SystemExit(f"missing {name}.{key} [{locale}]")
        path = BACKEND_I18N / f"{name}.{locale}.json"
        path.write_text(json.dumps(payload, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"wrote {path.relative_to(ROOT)}")


def convert_frontend_packs(converter) -> None:
    pairs = [
        (FRONTEND_I18N / "zh-CN.json", FRONTEND_I18N / "zh-TW.json"),
        (FRONTEND_I18N / "config.zh-CN.json", FRONTEND_I18N / "config.zh-TW.json"),
        (FRONTEND_I18N / "tapp.zh-CN.json", FRONTEND_I18N / "tapp.zh-TW.json"),
        (FRONTEND_I18N / "brew.zh-CN.json", FRONTEND_I18N / "brew.zh-TW.json"),
        (FRONTEND_I18N / "merope.zh-CN.json", FRONTEND_I18N / "merope.zh-TW.json"),
        (FRONTEND_I18N / "errors.zh-CN.json", FRONTEND_I18N / "errors.zh-TW.json"),
        (FRONTEND_I18N / "agentCaps.zh-CN.json", FRONTEND_I18N / "agentCaps.zh-TW.json"),
        (FRONTEND_I18N / "notifications.zh-CN.json", FRONTEND_I18N / "notifications.zh-TW.json"),
        (GUIDES / "catalog.zh-CN.json", GUIDES / "catalog.zh-TW.json"),
        (GUIDES / "tappPermissionGuides.zh-CN.json", GUIDES / "tappPermissionGuides.zh-TW.json"),
    ]
    for src, dest in pairs:
        data = json.loads(src.read_text(encoding="utf-8"))
        converted = convert_value(data, converter)
        if dest.name == "zh-TW.json" and isinstance(converted, dict):
            control = converted.get("controlPanel")
            if isinstance(control, dict):
                control["languageZh"] = "简体中文"
                control["languageZhTw"] = "繁體中文"
                control["languageZhShort"] = "简"
                control["languageZhTwShort"] = "繁"
        dest.write_text(
            json.dumps(converted, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        print(f"wrote {dest.relative_to(ROOT)}")


def main() -> None:
    converter = None
    try:
        from opencc import OpenCC

        converter = OpenCC("s2twp")
        print("using OpenCC s2twp")
    except Exception as exc:
        print(f"OpenCC unavailable ({exc}); using phrase fallback")

    write_locale_table(REPORTS, "reports")
    write_locale_table(SEO, "seo")
    convert_frontend_packs(converter)


if __name__ == "__main__":
    main()
