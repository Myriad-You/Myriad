import type { RankRow } from './RankList'
import type { TrendPoint } from './TrendChart'

export function aiDailyToTrendPoints(
  daily: Array<{ day: string; calls: number; tokens: number }>,
): TrendPoint[] {
  return daily.map((d) => ({
    day: d.day,
    views: d.calls,
    visitors: d.tokens,
  }))
}

export function aiUserDisplayName(
  row: {
    subject_id: number
    username?: string | null
    display_name?: string | null
  },
  anonymousLabel: string,
): string {
  if (row.subject_id <= 0) return anonymousLabel
  const name =
    (row.display_name && row.display_name.trim()) ||
    (row.username && row.username.trim()) ||
    ''
  if (name) return name
  return `#${row.subject_id}`
}

export function aiUsersToRankRows(
  users: Array<{
    subject_id: number
    username?: string | null
    display_name?: string | null
    calls: number
    tokens: number
  }>,
  opts: { anonymousLabel: string; callsLabel: (n: number) => string },
): RankRow[] {
  return users.map((u) => ({
    key: String(u.subject_id),
    name: aiUserDisplayName(u, opts.anonymousLabel),
    meta:
      u.subject_id > 0
        ? u.username && u.display_name && u.username !== u.display_name
          ? `@${u.username}`
          : `id ${u.subject_id}`
        : undefined,
    value: u.tokens,
    secondary: opts.callsLabel(u.calls),
  }))
}

export interface AiUsageSourceLabels {
  scheduler: string
  agent: string
  reports: string
  runtime: string
  merope: string
  playground: string
  speech: string
  brewlia: string
  prompt: string
  seo: string
  internal: string
  other: string
}

export function aiSourceDisplayName(
  source: string,
  labels: AiUsageSourceLabels,
): string {
  const key = source.trim().toLowerCase()
  if (key === 'scheduler' || key.startsWith('internal:scheduler')) {
    return labels.scheduler
  }
  if (key === 'agent') return labels.agent
  if (key === 'reports') return labels.reports
  if (key === 'merope') return labels.merope
  if (key === 'playground') return labels.playground
  if (key === 'speech') return labels.speech
  if (key === 'brewlia') return labels.brewlia
  if (key === 'prompt') return labels.prompt
  if (key === 'seo') return labels.seo
  if (key === 'internal') return labels.internal
  if (key === 'runtime' || key.startsWith('internal:')) {
    return labels.runtime
  }
  return source || labels.other
}

export function aiModelsToRankRows(
  models: Array<{
    model: string
    provider?: string
    calls: number
    tokens: number
  }>,
  opts: { callsLabel: (n: number) => string },
): RankRow[] {
  return models.map((m) => ({
    key: m.model,
    name: m.model || '—',
    meta: m.provider || undefined,
    value: m.tokens,
    secondary: opts.callsLabel(m.calls),
  }))
}
