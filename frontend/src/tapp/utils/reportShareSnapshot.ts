/**
 * Report share snapshot helpers for Aro chat / federation.
 *
 * Field names (stable contract): report_id, summary, platform, content_preview.
 * Snapshot is intentionally shallow — no full report JSON — so chat recipients
 * can render without user-scoped getReport.
 *
 * Aro embeds a mirrored copy in its page modules (sandbox cannot import this file).
 * Federation Article (content.rs) emits the same snake_case names plus mfp:* aliases.
 * Keep both in sync when changing field names or extraction rules.
 */

/** Stable Aro/federation report-share snapshot field names (order fixed for docs/tests). */
export const REPORT_SHARE_SNAPSHOT_FIELDS = [
  'report_id',
  'summary',
  'platform',
  'content_preview',
] as const

export interface ReportShareSnapshot {
  report_id: string
  summary: string
  platform: string
  content_preview: string
}

export interface ReportShareSource {
  id?: string | number | null
  report_id?: string | number | null
  platform?: string | null
  platform_id?: string | null
  summary?: string | null
  report_title?: string | null
  type?: string | null
  content_preview?: string | null
  content?: unknown
}

const PREVIEW_MAX = 500

/** Strip simple HTML to plain text (chat-safe). */
export function stripReportHtml(html: string): string {
  if (!html) return ''
  return String(html)
    .replace(/<br\s*\/?>/gi, '\n')
    .replace(/<\/p>/gi, '\n')
    .replace(/<[^>]+>/g, '')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, '&')
    .trim()
}

/**
 * Format structured report content into readable plain text.
 * Never returns "[object Object]" — objects are walked for known fields.
 */
export function formatReportContentBody(
  content: unknown,
  fallbackPreview = '',
): string {
  if (content == null || content === '') return fallbackPreview || ''
  if (typeof content === 'string') {
    return stripReportHtml(content) || fallbackPreview || ''
  }
  if (typeof content === 'number' || typeof content === 'boolean') {
    return String(content)
  }
  if (typeof content !== 'object') {
    return fallbackPreview || ''
  }

  const obj = content as Record<string, unknown>
  const parts: string[] = []

  if (typeof obj.summary === 'string' && obj.summary.trim()) {
    parts.push(obj.summary.trim())
  }

  if (Array.isArray(obj.insights)) {
    for (const item of obj.insights) {
      if (item == null || item === '') continue
      if (typeof item === 'string' || typeof item === 'number') {
        parts.push(`• ${String(item)}`)
      }
    }
  }

  const analysis = obj['综合分析']
  if (analysis && typeof analysis === 'object') {
    const a = analysis as Record<string, unknown>
    if (typeof a['总体画像'] === 'string' && a['总体画像'].trim()) {
      parts.push(String(a['总体画像']).trim())
    } else if (a.content && typeof a.content === 'object') {
      const c = a.content as Record<string, unknown>
      if (typeof c['总体画像'] === 'string' && c['总体画像'].trim()) {
        parts.push(String(c['总体画像']).trim())
      }
    }
  } else if (typeof analysis === 'string' && analysis.trim()) {
    parts.push(analysis.trim())
  }

  if (parts.length) return parts.join('\n')

  // Last resort: primitive key/value lines (not JSON dump, not [object Object])
  try {
    for (const key of Object.keys(obj).slice(0, 12)) {
      const v = obj[key]
      if (v == null) continue
      if (
        typeof v === 'string' ||
        typeof v === 'number' ||
        typeof v === 'boolean'
      ) {
        const s = String(v).trim()
        if (s) parts.push(`${key}: ${s}`)
      }
    }
  } catch {
    /* ignore */
  }

  if (parts.length) return parts.join('\n')
  return fallbackPreview || ''
}

/** Build chat/federation-safe snapshot from a catalog report or partial payload. */
export function buildReportShareSnapshot(
  report: ReportShareSource | null | undefined,
): ReportShareSnapshot {
  const reportId =
    report && report.id != null
      ? report.id
      : report && report.report_id != null
        ? report.report_id
        : ''
  const platform =
    (report && (report.platform || report.platform_id)) || ''

  let summary = ''
  if (report) {
    if (report.summary) summary = String(report.summary)
    else if (report.report_title) summary = String(report.report_title)
    else if (report.type) summary = String(report.type)
  }

  let preview = ''
  if (report) {
    if (report.content_preview) preview = String(report.content_preview)
    else if (report.summary) preview = String(report.summary)
    else preview = formatReportContentBody(report.content, '')
  }

  preview = stripReportHtml(preview || '').trim()
  if (preview.length > PREVIEW_MAX) preview = preview.slice(0, PREVIEW_MAX)
  if (!summary) summary = preview ? preview.slice(0, 80) : 'Report'

  return {
    report_id: reportId != null && reportId !== '' ? String(reportId) : '',
    summary,
    platform: platform ? String(platform) : '',
    content_preview: preview,
  }
}

/**
 * Wire Aro message payload fields for a report share.
 * Always sets report_id, summary, platform, content_preview (never id-only).
 */
export function wireReportSharePayload(
  base: Record<string, unknown>,
  attach: {
    reportId?: string | number | null
    summary?: string | null
    name?: string | null
    platform?: string | null
    contentPreview?: string | null
    desc?: string | null
  },
): Record<string, unknown> {
  const summary = String(attach.summary || attach.name || '').trim() || 'Report'
  const platform = String(attach.platform || '').trim()
  const content_preview = String(
    attach.contentPreview || attach.desc || '',
  ).trim()
  const report_id =
    attach.reportId != null && attach.reportId !== ''
      ? String(attach.reportId)
      : ''

  return {
    ...base,
    report_id,
    summary,
    platform,
    content_preview,
    title: (base.title as string) || summary,
    description:
      (base.description as string) ||
      (content_preview
        ? platform
          ? `${platform} · ${content_preview}`
          : content_preview
        : platform),
  }
}
