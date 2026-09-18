/** 精选只收订阅墙里的文章，不收友链入口和笔记。 */

import type { PhantasiItemPreview, PhantasiSource } from '../../../types/phantasi'
import { DEFAULT_THEME_COLOR, normalizeThemeColor } from '../constants'
import { isFriendSource, sourcesForBoard } from '../logic/board'

export const FEATURED_PICK_LIMIT = 5
export const FEATURED_MAX_PER_SOURCE = 2

export interface FeaturedPick {
  id: number
  title: string
  summary: string | null
  image: string | null
  published_at: number | null
  is_read: boolean
  source_id: number
  source_name: string
  source_icon: string | null
  source_color: string
}

export function feedSourcesForFeatured(
  sources: readonly PhantasiSource[],
): PhantasiSource[] {
  return sourcesForBoard(sources, 'feeds').filter((source) => !isFriendSource(source))
}

function toPick(source: PhantasiSource, item: PhantasiItemPreview): FeaturedPick | null {
  const title = item.title?.trim()
  if (!title) return null
  return {
    id: item.id,
    title,
    summary: item.summary,
    image: item.image,
    published_at: item.published_at,
    is_read: item.is_read,
    source_id: source.id,
    source_name: source.name,
    source_icon: source.icon,
    source_color: normalizeThemeColor(source.theme_color, DEFAULT_THEME_COLOR),
  }
}

/** 按发布时间排，源最多占两席；封面只在已入选的集合里提前，不拿旧文顶新鲜标题。 */
export function collectFeaturedPicks(
  sources: readonly PhantasiSource[],
  limit = FEATURED_PICK_LIMIT,
): FeaturedPick[] {
  if (limit <= 0) return []

  const pool: FeaturedPick[] = []
  const seen = new Set<number>()
  for (const source of feedSourcesForFeatured(sources)) {
    for (const item of source.recent_items ?? []) {
      if (seen.has(item.id)) continue
      const pick = toPick(source, item)
      if (!pick) continue
      seen.add(item.id)
      pool.push(pick)
    }
  }

  const ranked = pool.toSorted(
    (a, b) => (b.published_at ?? 0) - (a.published_at ?? 0) || a.id - b.id,
  )

  const picked: FeaturedPick[] = []
  const overflow: FeaturedPick[] = []
  const perSource = new Map<number, number>()
  for (const item of ranked) {
    const used = perSource.get(item.source_id) ?? 0
    if (used >= FEATURED_MAX_PER_SOURCE) {
      overflow.push(item)
      continue
    }
    picked.push(item)
    perSource.set(item.source_id, used + 1)
    if (picked.length >= limit) break
  }
  if (picked.length < limit) {
    for (const item of overflow) {
      picked.push(item)
      if (picked.length >= limit) break
    }
  }

  if (picked.length > 1 && !picked[0]?.image) {
    const withCover = picked.findIndex((item) => item.image)
    if (withCover > 0) {
      const [hero] = picked.splice(withCover, 1)
      if (hero) picked.unshift(hero)
    }
  }

  return picked
}
