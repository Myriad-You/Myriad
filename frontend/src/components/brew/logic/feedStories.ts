/** 不调 brewApi。 */

import type { BrewItem, BrewItemPreview } from '../../../types/brew'

export const FEEDS_ARTICLE_MAX = 20

export type FeedStory = BrewItemPreview & {
  author?: string | null
  source_name?: string | null
  source_icon?: string | null
}

export function toFeedStory(
  item: Pick<
    BrewItem,
    | 'id'
    | 'title'
    | 'summary'
    | 'image'
    | 'published_at'
    | 'is_read'
    | 'is_starred'
    | 'topic'
    | 'author'
    | 'source_name'
    | 'source_icon'
  >,
): FeedStory {
  return {
    id: item.id,
    title: item.title,
    summary: item.summary,
    image: item.image,
    published_at: item.published_at,
    is_read: item.is_read,
    is_starred: item.is_starred,
    topic: item.topic,
    author: item.author,
    source_name: item.source_name,
    source_icon: item.source_icon,
  }
}

export function latestStoryPreview(
  loose: readonly BrewItemPreview[] | null | undefined,
  recent: readonly BrewItemPreview[] | null | undefined,
): BrewItemPreview | undefined {
  return loose?.[0] ?? recent?.[0]
}

export function storiesForSource(
  fetched: { id: number; items: FeedStory[] } | null,
  source: {
    id: number
    name: string
    icon: string | null
    recent_items?: readonly BrewItemPreview[] | null
  } | null,
): FeedStory[] {
  if (!source) return []
  const base =
    fetched?.id === source.id && fetched.items.length > 0
      ? fetched.items
      : (source.recent_items ?? [])
  return base.map((item) => ({
    ...item,
    source_name: source.name,
    source_icon: source.icon,
  }))
}

export interface FeedStorySlot {
  stamp: number
  items: FeedStory[]
}

/** 换源先画能用的：精确戳 → 本会话槽 → 宽松缓存 → 旧槽。 */
export function paintReadyStories(
  stamp: number,
  exact: FeedStory[] | null,
  loose: FeedStory[] | null,
  slot?: FeedStorySlot,
): FeedStory[] | null {
  return (
    exact ??
    (slot?.stamp === stamp ? slot.items : null) ??
    loose ??
    slot?.items ??
    null
  )
}

export function storiesAreFresh(
  stamp: number,
  exact: FeedStory[] | null,
  slot?: FeedStorySlot,
): boolean {
  return exact != null || slot?.stamp === stamp
}
