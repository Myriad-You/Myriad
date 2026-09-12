import { normalizeJsonMediaUrls } from './proxyImageUrl'

export interface WidgetConfigLike {
  type?: string
  config?: {
    platformId?: unknown
  } | null
}

export const REPORT_PLATFORM_IDS = [
  'bilibili',
  'steam',
  'github',
  'youtube',
  'netease',
  'bangumi',
  'mal',
  'x',
  'xbox',
  'psn',
  'discord',
] as const

export type ReportPlatformId = (typeof REPORT_PLATFORM_IDS)[number]

const PLATFORM_ALIASES: Record<string, ReportPlatformId> = {
  bilibili: 'bilibili',
  b站: 'bilibili',
  steam: 'steam',
  github: 'github',
  netease: 'netease',
  'netease music': 'netease',
  'netease cloud music': 'netease',
  '网易云': 'netease',
  '网易云音乐': 'netease',
  bangumi: 'bangumi',
  bgm: 'bangumi',
  mal: 'mal',
  myanimelist: 'mal',
  'my anime list': 'mal',
  x: 'x',
  twitter: 'x',
  'x (twitter)': 'x',
  xbox: 'xbox',
  psn: 'psn',
  playstation: 'psn',
  'play station': 'psn',
  discord: 'discord',
}

export function normalizeReportPlatformId(raw: unknown): string {
  if (typeof raw !== 'string') return 'bilibili'
  const key = raw.trim().toLowerCase().replaceAll(/\s+/g, ' ')
  if (!key) return 'bilibili'
  return PLATFORM_ALIASES[key] ?? key
}

export function resolveReportPlatformId(config: WidgetConfigLike): string {
  const fromConfig = config.config?.platformId
  if (typeof fromConfig === 'string' && fromConfig.trim()) {
    return normalizeReportPlatformId(fromConfig)
  }
  const type = typeof config.type === 'string' ? config.type : ''
  if (type.startsWith('report-')) {
    const fromType = type.slice('report-'.length)
    if (fromType) return normalizeReportPlatformId(fromType)
  }
  return 'bilibili'
}

export function extractCardVisuals(
  report: unknown,
): Record<string, unknown> | null {
  if (!report || typeof report !== 'object') return null
  const r = report as Record<string, unknown>
  const content =
    r.content && typeof r.content === 'object' && !Array.isArray(r.content)
      ? (r.content as Record<string, unknown>)
      : null

  const candidates = [
    r.card_visuals,
    r.cardVisuals,
    content?.card_visuals,
    content?.cardVisuals,
  ]

  for (const raw of candidates) {
    if (raw == null) continue
    if (typeof raw === 'string') {
      try {
        const parsed = JSON.parse(raw) as unknown
        if (
          parsed &&
          typeof parsed === 'object' &&
          !Array.isArray(parsed) &&
          Object.keys(parsed as object).length > 0
        ) {
          return normalizeJsonMediaUrls(parsed as Record<string, unknown>)
        }
      } catch {
        // ignore malformed string
      }
      continue
    }
    // Skip empty {}.
    if (
      typeof raw === 'object' &&
      !Array.isArray(raw) &&
      Object.keys(raw as object).length > 0
    ) {
      return normalizeJsonMediaUrls(raw as Record<string, unknown>)
    }
  }

  const looksLikeVisuals =
    r.hardcore_score != null ||
    r.danmaku != null ||
    r.library_items != null ||
    r.recent_videos != null ||
    r.profile != null ||
    r.player_type != null ||
    r.contribution_level != null ||
    r.taste_profile != null ||
    r.vibe != null ||
    r.games_count != null ||
    r.gamerscore != null ||
    r.total_contributions != null ||
    r.total_stars != null ||
    r.contribution_calendar != null ||
    r.languages != null ||
    r.stats != null ||
    r.gamer_type != null ||
    r.hunter_type != null ||
    r.online_id != null ||
    r.gamertag != null ||
    r.total_playtime != null ||
    r.repos_count != null ||
    r.trophy_level != null ||
    r.subscriber_count != null ||
    r.video_count != null ||
    r.view_count != null ||
    r.channel_title != null ||
    r.is_empty_channel != null ||
    r.video_summary != null ||
    r.status_counts != null
  if (looksLikeVisuals && r.summary == null && r.insights == null) {
    return normalizeJsonMediaUrls(r)
  }

  return null
}

export function hasRenderableCardVisuals(
  visuals: Record<string, unknown> | null | undefined,
): boolean {
  if (!visuals || typeof visuals !== 'object') return false
  return Object.keys(visuals).length > 0
}

export function hasReportDetailContent(
  visuals: Record<string, unknown> | null | undefined,
): boolean {
  if (!visuals || typeof visuals !== 'object') return false
  const nonEmpty = (key: string) => {
    const v = visuals[key]
    return Array.isArray(v) && v.length > 0
  }
  return (
    nonEmpty('library_items') ||
    nonEmpty('following_highlights') ||
    nonEmpty('following_sample') ||
    nonEmpty('recent_videos') ||
    nonEmpty('guilds_preview') ||
    nonEmpty('top_posts') ||
    nonEmpty('recent_posts')
  )
}

function platformMatches(candidate: unknown, platformId: string): boolean {
  if (typeof candidate !== 'string') return false
  return (
    normalizeReportPlatformId(candidate) ===
    normalizeReportPlatformId(platformId)
  )
}

export function pickPlatformCardVisuals(
  data: unknown,
  platformId: string,
): Record<string, unknown> | null {
  if (!data || typeof data !== 'object') return null
  const body = data as Record<string, unknown>
  const want = normalizeReportPlatformId(platformId)

  const reports = Array.isArray(body.platform_reports)
    ? body.platform_reports
    : Array.isArray(body.reports)
      ? body.reports
      : null
  if (!reports) {
    const visuals = extractCardVisuals(body)
    return hasRenderableCardVisuals(visuals) ? visuals : null
  }

  const report = reports.find((item: unknown) => {
    if (!item || typeof item !== 'object') return false
    const r = item as Record<string, unknown>
    const content =
      r.content && typeof r.content === 'object'
        ? (r.content as Record<string, unknown>)
        : null
    return (
      platformMatches(r.platform, want) ||
      platformMatches(content?.platform, want)
    )
  })

  if (!report) return null

  return coerceReportVisuals(report)
}

export function isKnownReportPlatformId(id: string): boolean {
  return (REPORT_PLATFORM_IDS as readonly string[]).includes(
    normalizeReportPlatformId(id),
  )
}

export function coerceReportVisuals(
  input: unknown,
): Record<string, unknown> | null {
  if (!input || typeof input !== 'object' || Array.isArray(input)) return null
  const root = input as Record<string, unknown>

  let visuals = extractCardVisuals(root)

  if (!hasRenderableCardVisuals(visuals)) {
    for (const nestKey of ['report', 'content', 'data', 'payload'] as const) {
      const nested = root[nestKey]
      visuals = extractCardVisuals(nested)
      if (hasRenderableCardVisuals(visuals)) break
    }
  }

  if (visuals) {
    const nestedOnly =
      Object.keys(visuals).length <= 3 &&
      (visuals.card_visuals != null ||
        visuals.cardVisuals != null ||
        visuals.data != null)
    if (nestedOnly) {
      const unwrapped = extractCardVisuals(visuals)
      if (hasRenderableCardVisuals(unwrapped)) visuals = unwrapped
    }
  }

  if (!hasRenderableCardVisuals(visuals)) {
    const sibling = extractCardVisuals({
      ...root,
      summary: undefined,
      insights: undefined,
    })
    if (hasRenderableCardVisuals(sibling)) visuals = sibling
  }

  if (!hasRenderableCardVisuals(visuals)) return null

  const merged: Record<string, unknown> = { ...visuals }
  for (const key of [
    'library_items',
    'recent_videos',
    'guilds_preview',
    'following_highlights',
    'following_sample',
    'interest_circles',
    'top_titles',
    'profile',
    'stats',
    'avatar',
    'personaname',
    'gamertag',
    'online_id',
    'subscriber_count',
    'view_count',
    'video_count',
    'channel_title',
    'is_empty_channel',
    'video_summary',
    'status_counts',
    'total_contributions',
    'repos_count',
    'total_stars',
    'contribution_level',
    'contribution_calendar',
    'languages',
    'hardcore_score',
    'danmaku',
    'games_count',
    'player_type',
  ] as const) {
    if (merged[key] == null && root[key] != null) {
      merged[key] = root[key]
    }
    for (const nestKey of ['report', 'content', 'data'] as const) {
      const nest = root[nestKey]
      if (
        merged[key] == null &&
        nest &&
        typeof nest === 'object' &&
        !Array.isArray(nest)
      ) {
        const n = nest as Record<string, unknown>
        if (n[key] != null) merged[key] = n[key]
        const cv = n.card_visuals ?? n.cardVisuals
        if (
          merged[key] == null &&
          cv &&
          typeof cv === 'object' &&
          !Array.isArray(cv)
        ) {
          const c = cv as Record<string, unknown>
          if (c[key] != null) merged[key] = c[key]
        }
      }
    }
  }

  return hasRenderableCardVisuals(merged) ? merged : null
}
