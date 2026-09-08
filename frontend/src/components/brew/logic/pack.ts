/**
 * 磁贴墙的装箱与横向分页。
 *
 * 算法与 `ControlPanel/widgetReflow.ts` 同源（页内 first-fit、先填行再换行、
 * 不跨页），区别只有两点：
 * - 页数不封顶（Brew 要装下全部源，控制面板固定 3 页）
 * - 放不下不进 overflow：`packBrewCards` 保证不丢卡
 *
 * 坐标是渲染期的派生结果，**不入库**。拖拽只改 `sort_order`，位置由这里决定。
 */

import type { BrewSource } from '../../../types/brew'
import type { BrewTileSize } from './layout'
import type { BrewTopic } from './topics'

export interface GridSpan {
  w: number
  h: number
}

export type BrewCard =
  | { kind: 'source'; key: string; src: BrewSource; size: BrewTileSize }
  | { kind: 'topic'; key: string; topic: BrewTopic; size: BrewTileSize }

export type PackedCard = BrewCard & { x: number; y: number }

export interface PackOptions {
  /** 返回 true 时在 prev 与 next 之间强制翻页（`category` 排序用）。 */
  breakOn?: (prev: BrewCard, next: BrewCard) => boolean
}

/** 解析 `"4x2"`；无法解析返回 null。 */
export function parseTileSize(size: string): GridSpan | null {
  const m = /^(\d+)x(\d+)$/.exec(size)
  if (!m) return null
  const w = Number(m[1])
  const h = Number(m[2])
  if (w <= 0 || h <= 0) return null
  return { w, h }
}

/** 两张已装箱的卡是否相交 —— 供测试与 DEV 断言使用。 */
export function packedCardsOverlap(a: PackedCard, b: PackedCard): boolean {
  const sa = parseTileSize(a.size)
  const sb = parseTileSize(b.size)
  if (!sa || !sb) return false
  return (
    a.x < b.x + sb.w && b.x < a.x + sa.w && a.y < b.y + sb.h && b.y < a.y + sa.h
  )
}

function makePage(cols: number, rows: number): boolean[][] {
  return Array.from({ length: rows }, () =>
    Array.from({ length: cols }, () => false),
  )
}

function fits(
  page: boolean[][],
  x: number,
  y: number,
  span: GridSpan,
): boolean {
  for (let dy = 0; dy < span.h; dy++) {
    for (let dx = 0; dx < span.w; dx++) {
      if (page[y + dy][x + dx]) return false
    }
  }
  return true
}

function occupy(page: boolean[][], x: number, y: number, span: GridSpan): void {
  for (let dy = 0; dy < span.h; dy++) {
    for (let dx = 0; dx < span.w; dx++) {
      page[y + dy][x + dx] = true
    }
  }
}

/**
 * 按输入顺序把卡装进 `cols × rows` 的页面序列。
 *
 * - 页内 first-fit：先扫 y 再扫 x，所以是「填满一行再换行」
 * - 不跨页：横跨页边界的卡会被翻页滑动切开
 * - `breakOn` 命中时强制开新页
 * - **不丢卡**：超过一页容量的尺寸会被夹到网格内（理论上不该出现 ——
 *   `tileSize` 已按 band 降过档 —— 但宁可放歪也不静默吞掉一张卡）
 *
 * @returns 每页一个数组；输入非空时至少一页。
 */
export function packBrewCards(
  cards: readonly BrewCard[],
  cols: number,
  rows: number,
  opts?: PackOptions,
): PackedCard[][] {
  // 非法网格没有任何可落点，也就没有可渲染的页；调用方在 cols/rows 就绪前
  // 不该渲染磁贴墙（band 尚未测出时用缓存骨架）。
  if (cols <= 0 || rows <= 0) return []
  if (cards.length === 0) return []

  const pages: PackedCard[][] = []
  const grids: boolean[][][] = []
  const openPage = () => {
    pages.push([])
    grids.push(makePage(cols, rows))
    return pages.length - 1
  }

  const place = (p: number, span: GridSpan, card: BrewCard): boolean => {
    for (let y = 0; y + span.h <= rows; y++) {
      for (let x = 0; x + span.w <= cols; x++) {
        if (!fits(grids[p], x, y, span)) continue
        occupy(grids[p], x, y, span)
        pages[p].push({ ...card, x, y })
        return true
      }
    }
    return false
  }

  let current = openPage()
  let prev: BrewCard | null = null

  for (const card of cards) {
    const raw = parseTileSize(card.size)
    // 夹到网格内：宽于一页 / 高于一页的卡放不进任何页，夹住总比丢掉好
    const span: GridSpan = raw
      ? { w: Math.min(raw.w, cols), h: Math.min(raw.h, rows) }
      : { w: Math.min(2, cols), h: Math.min(2, rows) }

    if (prev && opts?.breakOn?.(prev, card)) {
      // 强制翻页：即使当前页还有空位也开新页（分类边界）
      if (pages[current].length > 0) current = openPage()
    }

    // 只看当前页：放不下就开新页，不回填更早的页。回填会让靠后的小卡
    // 跳到前面的空洞里，破坏阅读顺序，也会让键盘 j/k 的「装箱顺序」失真。
    if (!place(current, span, card)) {
      current = openPage()
      if (!place(current, span, card)) {
        // span 已夹到 ≤ cols × rows，空页上必定放得下；留个断言而不是静默丢卡
        throw new Error(
          `packBrewCards: ${card.key} (${card.size}) 放不进 ${cols}x${rows} 空页`,
        )
      }
    }

    prev = card
  }

  return pages
}

/** 装箱顺序展平（键盘 j/k 与入场 stagger 的 index 依据）。 */
export function flattenPackedCards(pages: PackedCard[][]): PackedCard[] {
  return pages.flat()
}
