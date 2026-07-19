/**
 * Helpers for ReportCardWidget data resolution.
 *
 * Home dashboard cards fetch `/api/reports/latest` and must map each widget
 * to the correct platform report + card_visuals payload. These pure helpers
 * are unit-tested so field-mapping regressions surface as test failures.
 *
 * API shape (GET /api/reports/latest):
 * ```
 * {
 *   success: true,
 *   user_id?: number,
 *   platform_reports: Array<{
 *     platform: string,           // e.g. "steam"
 *     summary?: string,
 *     insights?: string[],
 *     card_visuals?: object,      // stats the platform widget renders
 *     cardVisuals?: object,       // camelCase alias (catalog)
 *     content?: { card_visuals?, platform? },
 *     report?: { card_visuals? }, // older nested shape
 *     ...
 *   }>
 * }
 * ```
 * Widget path: resolve platformId → pickPlatformCardVisuals → platform branch render.
 */

/** Structural input for report-card platform resolution (accepts WidgetConfig). */
export interface WidgetConfigLike {
  type?: string
  config?: {
    platformId?: unknown
  } | null
}

/** Canonical home widget platform ids (must match ReportCardWidget branches). */
export const REPORT_PLATFORM_IDS = [
  'bilibili',
  'steam',
  'github',
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

/**
 * Normalize free-form platform labels to a canonical ReportCard platform id.
 * Keeps unknown ids lowercased so equality matches still work when possible.
 */
export function normalizeReportPlatformId(raw: unknown): string {
  if (typeof raw !== 'string') return 'bilibili'
  const key = raw.trim().toLowerCase().replace(/\s+/g, ' ')
  if (!key) return 'bilibili'
  return PLATFORM_ALIASES[key] ?? key
}

/**
 * Resolve platform id for a report card widget.
 * Prefer explicit config; fall back to widget type suffix (`report-steam` → `steam`).
 * Always normalizes aliases (MyAnimeList → mal, Twitter → x, …).
 */
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

/**
 * Extract card_visuals from a platform report payload.
 * Tolerates snake_case / camelCase / catalog `content` nesting / double-encoded JSON.
 */
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
          return parsed as Record<string, unknown>
        }
      } catch {
        // ignore malformed string
      }
      continue
    }
    // Skip empty {} so later candidates (content.card_visuals, etc.) still win
    if (
      typeof raw === 'object' &&
      !Array.isArray(raw) &&
      Object.keys(raw as object).length > 0
    ) {
      return raw as Record<string, unknown>
    }
  }

  // Payload itself looks like card_visuals (legacy flat shape / direct prop)
  const looksLikeVisuals =
    r.hardcore_score != null ||
    r.danmaku != null ||
    r.library_items != null ||
    r.profile != null ||
    r.player_type != null ||
    r.contribution_level != null ||
    r.taste_profile != null ||
    r.vibe != null ||
    r.games_count != null ||
    r.gamerscore != null ||
    r.total_contributions != null ||
    r.stats != null ||
    r.gamer_type != null ||
    r.hunter_type != null ||
    r.online_id != null ||
    r.gamertag != null ||
    r.total_playtime != null ||
    r.repos_count != null ||
    r.trophy_level != null
  if (looksLikeVisuals && r.summary == null && r.insights == null) {
    return r
  }

  return null
}

/** True when visuals object has at least one renderable field. */
export function hasRenderableCardVisuals(
  visuals: Record<string, unknown> | null | undefined,
): boolean {
  if (!visuals || typeof visuals !== 'object') return false
  return Object.keys(visuals).length > 0
}

function platformMatches(candidate: unknown, platformId: string): boolean {
  if (typeof candidate !== 'string') return false
  return (
    normalizeReportPlatformId(candidate) ===
    normalizeReportPlatformId(platformId)
  )
}

/**
 * Pick a platform report from `/api/reports/latest` (or catalog / generate) list
 * and return non-empty card_visuals, or null (empty-render guard).
 *
 * This is the home ReportCardWidget data path: wrong platform match or empty
 * `{}` visuals previously mounted a blank shell with only the platform logo.
 */
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
    // Single report envelope or raw visuals
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

  // Prefer nested card_visuals; if the row *is* the visuals object (or nested
  // under report / content / data from older writers), still recover stats.
  let visuals = extractCardVisuals(report)
  if (!hasRenderableCardVisuals(visuals)) {
    const row = report as Record<string, unknown>
    for (const nestedKey of [
      'report',
      'content',
      'data',
      'payload',
    ] as const) {
      const nested = row[nestedKey]
      visuals = extractCardVisuals(nested)
      if (hasRenderableCardVisuals(visuals)) break
    }
  }
  return hasRenderableCardVisuals(visuals) ? visuals : null
}

/** Whether a platform id has a dedicated home ReportCard branch. */
export function isKnownReportPlatformId(id: string): boolean {
  return (REPORT_PLATFORM_IDS as readonly string[]).includes(
    normalizeReportPlatformId(id),
  )
}
