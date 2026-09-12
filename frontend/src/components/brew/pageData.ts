/** skin 不进口。换源不重打轨缓存；源变更时跟 sources 一起失效。 */

import type { BrewItemPreview } from '../../types/brew'
import type { FeedStory } from './logic/feedStories'
import type { HomeBoardNote } from './logic/homeBoard'
import * as brewApi from '../../services/brewApi'
import { requestCache } from '../../utils/requestCache'
import { BREW_MINE_CATEGORY } from './constants'
import {
  FEEDS_ARTICLE_MAX,
  latestStoryPreview,
  toFeedStory,
} from './logic/feedStories'
import {
  noteSourceKey,
  pickHomeBoardNotes,
} from './logic/homeBoard'

export const FEED_STORIES_CACHE_PREFIX = 'brew:feed-stories:'
export const HOME_NOTES_CACHE_PREFIX = 'brew:home-notes:'
const BOARD_PAGE_TTL = 60_000

export interface CachedFeedStories {
  stamp: number
  items: FeedStory[]
}

export function feedStoriesCacheKey(sourceId: number): string {
  return `${FEED_STORIES_CACHE_PREFIX}${sourceId}`
}

export function peekFeedStories(
  sourceId: number,
  stamp?: number | null,
): FeedStory[] | null {
  const hit = requestCache.get<CachedFeedStories>(feedStoriesCacheKey(sourceId))
  if (!hit) return null
  if (stamp != null && (stamp ?? 0) !== hit.stamp) return null
  return hit.items
}

export function peekLatestStory(source: {
  id: number
  recent_items?: readonly BrewItemPreview[] | null
}): BrewItemPreview | undefined {
  return latestStoryPreview(peekFeedStoriesLoose(source.id), source.recent_items)
}

export async function loadLatestStory(
  source: {
    id: number
    last_success_at?: number | null
    recent_items?: readonly BrewItemPreview[] | null
  },
  signal?: AbortSignal,
): Promise<BrewItemPreview | undefined> {
  const latest = peekLatestStory(source)
  if (latest) return latest
  const stories = await loadFeedStories(
    source.id,
    source.last_success_at ?? 0,
    signal,
  )
  return stories[0]
}

/** 不管抓取戳；换源先画上一轮，避免闪回预览。 */
export function peekFeedStoriesLoose(sourceId: number): FeedStory[] | null {
  return (
    requestCache.get<CachedFeedStories>(feedStoriesCacheKey(sourceId))?.items ??
    null
  )
}

export function putFeedStories(
  sourceId: number,
  stamp: number | null | undefined,
  items: FeedStory[],
): void {
  requestCache.set(
    feedStoriesCacheKey(sourceId),
    { stamp: stamp ?? 0, items },
    BOARD_PAGE_TTL,
  )
}

export async function loadHomeBoardNotes(
  sources: Array<{ id: number; source_type: string }>,
  signal?: AbortSignal,
): Promise<HomeBoardNote[]> {
  const key = noteSourceKey(sources)
  if (!key) return []
  const cacheKey = `${HOME_NOTES_CACHE_PREFIX}${key}`
  const load = async () => {
    const res = await brewApi.getItemPreviews(
      {
        category: BREW_MINE_CATEGORY,
        sort_order: 'desc',
        per_page: 8,
      },
      undefined,
      signal ? { signal } : undefined,
    )
    return pickHomeBoardNotes(res.items, sources)
  }
  if (signal) {
    const notes = await load()
    if (!signal.aborted) requestCache.set(cacheKey, notes, BOARD_PAGE_TTL)
    return notes
  }
  return requestCache.fetch(cacheKey, load, BOARD_PAGE_TTL)
}

export async function loadFeedStories(
  sourceId: number,
  stamp?: number | null,
  signal?: AbortSignal,
): Promise<FeedStory[]> {
  const normalized = stamp ?? 0
  const latest = requestCache.get<CachedFeedStories>(
    feedStoriesCacheKey(sourceId),
  )
  if (latest?.stamp === normalized) return latest.items

  const load = async () => {
    const res = await brewApi.getItemPreviews(
      {
        source_id: sourceId,
        sort_order: 'desc',
        per_page: FEEDS_ARTICLE_MAX,
      },
      undefined,
      signal ? { signal } : undefined,
    )
    const items = res.items.map(toFeedStory)
    if (!signal?.aborted) putFeedStories(sourceId, normalized, items)
    return items
  }
  if (signal) return load()
  return requestCache.fetch(
    `${feedStoriesCacheKey(sourceId)}:${normalized}`,
    load,
    BOARD_PAGE_TTL,
  )
}
