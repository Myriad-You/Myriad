/** 不产出 WidgetGrid 坐标。 */

import type { BrewItem } from '../../../types/brew'

export const NOTES_FEATURED_MAX = 3

export interface HomeBoardNote {
  id: number
  title: string
  summary: string | null
  image: string | null
  published_at: number | null
  source_id: number
}

export function toHomeBoardNote(
  item: Pick<
    BrewItem,
    'id' | 'title' | 'summary' | 'image' | 'published_at' | 'source_id'
  >,
): HomeBoardNote {
  return {
    id: item.id,
    title: item.title,
    summary: item.summary,
    image: item.image,
    published_at: item.published_at,
    source_id: item.source_id,
  }
}

export function pickHomeBoardNotes(
  items: Array<
    Pick<
      BrewItem,
      'id' | 'title' | 'summary' | 'image' | 'published_at' | 'source_id'
    >
  >,
  sources: Array<{ id: number; source_type: string }>,
): HomeBoardNote[] {
  const noteSourceIds = new Set(
    sources
      .filter((source) => source.source_type === 'note')
      .map((source) => source.id),
  )
  if (noteSourceIds.size === 0) return []
  return Iterator.from(items)
    .filter((item) => noteSourceIds.has(item.source_id))
    .take(NOTES_FEATURED_MAX)
    .map(toHomeBoardNote)
    .toArray()
}

export function noteSourceKey(
  sources: Array<{ id: number; source_type: string }>,
): string {
  return sources
    .filter((source) => source.source_type === 'note')
    .map((source) => source.id)
    .join(',')
}
