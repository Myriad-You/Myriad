/**
 * Helpers for report-card field mapping used by home widgets + catalog clients.
 * Kept free of React so unit tests can cover the mapping edge cases.
 */

export type WidgetConfigLike = {
  type?: string
  config?: { platformId?: string; [key: string]: unknown }
}

/**
 * Resolve platform id for a report card widget.
 * Prefer explicit config; fall back to widget type suffix (`report-steam` → `steam`).
 */
export function resolveReportPlatformId(config: WidgetConfigLike): string {
  const fromConfig = config.config?.platformId
  if (typeof fromConfig === 'string' && fromConfig.trim()) {
    return fromConfig.trim()
  }
  const type = typeof config.type === 'string' ? config.type : ''
  if (type.startsWith('report-')) {
    const fromType = type.slice('report-'.length)
    if (fromType) return fromType
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
      : undefined

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
        if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
          return parsed as Record<string, unknown>
        }
      } catch {
        // ignore malformed string
      }
      continue
    }
    if (typeof raw === 'object' && !Array.isArray(raw)) {
      return raw as Record<string, unknown>
    }
  }

  // Payload itself looks like card_visuals (legacy flat shape)
  const looksLikeVisuals =
    r.hardcore_score != null ||
    r.danmaku != null ||
    r.library_items != null ||
    r.profile != null ||
    r.player_type != null ||
    r.contribution_level != null ||
    r.taste_profile != null ||
    r.vibe != null
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

/**
 * Find a platform report row in a /api/reports/latest-style payload.
 */
export function findPlatformReport(
  platformReports: unknown,
  platformId: string,
): Record<string, unknown> | null {
  if (!Array.isArray(platformReports)) return null
  const found = platformReports.find((r) => {
    if (!r || typeof r !== 'object') return false
    const row = r as Record<string, unknown>
    const content =
      row.content && typeof row.content === 'object'
        ? (row.content as Record<string, unknown>)
        : undefined
    return row.platform === platformId || content?.platform === platformId
  })
  return found && typeof found === 'object'
    ? (found as Record<string, unknown>)
    : null
}
