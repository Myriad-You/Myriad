/** `sites` 深链 id 不能改。「友情链接」是分类名；朋友源从订阅 inbox 拿走。手记源仍可出现在订阅墙。 */

import type { BrewSource } from '../../../types/brew'
import type { BrewViewerRole } from './score'
import {
  brewCategoryParts,
  brewMainCategory,
  isFriendLinkCategory,
  isOwnBrewSource,
} from '../constants'
import { compareByScore } from './score'

export type BrewBoard = 'feeds' | 'notes' | 'sites'

/** 收藏和主题流不是板块，是订阅上的筛选。 */
export type BrewViewMode = 'sources' | 'starred' | 'topic-feed'

export const BREW_BOARDS = ['feeds', 'notes', 'sites'] as const satisfies
  readonly BrewBoard[]

export function isBrewBoard(value: string): value is BrewBoard {
  return (BREW_BOARDS as readonly string[]).includes(value)
}

/** 入口型：不抓取、无未读、不进阅读器。 */
export function isSiteSource(s: Pick<BrewSource, 'source_type'>): boolean {
  return s.source_type === 'link'
}

export function refreshableSourceCount(
  sources: readonly Pick<BrewSource, 'source_type'>[],
): number {
  return sources.filter((source) => !isSiteSource(source)).length
}

export function isNotesSource(
  s: Pick<BrewSource, 'source_type' | 'category' | 'admin_only'>,
): boolean {
  return s.source_type === 'note' || isOwnBrewSource(s)
}

/** 自有源优先去手记，不进朋友们。 */
export function isFriendSource(
  s: Pick<BrewSource, 'source_type' | 'category' | 'admin_only'>,
): boolean {
  if (isNotesSource(s)) return false
  if (isSiteSource(s)) return true
  return brewCategoryParts(s.category).some(isFriendLinkCategory)
}

export function sourcesForBoard(
  sources: readonly BrewSource[],
  board: BrewBoard,
): BrewSource[] {
  if (board === 'sites') return sources.filter(isFriendSource)
  if (board === 'notes') return sources.filter(isNotesSource)
  return sources.filter((s) => !isFriendSource(s))
}

export function collectSourceCategories(
  sources: readonly Pick<BrewSource, 'category'>[],
): string[] {
  const cats = new Set<string>()
  for (const source of sources) {
    for (const part of brewCategoryParts(source.category)) cats.add(part)
  }
  return Iterator.from(cats).toArray()
}

export function filterSourcesByQuery(
  sources: readonly BrewSource[],
  query: string,
): BrewSource[] {
  const needle = query.trim().toLowerCase()
  if (!needle) return Iterator.from(sources).toArray()
  return sources.filter(
    (source) =>
      source.name.toLowerCase().includes(needle) ||
      source.url.toLowerCase().includes(needle) ||
      (source.description?.toLowerCase().includes(needle) ?? false),
  )
}

export type SourceSortMode = 'smart' | 'update' | 'category' | 'pinyin'

export function sortSourcesForBoard(
  sources: readonly BrewSource[],
  mode: SourceSortMode,
  role: BrewViewerRole,
  now: number,
  locale = 'en-US',
): BrewSource[] {
  switch (mode) {
    case 'smart':
      return sources.toSorted((a, b) => compareByScore(a, b, role, now))
    case 'update':
      return sources.toSorted((a, b) => {
        const latestA =
          a.recent_items?.[0]?.published_at || a.last_success_at || 0
        const latestB =
          b.recent_items?.[0]?.published_at || b.last_success_at || 0
        return latestB - latestA
      })
    case 'category':
      return sources.toSorted((a, b) => {
        const catA = brewMainCategory(a.category, '')
        const catB = brewMainCategory(b.category, '')
        if (catA !== catB) return catA.localeCompare(catB, locale)
        return a.name.localeCompare(b.name, locale)
      })
    case 'pinyin':
      return sources.toSorted((a, b) => a.name.localeCompare(b.name, locale))
    default:
      return Iterator.from(sources).toArray()
  }
}

/** 三个板块都进源墙；收藏仍是订阅上的筛选。 */
export type BrewBoardEntry =
  | { view: 'sources'; board: BrewBoard }
  | { view: 'starred'; board: 'feeds' }

export function boardEntry(board: BrewBoard): BrewBoardEntry {
  return { view: 'sources', board }
}

/** `?category=friends|mine|all|starred` 深链不能断。starred → 订阅板块 + 收藏视图。 */
const LEGACY_NAV_TO_BOARD: Record<string, BrewBoardEntry> = {
  all: { view: 'sources', board: 'feeds' },
  friends: { view: 'sources', board: 'sites' },
  mine: { view: 'sources', board: 'notes' },
  starred: { view: 'starred', board: 'feeds' },
}

/** `?board=` 优先；认不出的取值返回 null，不回落默认板块。 */
export function resolveBoardParam(value: string): BrewBoardEntry | null {
  if (isBrewBoard(value)) return boardEntry(value)
  if (value === 'friends') return boardEntry('sites')
  return LEGACY_NAV_TO_BOARD[value] ?? null
}

/** 游客没有收藏，深链落到板块本身。 */
export function viewForBoardEntry(
  entry: BrewBoardEntry,
  isAuthenticated: boolean,
): BrewViewMode {
  if (entry.view === 'starred' && !isAuthenticated) return 'sources'
  return entry.view
}

/** 落地后把 query 吃掉，刷新和后退不再触发一次。 */
export function eatSearchKeys(
  params: URLSearchParams,
  keys: readonly string[],
): URLSearchParams {
  const next = new URLSearchParams(params)
  for (const key of keys) next.delete(key)
  return next
}

/** 分页 items 只属于收藏和主题流。 */
export function filterLaneItems<T>(
  viewMode: BrewViewMode,
  items: T[],
  empty: T[],
): T[] {
  return viewMode === 'starred' || viewMode === 'topic-feed' ? items : empty
}

export function showsFilterLane(
  viewMode: BrewViewMode,
  hasTopic: boolean,
  isAuthenticated: boolean,
): boolean {
  return (
    (viewMode === 'topic-feed' && hasTopic) ||
    (viewMode === 'starred' && isAuthenticated)
  )
}
