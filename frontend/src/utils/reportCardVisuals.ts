/**
 * Helpers for ReportCardWidget data resolution.
 *
 * Home dashboard cards fetch `/api/reports/latest` and must map each widget
 * to the correct platform report + card_visuals payload. These pure helpers
 * are unit-tested so field-mapping regressions surface as test failures.
 */

/** Minimal structural type — accepts WidgetConfig without index-signature friction. */
export interface WidgetConfigLike {
  type?: string
  config?: {
    platformId?: unknown
  } | null
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

  // Payload itself looks like card_visuals (legacy flat shape / direct prop)
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
 * Pick a platform report from `/api/reports/latest` (or catalog) list and
 * return non-empty card_visuals, or null (empty-render guard).
 */
export function pickPlatformCardVisuals(
  data: unknown,
  platformId: string,
): Record<string, unknown> | null {
  if (!data || typeof data !== 'object') return null
  const body = data as Record<string, unknown>
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
    return r.platform === platformId || content?.platform === platformId
  })

  const visuals = extractCardVisuals(report)
  return hasRenderableCardVisuals(visuals) ? visuals : null
}
