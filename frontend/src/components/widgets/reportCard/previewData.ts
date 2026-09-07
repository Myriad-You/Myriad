/**
 * Widget-library preview fixtures for report cards (no network).
 * SVG data URIs keep previews offline-safe for avatar walls / posters.
 */
import type { useI18n } from '../../../contexts/I18nContext'

type I18nT = ReturnType<typeof useI18n>['t']

export function buildReportCardPreviewData(
  platformId: string,
  t: I18nT,
): Record<string, unknown> {
const previewAvatar = (letter: string, bg: string) => {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect width="96" height="96" rx="48" fill="${bg}"/><text x="48" y="58" text-anchor="middle" fill="#fff" font-size="36" font-family="system-ui,sans-serif" font-weight="700">${letter}</text></svg>`
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
}
const previewCover = (letter: string, bg: string, w = 160, h = 200) => {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="${bg}"/><stop offset="100%" stop-color="#111827"/></linearGradient></defs><rect width="${w}" height="${h}" fill="url(#g)"/><text x="${w / 2}" y="${h / 2 + 12}" text-anchor="middle" fill="#fff" font-size="42" font-family="system-ui,sans-serif" font-weight="700" opacity="0.92">${letter}</text></svg>`
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
}

// YouTube: channel stats + recent uploads (matches generate.rs card_visuals)
// DEV force-mock on home — rotate fixtures freely while tuning the face.
if (platformId === 'youtube') {
  const thumb = (letter: string, bg: string) => {
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="${bg}"/><stop offset="100%" stop-color="#111827"/></linearGradient></defs><rect width="320" height="180" fill="url(#g)"/><text x="160" y="98" text-anchor="middle" fill="#fff" font-size="28" font-family="system-ui,sans-serif" font-weight="700" opacity="0.92">${letter}</text></svg>`
    return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
  }
  const daysAgo = (n: number) =>
    new Date(Date.now() - n * 86_400_000).toISOString()
  // Widget-library / isPreview fixture — gold-tier tech channel (representative)
  const videos = [
    {
      title: '从零搭一个个人站：技术选型与踩坑',
      letter: '01',
      bg: '#b91c1c',
      view_count: 52_000,
      like_count: 3_200,
      comment_count: 180,
      duration: 'PT12M34S',
      days: 3,
    },
    {
      title: 'Rust 后端实战：Axum 路由与鉴权',
      letter: '02',
      bg: '#991b1b',
      view_count: 31_000,
      like_count: 1_100,
      comment_count: 64,
      duration: 'PT8M5S',
      days: 14,
    },
    {
      title: '一周开发日志 #27 — 报告卡片重设计',
      letter: '03',
      bg: '#7f1d1d',
      view_count: 18_400,
      like_count: 890,
      comment_count: 42,
      duration: 'PT1H2M3S',
      days: 40,
    },
    {
      title: '小屏幕 UI 密度：4×2 小组件怎么排',
      letter: '04',
      bg: '#dc2626',
      view_count: 9_200,
      like_count: 410,
      comment_count: 19,
      duration: 'PT4M20S',
      days: 70,
    },
    {
      title: '公开 API 限流与缓存策略随记',
      letter: '05',
      bg: '#ef4444',
      view_count: 6_800,
      like_count: 280,
      duration: 'PT22M',
      days: 100,
    },
  ]
  return {
    channel_title: 'Myriad Studio',
    username: 'Myriad Studio',
    custom_url: '@myriadstudio',
    channel_url: 'https://www.youtube.com/@myriadstudio',
    channel_id: 'UCpreviewYouTubeMock',
    // vibe ≤20 字 / line-clamp-2（与 X 卡 + AI prompt 同规）
    vibe: '技术日志型创作者，上传稳均播不虚',
    channel_type: '稳定更新',
    // 24.8K → Gold on card award scale (≥10K)
    subscriber_count: 24_800,
    view_count: 1_920_000,
    video_count: 64,
    is_empty_channel: false,
    video_summary:
      '64 个公开视频，2.48 万订阅，累计 192 万观看；开发日志与 UI 实验',
    summary: '「Myriad Studio」：技术站气质，上传稳、均播不虚。',
    insights: [
      '订阅 24.8K / 视频 64，体量匹配金牌档。',
      '题材收敛在个人站与工程实践。',
      '适合作为组件库预览的代表性 YouTube 卡数据。',
    ],
    avatar: previewAvatar('M', '#FF0000'),
    library_items: videos.map((v, i) => {
      const cover = thumb(v.letter, v.bg)
      return {
        title: v.title,
        type: 'video',
        image: cover,
        cover,
        view_count: v.view_count,
        like_count: v.like_count,
        comment_count: v.comment_count,
        duration: v.duration,
        published_at: daysAgo(v.days),
        video_id: `preview${i + 1}`,
        url: `https://www.youtube.com/watch?v=preview${i + 1}`,
      }
    }),
    recent_videos: videos.slice(0, 3).map((v, i) => ({
      title: v.title,
      video_id: `preview${i + 1}`,
      cover: thumb(v.letter, v.bg),
      view_count: v.view_count,
      like_count: v.like_count,
      duration: v.duration,
      published_at: daysAgo(v.days),
    })),
  }
}

// Discord 卡片字段结构与其他平台差异较大（profile/stats/library_items
// 形状不同），单独给一份预览数据，避免与通用预览字段互相污染。
if (platformId === 'discord') {
  const guildTile = (letter: string, bg: string) => {
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect width="96" height="96" rx="24" fill="${bg}"/><text x="48" y="62" text-anchor="middle" fill="#fff" font-size="44" font-family="system-ui,sans-serif" font-weight="700">${letter}</text></svg>`
    return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
  }
  return {
    vibe: t.reportCardWidget.discordVibeDefault,
    role_profile: t.reportCardWidget.discordRoleDefault,
    community_tags: [
      t.reportCardWidget.discordTagOpenSource,
      t.reportCardWidget.discordTagIndieGame,
      t.reportCardWidget.discordTagAcg,
    ],
    profile: {
      display_name: 'PreviewUser',
      username: 'preview',
      avatar_url: previewAvatar('D', '#5865F2'),
      nitro: 'Nitro',
      account_age_years: 7,
      badges: [
        'Active Developer',
        'HypeSquad Balance',
        'Early Supporter',
      ],
      mfa_enabled: true,
    },
    stats: {
      guilds: 42,
      owned_guilds: 2,
      admin_guilds: 5,
      manage_guilds: 8,
      connections: 4,
      member_reach: 128000,
      online_reach: 21000,
    },
    linked_platforms: ['github', 'steam', 'spotify', 'youtube'],
    connections: [
      { type: 'github', name: 'octocat', verified: true },
      { type: 'steam', name: 'PreviewGamer', verified: true },
      { type: 'spotify', name: 'preview', verified: false },
    ],
    library_items: [
      {
        id: 'g1',
        name: 'Open Source Guild',
        title: 'Open Source Guild',
        icon: guildTile('O', '#5865F2'),
        owner: true,
        permissions: ['ADMINISTRATOR'],
        member_count: 8200,
        presence_count: 1400,
        features: ['COMMUNITY'],
      },
      {
        id: 'g2',
        name: 'Indie Devs',
        title: 'Indie Devs',
        icon: guildTile('I', '#4752C4'),
        owner: false,
        permissions: ['MANAGE_GUILD'],
        member_count: 25000,
        presence_count: 3800,
        features: ['PARTNERED'],
      },
      {
        id: 'g3',
        name: 'ACG Lounge',
        title: 'ACG Lounge',
        icon: guildTile('A', '#7289DA'),
        owner: false,
        permissions: [],
        member_count: 61000,
        presence_count: 9200,
        features: ['VERIFIED'],
      },
      {
        id: 'g4',
        name: 'Pixel Art',
        title: 'Pixel Art',
        icon: guildTile('P', '#949CF7'),
        owner: false,
        permissions: [],
        member_count: 4300,
        presence_count: 700,
        features: [],
      },
      {
        id: 'g5',
        name: 'Rust Nomads',
        title: 'Rust Nomads',
        icon: guildTile('R', '#3C45A5'),
        owner: false,
        permissions: [],
        member_count: 12000,
        presence_count: 1900,
        features: ['COMMUNITY'],
      },
    ],
  }
}

const sampleGame = t.reportCardWidget.sampleGame
const sampleAnime = t.reportCardWidget.sampleAnime
const samplePlaylist = t.reportCardWidget.samplePlaylist
const sampleProject = t.reportCardWidget.sampleProject
const sampleManga = t.reportCardWidget.sampleManga
const sampleGame2 = t.reportCardWidget.sampleGame2
const sampleAnime2 = t.reportCardWidget.sampleAnime2

// Bangumi 与 MAL 的条目类型词表并不重合（MAL 只有 anime/manga，Bangumi 没有
// manga）。共用一份并集会让两张卡都画出对方的类型段，且落到未翻译的原始键 +
// 兜底颜色，所以这两个字段按平台分发。
const isMal = platformId === 'mal'
const animeTypeDistribution = isMal
  ? { anime: 80, manga: 28 }
  : { anime: 80, book: 28, game: 22, music: 12, real: 8 }

return {
  // —— 通用评分 / 身份标签 ——
  hardcore_score: 85,
  // Stable enum key (FE maps via i18n); not a localized string
  player_type: 'hardcore',
  gamer_type: t.reportCardWidget.xboxGamerDefault,
  hunter_type: t.reportCardWidget.psnHunterDefault,
  contribution_level: t.reportCardWidget.seniorDev,
  taste_profile: isMal
    ? t.reportCardWidget.malTasteDefault
    : t.reportCardWidget.bangumiTasteDefault,

  // —— Steam ——
  games_count: 120,
  total_playtime: 2500,
  personaname: 'PreviewGamer',
  personastate_label: 'online',
  is_online: true,
  is_in_game: false,
  avatar: previewAvatar('S', '#1b2838'),
  recent_2weeks_minutes: 840,

  // —— Xbox ——
  gamertag: 'PreviewGamer',
  gamerscore: 12500,
  total_achievements: 340,
  completion_rate: 42,
  completed_games: 8,

  // —— PSN ——
  online_id: 'PreviewPSN',
  trophy_level: 245,
  platinum_count: 18,
  total_trophies: 1260,

  // —— GitHub ——
  total_contributions: 1200,
  repos_count: 45,
  total_stars: 890,
  languages: [
    { name: 'TypeScript', percentage: 42 },
    { name: 'Rust', percentage: 28 },
    { name: 'Python', percentage: 18 },
    { name: 'Go', percentage: 12 },
  ],

  // —— 网易云 ——
  follower_count: 1200,
  playlist_count: 15,
  level: 8,
  mood_keywords: [
    t.reportCardWidget.happyMood,
    t.reportCardWidget.sadMood,
    t.reportCardWidget.passionateMood,
    t.reportCardWidget.calmMood,
    t.reportCardWidget.nightMood,
  ],

  // —— Bilibili 弹幕 + 右下角统计 ——
  danmaku: t.reportCard.danmakuDefault as unknown as string[],
  user_level: 6,
  following_count: 86,

  // —— Bangumi / MAL 收藏结构 ——
  // Widget overview only shows done / doing / wish
  status_counts: {
    done: 128,
    doing: 12,
    wish: 45,
  },
  subject_type_distribution: animeTypeDistribution,

  // —— 详情轮播 / 海报墙（多平台共用，字段取并集） ——
  library_items: [
    {
      title: sampleProject,
      type: 'repo',
      language: 'TypeScript',
      stars: 120,
      forks: 30,
      description: t.reportCardWidget.sampleProjectDesc,
    },
    {
      title: sampleGame,
      type: 'game',
      cover: previewCover('G', '#1b2838', 320, 150),
      progress: 100,
      achievements_earned: 48,
      achievements_total: 48,
      gamerscore: 1000,
      platinum: true,
    },
    {
      title: sampleGame2,
      type: 'game',
      cover: previewCover('H', '#107C10', 320, 150),
      progress: 72,
      achievements_earned: 36,
      achievements_total: 50,
      gamerscore: 640,
      platinum: false,
    },
    {
      title: sampleAnime,
      type: 'anime',
      cover: previewCover('A', '#f09199'),
      rate: 9,
    },
    {
      title: sampleAnime2,
      type: 'anime',
      cover: previewCover('B', '#2e51a2'),
      rate: 8,
    },
    {
      title: sampleManga,
      type: 'book',
      cover: previewCover('M', '#e11d48'),
      rate: 10,
    },
    {
      title: samplePlaylist,
      type: 'music',
      cover: previewCover('♪', '#e60026', 200, 200),
    },
    {
      title: t.reportCardWidget.samplePlaylist2,
      type: 'music',
      cover: previewCover('♫', '#7B68EE', 200, 200),
    },
  ],
  // Xbox / PSN 无 library 封面时的回退列表
  top_titles: [
    {
      name: sampleGame,
      title: sampleGame,
      progress: 100,
      platinum: true,
      cover: previewCover('G', '#1b2838', 320, 150),
      achievements_earned: 48,
      achievements_total: 48,
      gamerscore: 1000,
    },
    {
      name: sampleGame2,
      title: sampleGame2,
      progress: 72,
      platinum: false,
      cover: previewCover('H', '#107C10', 320, 150),
      achievements_earned: 36,
      achievements_total: 50,
      gamerscore: 640,
    },
    {
      name: t.reportCardWidget.sampleGame3,
      title: t.reportCardWidget.sampleGame3,
      progress: 45,
      platinum: false,
      cover: previewCover('J', '#0070D1', 320, 150),
      achievements_earned: 18,
      achievements_total: 40,
      gamerscore: 280,
    },
  ],

  // —— X 关注图谱 + 人设 ——
  vibe: t.reportCardWidget.xVibeDefault,
  engagement_level: t.reportCardWidget.xEngagementDefault,
  signature_topics: [
    t.reportCardWidget.xCircleIndie,
    t.reportCardWidget.xCircleOpenSource,
    t.reportCardWidget.xCircleArt,
  ],
  profile: {
    username: 'preview',
    name: 'Preview',
    avatar: previewAvatar('P', '#111827'),
  },
  stats: {
    followers: 1280,
    following: 420,
    posts: 86,
  },
  interest_circles: [
    {
      name: t.reportCardWidget.xCircleIndie,
      count: 22,
      accounts: ['pixelcraft', 'roguelike'],
    },
    {
      name: t.reportCardWidget.xCircleOpenSource,
      count: 15,
      accounts: ['octocat_lab'],
    },
    {
      name: t.reportCardWidget.xCircleArt,
      count: 9,
      accounts: ['inkwave'],
    },
  ],
  following_highlights: [
    {
      username: 'pixelcraft',
      name: 'PixelCraft',
      tag: t.reportCardWidget.xTagIndie,
    },
    {
      username: 'octocat_lab',
      name: 'Octocat Lab',
      tag: t.reportCardWidget.xTagTech,
    },
    {
      username: 'inkwave',
      name: 'Ink Wave',
      tag: t.reportCardWidget.xTagArt,
    },
  ],
  following_sample: [
    {
      username: 'pixelcraft',
      name: 'PixelCraft',
      description: t.reportCardWidget.xPreviewDescIndie,
      follower_count: 18200,
      avatar: previewAvatar('P', '#2563eb'),
    },
    {
      username: 'octocat_lab',
      name: 'Octocat Lab',
      description: t.reportCardWidget.xPreviewDescTech,
      follower_count: 9400,
      avatar: previewAvatar('O', '#7c3aed'),
    },
    {
      username: 'inkwave',
      name: 'Ink Wave',
      description: t.reportCardWidget.xPreviewDescArt,
      follower_count: 5600,
      avatar: previewAvatar('I', '#db2777'),
    },
    {
      username: 'roguelike',
      name: 'RogueLike',
      description: t.reportCardWidget.xPreviewDescIndie,
      follower_count: 3100,
      avatar: previewAvatar('R', '#d97706'),
    },
    {
      username: 'synthwave',
      name: 'SynthWave',
      description: t.reportCardWidget.xPreviewDescArt,
      follower_count: 2200,
      avatar: previewAvatar('S', '#0891b2'),
    },
    {
      username: 'typecraft',
      name: 'TypeCraft',
      description: t.reportCardWidget.xPreviewDescTech,
      follower_count: 4800,
      avatar: previewAvatar('T', '#059669'),
    },
    {
      username: 'loomstudio',
      name: 'Loom Studio',
      description: t.reportCardWidget.xPreviewDescArt,
      follower_count: 1700,
      avatar: previewAvatar('L', '#e11d48'),
    },
  ],
}
}
