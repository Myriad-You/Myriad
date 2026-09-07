import type {
  LibraryCanvasLayout,
  LibraryCanvasTransform,
} from '../../utils/libraryCanvas'
import {
  getLibraryCanvasViewportBounds,
  LIBRARY_CANVAS_STRIDE,
  libraryCanvasLayoutIntersects,
} from '../../utils/libraryCanvas'
import { isMobileNavLayout } from '../../utils/navLayout'

export interface LibraryItem {
  id: string
  item_type: 'game' | 'video' | 'music' | 'anime' | 'tv_series' | 'book'
  title: string
  cover: string | null
  platform: string
  metadata: any
}

export const CANVAS_STRIDE = LIBRARY_CANVAS_STRIDE
/** Desktop default zoom; mobile uses a tighter fit so cards aren't huge on first open. */
export const CANVAS_DEFAULT_SCALE_DESKTOP = 0.75
export const CANVAS_DEFAULT_SCALE_MOBILE = 0.5
export const CANVAS_MIN_SCALE = 0.45
export const CANVAS_MAX_SCALE = 1.6
export const CANVAS_SPATIAL_BIN_SIZE = CANVAS_STRIDE * 4
export const LIBRARY_PAGE_SIZE = 120

export interface LibraryCanvasSpatialIndex {
  bins: Map<string, LibraryItem[]>
  order: Map<string, number>
}

/**
 * Session-scoped: canvas cards that already played (or silently claimed) their
 * first-reveal enter. Prevents virtualization remounts from replaying motion.
 */
const canvasCardRevealedIds = new Set<string>()
const MAX_CANVAS_CARD_REVEALED = 500

/**
 * Claim first-reveal for a canvas card id.
 * Returns enter delay seconds when the shell should animate; null to mount quiet
 * (already seen, or surface is mid-drag).
 */
export function claimCanvasCardEnter(
  itemId: string,
  itemIndex: number,
  surfaceDragging: boolean,
): number | null {
  if (canvasCardRevealedIds.has(itemId)) return null
  canvasCardRevealedIds.add(itemId)
  while (canvasCardRevealedIds.size > MAX_CANVAS_CARD_REVEALED) {
    const oldest = canvasCardRevealedIds.values().next().value
    if (oldest === undefined) break
    canvasCardRevealedIds.delete(oldest)
  }
  if (surfaceDragging) return null
  return Math.min((itemIndex % 12) * 0.055, 0.6)
}

/** Visible-card query used by both React virtualization and live paint. */
export function queryCanvasVisibleItems(
  transform: LibraryCanvasTransform,
  viewport: { width: number; height: number },
  layouts: Map<string, LibraryCanvasLayout>,
  spatialIndex: LibraryCanvasSpatialIndex,
  laidOutItems: LibraryItem[],
): LibraryItem[] {
  if (layouts.size === 0) return []
  if (viewport.width === 0 || viewport.height === 0) {
    return laidOutItems.slice(0, 30)
  }
  const { minX, maxX, minY, maxY } = getLibraryCanvasViewportBounds(
    transform,
    viewport,
  )
  const minBinX = Math.floor(minX / CANVAS_SPATIAL_BIN_SIZE)
  const maxBinX = Math.floor(maxX / CANVAS_SPATIAL_BIN_SIZE)
  const minBinY = Math.floor(minY / CANVAS_SPATIAL_BIN_SIZE)
  const maxBinY = Math.floor(maxY / CANVAS_SPATIAL_BIN_SIZE)
  const candidates: LibraryItem[] = []
  const seen = new Set<string>()

  for (let binX = minBinX; binX <= maxBinX; binX++) {
    for (let binY = minBinY; binY <= maxBinY; binY++) {
      const bin = spatialIndex.bins.get(`${binX},${binY}`)
      if (!bin) continue
      bin.forEach((item) => {
        if (seen.has(item.id)) return
        seen.add(item.id)
        const layout = layouts.get(item.id)
        if (
          layout &&
          libraryCanvasLayoutIntersects(layout, { minX, maxX, minY, maxY })
        ) {
          candidates.push(item)
        }
      })
    }
  }

  return candidates.sort(
    (a, b) =>
      (spatialIndex.order.get(a.id) ?? 0) - (spatialIndex.order.get(b.id) ?? 0),
  )
}

/** 教程只钉一张卡：列表取第一张，画布取距世界原点最近的可见卡。 */
export function pickLibraryTourCardId(
  items: readonly { id: string }[],
  layouts?: ReadonlyMap<
    string,
    Pick<LibraryCanvasLayout, 'left' | 'top' | 'width' | 'height'>
  >,
): string | null {
  if (items.length === 0) return null
  if (!layouts || layouts.size === 0) return items[0]!.id
  let bestId: string | null = null
  let bestDist = Infinity
  for (const item of items) {
    const layout = layouts.get(item.id)
    if (!layout) continue
    const cx = layout.left + layout.width / 2
    const cy = layout.top + layout.height / 2
    const dist = cx * cx + cy * cy
    if (dist < bestDist) {
      bestDist = dist
      bestId = item.id
    }
  }
  return bestId ?? items[0]!.id
}

/** Keep the tour card mounted when virtualization would drop it. */
export function pinLibraryTourCard<T extends { id: string }>(
  visible: readonly T[],
  laidOut: readonly T[],
  tourCardId: string | null,
): T[] {
  if (!tourCardId || visible.some((item) => item.id === tourCardId)) {
    return visible as T[]
  }
  const pinned = laidOut.find((item) => item.id === tourCardId)
  return pinned ? [...visible, pinned] : (visible as T[])
}

export function readCanvasDefaultScale(): number {
  // Align with nav island: touch tablets in 768–1023 use compact chrome too.
  if (typeof window === 'undefined') return CANVAS_DEFAULT_SCALE_DESKTOP
  try {
    return isMobileNavLayout()
      ? CANVAS_DEFAULT_SCALE_MOBILE
      : CANVAS_DEFAULT_SCALE_DESKTOP
  } catch {
    return CANVAS_DEFAULT_SCALE_DESKTOP
  }
}

export function balancedShuffleLibraryItems(
  items: LibraryItem[],
): LibraryItem[] {
  const groups: Record<string, LibraryItem[]> = {
    game: [],
    video: [],
    music: [],
    anime: [],
    tv_series: [],
    book: [],
  }
  items.forEach((item) => groups[item.item_type]?.push(item))
  Object.values(groups).forEach((group) =>
    group.sort(() => Math.random() - 0.5),
  )

  const result: LibraryItem[] = []
  const maxLength = Math.max(
    0,
    ...Object.values(groups).map((group) => group.length),
  )
  for (let index = 0; index < maxLength; index++) {
    Object.keys(groups)
      .sort(() => Math.random() - 0.5)
      .forEach((type) => {
        const item = groups[type][index]
        if (item) result.push(item)
      })
  }
  return result
}

/** Pack list-mode cards into a gapless grid for the current container width. */
export function computeLibraryListLayout(
  items: LibraryItem[],
  containerWidth: number,
  getGridSize: (item: LibraryItem) => { w: number; h: number },
): Map<string, LibraryCanvasLayout> {
  const gap = 16
  let columns = 5

  // 响应式列数
  if (containerWidth < 640) columns = 2
  else if (containerWidth < 768) columns = 3
  else if (containerWidth < 1024) columns = 4
  else if (containerWidth < 1536) columns = 5
  else columns = 6

  const baseWidth = (containerWidth - gap * (columns + 1)) / columns
  const uniformHeight = baseWidth

  // 居中逻辑修正：针对纯宽卡片（2x1）在奇数列数下的居中处理
  let startOffset = 0
  let layoutColumns = columns

  // 如果当前筛选下只有宽2的卡片（如纯 Steam 游戏或视频分类），且列数是奇数
  // 那么最后一列无法被填满（因为没有宽1的卡片），导致整体偏左
  // 需要计算偏移量使内容居中
  // 注意：Bangumi 游戏为竖版（宽1），与 Steam 游戏混排时不应触发此居中
  const allWideCards =
    items.length > 0 && items.every((item) => getGridSize(item).w === 2)
  if (allWideCards && columns % 2 !== 0 && columns > 1) {
    layoutColumns = columns - 1
    // 剩余空间 = 1个列宽 + 1个间隙
    // 偏移量 = 剩余空间 / 2
    startOffset = (baseWidth + gap) / 2
  }

  // 1. 准备队列：按尺寸分类，保持原始相对顺序
  const queues: Record<string, { item: LibraryItem; originalIndex: number }[]> =
    {
      '1x1': [],
      '1x2': [],
      '2x1': [],
      // '2x2': [] // 暂无2x2类型
    }

  items.forEach((item, index) => {
    const size = getGridSize(item)
    const key = `${size.w}x${size.h}`
    if (queues[key]) {
      queues[key].push({ item, originalIndex: index })
    } else {
      // 默认归为 1x1
      queues['1x1'].push({ item, originalIndex: index })
    }
  })

  // 2. 网格状态追踪
  // 使用 Map 记录被占用的格子 "x,y" -> true
  const occupied = new Set<string>()
  const isOccupied = (x: number, y: number) => occupied.has(`${x},${y}`)
  const markOccupied = (x: number, y: number, w: number, h: number) => {
    for (let i = 0; i < w; i++) {
      for (let j = 0; j < h; j++) {
        occupied.add(`${x + i},${y + j}`)
      }
    }
  }

  const newLayouts = new Map<string, LibraryCanvasLayout>()
  let maxY = 0
  let placedCount = 0
  const totalItems = items.length

  // 3. 遍历网格填充
  // y 从 0 开始无限增长，x 从 0 到 columns-1
  let y = 0
  while (placedCount < totalItems) {
    for (let x = 0; x < layoutColumns; x++) {
      if (isOccupied(x, y)) continue

      // 发现空位 (x, y)
      // 尝试寻找最佳匹配项
      // 优先级：
      // 1. 检查是否能放入 2x1 (需要 x+1 空闲)
      // 2. 检查是否能放入 1x2 (需要 y+1 空闲 - 总是假设 y+1 空闲，除非有预占，但这里我们是逐行扫描，y+1通常未处理)
      // 注意：如果之前有 1x2 占据了 (x, y+1)，则 isOccupied(x, y+1) 会为 true。
      // 3. 放入 1x1

      // 为了保持"平均开始排布"，我们在所有能放入的候选中，选择 originalIndex 最小的那个

      const candidates: {
        type: string
        index: number
        item: LibraryItem
        w: number
        h: number
      }[] = []

      // 检查 1x1
      if (queues['1x1'].length > 0) {
        const qItem = queues['1x1'][0]
        candidates.push({
          item: qItem.item,
          index: qItem.originalIndex,
          type: '1x1',
          w: 1,
          h: 1,
        })
      }

      // 检查 2x1
      const canFit2x1 = x + 1 < layoutColumns && !isOccupied(x + 1, y)
      if (canFit2x1 && queues['2x1'].length > 0) {
        const qItem = queues['2x1'][0]
        candidates.push({
          item: qItem.item,
          index: qItem.originalIndex,
          type: '2x1',
          w: 2,
          h: 1,
        })
      }

      // 检查 1x2
      // 垂直方向通常是无限的，但要检查是否被上方的某些长条物体阻挡？
      // 我们是按 y 递增扫描，所以 (x, y+1) 只有可能被之前的操作占据（不太可能，除非有复杂形状）
      // 但为了严谨，检查一下
      const canFit1x2 = !isOccupied(x, y + 1)
      if (canFit1x2 && queues['1x2'].length > 0) {
        const qItem = queues['1x2'][0]
        candidates.push({
          item: qItem.item,
          index: qItem.originalIndex,
          type: '1x2',
          w: 1,
          h: 2,
        })
      }

      if (candidates.length === 0) {
        // 没有剩余物品能放入此格
        // 只能留空 (虽然用户说避免空白，但如果没有物品了就没办法)
        // 或者：如果只有 2x1 且当前只有 1格宽，那必须留空
        // 标记此格为"跳过/虚拟占用"以继续循环?
        // 不，直接 continue，外层循环会处理下一个 x
        // 但如果不标记，下次循环回来还是空的。
        // 所以必须标记为"废弃"
        // 但如果后续还有物品，只是当前放不下（比如只有2x1但这里只有1格），那这个格子就真的废了
        // 除非我们能从后面拉一个 1x1 过来。但如果 1x1 队列空了，那就真没办法。
        // 标记为占用，但不放置物品
        // occupied.add(`${x},${y}`); // 实际上不需要显式add，只要不处理就行，但为了算法推进，视为已处理
        continue
      }

      // 选择 originalIndex 最小的候选者
      candidates.sort((a, b) => a.index - b.index)
      const best = candidates[0]

      // 放置物品
      const queue = queues[best.type as keyof typeof queues]
      queue.shift() // 移除已使用的

      // 计算像素位置
      const left = gap + x * (baseWidth + gap) + startOffset
      const top = gap + y * (uniformHeight + gap)
      const width = best.w * baseWidth + (best.w - 1) * gap
      const height = best.h * uniformHeight + (best.h - 1) * gap

      newLayouts.set(best.item.id, {
        left,
        top,
        width,
        height,
        gridX: x,
        gridY: y,
        gridW: best.w,
        gridH: best.h,
      })

      markOccupied(x, y, best.w, best.h)
      placedCount++

      // 更新最大高度
      const itemBottom = top + height
      if (itemBottom > maxY) maxY = itemBottom
    }

    // 检查当前行是否还有未处理的空位（被跳过的）
    // 如果所有列都处理过（占用或尝试过），进入下一行
    y++

    // 安全阀：防止死循环 (如果数据异常)
    if (y > totalItems * 2) break
  }

  return newLayouts
}
