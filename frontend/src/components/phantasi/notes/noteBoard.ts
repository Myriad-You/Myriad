/** 笔记板上只列还没公开的云端稿。空草稿不占格子。 */

import type { PhantasiItemPreview } from '../../../types/phantasi'

import type { FeedStory } from '../logic/feedStories'
import { haystackMatchesQuery } from '../logic/board'

/** 墙上已有笔记的源不再另占一张源卡。 */
export function leftoverNoteSources<T extends { id: number }>(
  sources: readonly T[],
  notes: ReadonlyArray<{ source_id: number }>,
): T[] {
  const taken = new Set(notes.map((note) => note.source_id))
  return sources.filter((source) => !taken.has(source.id))
}

export function sourceLatestStory(
  source: {
    id: number
    name: string
    icon: string | null
    source_type?: string
    recent_items?: readonly PhantasiItemPreview[] | null
  },
): (FeedStory & { source_id: number }) | null {
  const latest = source.recent_items?.[0]
  if (!latest) return null
  return {
    ...latest,
    source_id: source.id,
    source_name: source.name,
    source_icon: source.icon,
    source_type: source.source_type,
  }
}

type NoteWallSource = Parameters<typeof sourceLatestStory>[0]

/**
 * 搜索时笔记墙在按名称命中的源之外补回两类：有笔记命中的源；
 * 没有笔记、墙上显示其最新一篇的源——按那篇的标题/作者命中也算。
 */
export function noteWallSearchSources<T extends NoteWallSource>(
  named: T[],
  sources: readonly T[],
  wallSources: readonly T[],
  matchedNotes: ReadonlyArray<{ source_id: number }>,
  query: string,
): T[] {
  const have = new Set(named.map((source) => source.id))
  const noteHits = new Set(matchedNotes.map((note) => note.source_id))
  const storyHits = new Set<number>()
  if (query.trim()) {
    for (const source of wallSources) {
      const story = sourceLatestStory(source)
      if (story && haystackMatchesQuery(query, story.title, story.author))
        storyHits.add(source.id)
    }
  }
  const extra = sources.filter(
    (source) =>
      !have.has(source.id) &&
      (noteHits.has(source.id) || storyHits.has(source.id)),
  )
  return extra.length > 0 ? [...named, ...extra] : named
}

export function noteScheduleLabel(
  ms: number | null | undefined,
  locale: string,
): string {
  if (ms == null || !Number.isFinite(ms)) return ''
  return new Date(ms).toLocaleString(locale, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function noteDocHasBody(doc: {
  content_md?: string
  has_body?: boolean
}): boolean {
  return Boolean(doc.has_body) || (doc.content_md?.trim() ?? '') !== ''
}

export function visibleCloudNoteDocs<
  T extends { status: string; title: string; content_md?: string; has_body?: boolean },
>(docs: T[]): T[] {
  return docs.filter(
    (doc) =>
      doc.status !== 'published' &&
      (doc.title.trim() !== '' || noteDocHasBody(doc)),
  )
}

/** 笔记板空不空：云端稿也算。没有源、没有已发布、也没有云端稿才是空。 */
export function notesBoardIsEmpty(
  sourceCount: number,
  publishedCount: number,
  docs: Array<{
    status: string
    title: string
    content_md?: string
    has_body?: boolean
  }>,
): boolean {
  return (
    sourceCount === 0 &&
    publishedCount === 0 &&
    visibleCloudNoteDocs(docs).length === 0
  )
}

export function noteDocKicker(
  doc: {
    last_error?: string | null
    status: string
    scheduled_at?: number | null
  },
  copy: { failed: string; scheduled: string; draft: string },
  locale: string,
): string {
  const when = noteScheduleLabel(doc.scheduled_at, locale)
  if (doc.last_error) {
    return when ? `${copy.failed} · ${when}` : copy.failed
  }
  if (doc.status === 'scheduled') {
    return when ? `${copy.scheduled} · ${when}` : copy.scheduled
  }
  return copy.draft
}

export function noteEditorStatus(
  input: {
    lastError?: string | null
    status: string
    scheduledAt?: number | null
    savedHint?: boolean
  },
  copy: {
    failed: string
    scheduled: string
    published: string
    draft: string
    saved: string
  },
  locale: string,
): string {
  const when = noteScheduleLabel(input.scheduledAt, locale)
  const status = input.lastError
    ? copy.failed
    : input.status === 'scheduled'
      ? when
        ? `${copy.scheduled} · ${when}`
        : copy.scheduled
      : input.status === 'published'
        ? copy.published
        : copy.draft
  return input.savedHint ? `${status} · ${copy.saved}` : status
}
