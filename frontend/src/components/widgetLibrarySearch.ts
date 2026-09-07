/**
 * Widget library search — matches against each widget's runtime metadata.
 *
 * Built-in and third-party (Tapp) widgets both go through the same path:
 * id / name / optional display label / any extra strings the registry provides
 * (category, tappId, description…). No preset allowlist.
 */

import {
  parseTappWidgetCategory,
  TAPP_CATEGORIES,
} from '../tapp/utils/tappCategories'

export interface WidgetLibrarySearchable {
  id: string
  name: string
  /** UI-resolved label when available (i18n or host-provided). */
  label?: string | null
  /** Free-form extras: category, tappId, description, aliases… */
  extras?: Array<string | null | undefined>
}

export function normalizeWidgetLibraryQuery(query: string): string {
  return query.trim().toLowerCase()
}

/** Expand id separators so "music player" can hit `music-player`. */
function idSearchVariants(id: string): string[] {
  const trimmed = id.trim()
  if (!trimmed) return []
  const spaced = trimmed.replace(/[-_./]+/g, ' ').replace(/\s+/g, ' ').trim()
  return spaced && spaced !== trimmed ? [trimmed, spaced] : [trimmed]
}

/** Collect searchable strings for one widget entry. */
export function collectWidgetLibrarySearchText(
  widget: WidgetLibrarySearchable,
): Array<string | null | undefined> {
  return [
    widget.label,
    widget.name,
    ...idSearchVariants(widget.id),
    ...(widget.extras ?? []),
  ]
}

export function widgetMatchesLibrarySearch(
  query: string,
  candidates: Array<string | null | undefined>,
): boolean {
  const q = normalizeWidgetLibraryQuery(query)
  if (!q) return true
  return candidates.some((candidate) => {
    const value = candidate?.trim().toLowerCase()
    return Boolean(value && value.includes(q))
  })
}

/** Convenience: match one library entry by its runtime fields. */
export function widgetTypeMatchesLibrarySearch(
  query: string,
  widget: WidgetLibrarySearchable,
): boolean {
  return widgetMatchesLibrarySearch(
    query,
    collectWidgetLibrarySearchText(widget),
  )
}

export type WidgetLibraryKindFilter = 'all' | 'report' | `tapp:${string}`

export interface WidgetLibraryKindSource {
  id: string
  isTappWidget?: boolean
  category?: string
}

/** Host widgets join the same topic rows as Tapp categories. */
const BUILTIN_TOPIC_BY_ID: Record<
  string,
  Exclude<WidgetLibraryKindFilter, 'all'>
> = {
  'agent-persona': 'tapp:ai',
  'music-player': 'tapp:media',
  'social-network': 'tapp:social',
  'friend-links': 'tapp:social',
  'game-presence': 'tapp:game',
  'quick-stats': 'tapp:data',
  'recent-activity': 'tapp:data',
  'visitor-stats': 'tapp:data',
  'github-repos': 'tapp:social',
}

export function classifyWidgetLibraryKind(
  widget: WidgetLibraryKindSource,
): Exclude<WidgetLibraryKindFilter, 'all'> {
  if (widget.isTappWidget) {
    return `tapp:${parseTappWidgetCategory(widget.category) ?? 'utility'}`
  }
  if (widget.id.startsWith('report-')) return 'report'
  if (widget.id.startsWith('platform-')) return 'tapp:social'
  return BUILTIN_TOPIC_BY_ID[widget.id] ?? 'tapp:utility'
}

export function widgetMatchesLibraryKind(
  filter: WidgetLibraryKindFilter,
  widget: WidgetLibraryKindSource,
): boolean {
  if (filter === 'all') return true
  return classifyWidgetLibraryKind(widget) === filter
}

/**
 * 全部, then only kinds that currently have a widget.
 * Host builtins share Tapp topic rows (媒体 / 社交 / …) instead of a dump bucket.
 */
export function presentWidgetLibraryKindFilters(
  widgets: WidgetLibraryKindSource[],
): WidgetLibraryKindFilter[] {
  const seen = new Set<Exclude<WidgetLibraryKindFilter, 'all'>>()
  for (const widget of widgets) {
    seen.add(classifyWidgetLibraryKind(widget))
  }
  const chips: WidgetLibraryKindFilter[] = ['all']
  if (seen.has('report')) chips.push('report')
  for (const category of TAPP_CATEGORIES) {
    const kind = `tapp:${category}` as const
    if (seen.has(kind)) chips.push(kind)
  }
  for (const kind of [...seen].sort()) {
    if (kind.startsWith('tapp:') && !chips.includes(kind)) chips.push(kind)
  }
  return chips
}

export function tappCategoryFromKindFilter(
  filter: WidgetLibraryKindFilter,
): string | null {
  if (!filter.startsWith('tapp:')) return null
  return filter.slice('tapp:'.length) || null
}
