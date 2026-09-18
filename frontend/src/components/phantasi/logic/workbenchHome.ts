/** 工作台概览：接着写、即将发布、桌上各一件。坏了的只收成一行。 */

import { isSiteSource } from './board'
import { mediaPointerUrl } from './mediaPointer'

export const WORKBENCH_HOME_DRAFTS = 3
export const WORKBENCH_HOME_UPCOMING = 2
export const WORKBENCH_HOME_FAIL_SOURCE_ERRORS = 2
export const WORKBENCH_HOME_QUIET = 3

export function workbenchHomeMediaFace(item: {
  url: string
  mime: string
}): { src: string; video: boolean } | null {
  const src = mediaPointerUrl(item.url)
  if (!src) return null
  if (item.mime.startsWith('video/')) return { src, video: true }
  if (item.mime.startsWith('image/')) return { src, video: false }
  return null
}

export function workbenchFeedSourceCount(
  sources: ReadonlyArray<{ source_type: string }>,
): number {
  return sources.filter((source) => source.source_type !== 'note').length
}

export function workbenchHomeIsEmpty(
  noteCount: number,
  feedCount: number,
  mediaCount = 0,
  reviewCount = 0,
): boolean {
  return (
    noteCount === 0 && feedCount === 0 && mediaCount === 0 && reviewCount === 0
  )
}

export function workbenchPendingReviews<T extends { status: string }>(
  rows: readonly T[],
): T[] {
  return rows.filter((row) => row.status === 'pending')
}

export function workbenchHomeDrafts<
  T extends { status: string; updated_at: number; last_error?: string | null },
>(docs: readonly T[], limit = WORKBENCH_HOME_DRAFTS): T[] {
  return docs
    .filter((doc) => doc.status === 'draft' && !doc.last_error)
    .toSorted((a, b) => b.updated_at - a.updated_at)
    .slice(0, limit)
}

export function workbenchHomeUpcoming<
  T extends { status: string; scheduled_at: number | null; last_error?: string | null },
>(docs: readonly T[], limit = WORKBENCH_HOME_UPCOMING): T[] {
  return docs
    .filter((doc) => doc.status === 'scheduled' && !doc.last_error)
    .toSorted((a, b) => {
      const left = a.scheduled_at
      const right = b.scheduled_at
      if (left == null && right == null) return 0
      if (left == null) return -1
      if (right == null) return 1
      return left - right
    })
    .slice(0, limit)
}

export type WorkbenchHomeScheduleKind = 'missing' | 'overdue' | 'soon'

export function workbenchHomeScheduleKind(
  scheduledAt: number | null | undefined,
  now = Date.now(),
): WorkbenchHomeScheduleKind {
  if (scheduledAt == null || !Number.isFinite(scheduledAt)) return 'missing'
  if (scheduledAt <= now) return 'overdue'
  return 'soon'
}

export type WorkbenchHomeRecentKind = 'note' | 'source' | 'media'

export interface WorkbenchHomeRecent {
  kind: WorkbenchHomeRecentKind
  id: number
  at: number
}

/** 桌上各记一件：刚动过的笔记、刚加上的订阅、刚上传的媒体。 */
export function workbenchHomeRecent(input: {
  notes: ReadonlyArray<{ id: number; updated_at: number }>
  skipNoteIds: ReadonlySet<number>
  sources: ReadonlyArray<{ id: number; created_at: number; source_type: string }>
  skipSourceIds?: ReadonlySet<number>
  media: ReadonlyArray<{ id: number; created_at: number }>
}): WorkbenchHomeRecent[] {
  const skipSources = input.skipSourceIds ?? new Set<number>()
  const items: WorkbenchHomeRecent[] = []
  const note = input.notes
    .filter((doc) => !input.skipNoteIds.has(doc.id))
    .toSorted((a, b) => b.updated_at - a.updated_at)[0]
  if (note) items.push({ kind: 'note', id: note.id, at: note.updated_at })

  const source = input.sources
    .filter(
      (item) => item.source_type !== 'note' && !skipSources.has(item.id),
    )
    .toSorted((a, b) => b.created_at - a.created_at)[0]
  if (source) items.push({ kind: 'source', id: source.id, at: source.created_at })

  const media = [...input.media].toSorted((a, b) => b.created_at - a.created_at)[0]
  if (media) items.push({ kind: 'media', id: media.id, at: media.created_at })

  return items.toSorted((a, b) => b.at - a.at)
}

export function workbenchHomeFailedNotes<
  T extends { last_error?: string | null },
>(docs: readonly T[]): T[] {
  return docs.filter((doc) => Boolean(doc.last_error))
}

export function workbenchHomeFailedSources<
  T extends {
    source_type: string
    error_count: number
    last_error: string | null
  },
>(
  sources: readonly T[],
  minErrors = WORKBENCH_HOME_FAIL_SOURCE_ERRORS,
): T[] {
  return sources.filter(
    (source) =>
      source.source_type !== 'note' &&
      !isSiteSource(source) &&
      source.error_count >= minErrors &&
      Boolean(source.last_error),
  )
}

export function workbenchHomeQuietFails<
  N extends { last_error?: string | null },
  S extends {
    source_type: string
    error_count: number
    last_error: string | null
  },
>(
  docs: readonly N[],
  sources: readonly S[],
  limit = WORKBENCH_HOME_QUIET,
): { notes: N[]; sources: S[] } {
  const notes = workbenchHomeFailedNotes(docs).slice(0, limit)
  const left = Math.max(0, limit - notes.length)
  return {
    notes,
    sources: workbenchHomeFailedSources(sources).slice(0, left),
  }
}
