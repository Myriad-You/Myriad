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

export function stripReportHtml(html: string): string {
  if (!html) return ''
  return String(html)
    .replaceAll(/<br\s*\/?>/gi, '\n')
    .replaceAll(/<\/p>/gi, '\n')
    .replaceAll(/<[^>]+>/g, '')
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&quot;', '"')
    .replaceAll('&amp;', '&')
    .trim()
}

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

  if (parts.length) return parts.join('\n')

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
  }

  if (parts.length) return parts.join('\n')
  return fallbackPreview || ''
}

export function buildReportShareSnapshot(
  report: ReportShareSource | null | undefined,
): ReportShareSnapshot {
  const reportId = report?.id ?? report?.report_id ?? ''
  const platform =
    report?.platform || report?.platform_id || ''

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
