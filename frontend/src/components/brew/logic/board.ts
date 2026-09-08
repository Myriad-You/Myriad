/**
 * Brew 的三个板块：订阅 / 手记 / 站点。
 *
 * 板块划分的是「这批内容长什么样」，不是分类筛选：
 *
 * - `feeds` 订阅源磁贴墙。会更新、有未读、要刷新的来源。
 * - `notes` 「我」分类的合并文章流。站长自写的手记与自有外部源同在这里。
 * - `sites` 入口型来源的紧凑磁贴墙。友情链接与书签都在这里，不订阅。
 *
 * 划分依据是 `source_type`，不是 `category` —— 一个源可以同时挂「我」和别的
 * 分类，但它要么会更新要么不会。友情链接分类里若混进一个真订阅源，它仍属于
 * 订阅板块，因为它有未读要读。
 *
 * 「我」分类的订阅源同时出现在 feeds 与 notes：前者按源看更新，后者按文章看
 * 内容。这是有意的，不是漏了过滤。手记源（`source_type = note`）同样如此 ——
 * 它在订阅板块里是一张磁贴，点进去是站长自己写的那些文章。
 */

import type { BrewSource } from '../../../types/brew'

/** 板块 id。也是二级导航的 id 与 `?board=` 深链的取值。 */
export type BrewBoard = 'feeds' | 'notes' | 'sites'

export const BREW_BOARDS = ['feeds', 'notes', 'sites'] as const satisfies
  readonly BrewBoard[]

export function isBrewBoard(value: string): value is BrewBoard {
  return (BREW_BOARDS as readonly string[]).includes(value)
}

/**
 * 入口型来源：只提供一个外站入口，不抓取、无未读、不进阅读器。
 *
 * 与 `layout.ts` 里 `icon` 构图的判据是同一个 —— 那边决定长什么样，
 * 这边决定去哪个板块，两处都只看 `source_type === 'link'`。
 */
export function isSiteSource(s: Pick<BrewSource, 'source_type'>): boolean {
  return s.source_type === 'link'
}

/**
 * 磁贴墙板块的源过滤。
 *
 * `notes` 不是源墙而是文章流，传进来返回空数组 —— 调用方不该拿它渲染磁贴墙，
 * 但静默返回空比抛异常更适合渲染路径。
 */
export function sourcesForBoard(
  sources: readonly BrewSource[],
  board: BrewBoard,
): BrewSource[] {
  if (board === 'sites') return sources.filter(isSiteSource)
  if (board === 'feeds') return sources.filter((s) => !isSiteSource(s))
  return []
}

/** 板块进入时的落点视图。`notes` 直接进合并文章流，另外两个是源墙。 */
export type BrewBoardEntry =
  | { view: 'sources'; board: BrewBoard }
  | { view: 'category-feed'; board: 'notes' }
  | { view: 'starred'; board: 'feeds' }

export function boardEntry(board: BrewBoard): BrewBoardEntry {
  if (board === 'notes') return { view: 'category-feed', board: 'notes' }
  return { view: 'sources', board }
}

/**
 * 旧二级导航 id → 新板块。
 *
 * `?category=friends|mine|all|starred` 这四个深链在外面已经存在（站内导航、
 * Agent 的跳转、可能的外链），改板块不能把它们打断。收藏不再是板块，映射成
 * 「订阅板块 + 打开收藏视图」。
 */
const LEGACY_NAV_TO_BOARD: Record<string, BrewBoardEntry> = {
  all: { view: 'sources', board: 'feeds' },
  friends: { view: 'sources', board: 'sites' },
  mine: { view: 'category-feed', board: 'notes' },
  starred: { view: 'starred', board: 'feeds' },
}

/**
 * 解析 `?board=` / `?category=` 的取值。新参数优先，旧参数走别名表。
 * 认不出的取值返回 null，调用方保持当前状态而不是回落到默认板块。
 */
export function resolveBoardParam(value: string): BrewBoardEntry | null {
  if (isBrewBoard(value)) return boardEntry(value)
  return LEGACY_NAV_TO_BOARD[value] ?? null
}
