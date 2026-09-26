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

const canvasCardRevealedIds = new Set<string>()
const MAX_CANVAS_CARD_REVEALED = 500

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

  return candidates.toSorted(
    (a, b) =>
      (spatialIndex.order.get(a.id) ?? 0) - (spatialIndex.order.get(b.id) ?? 0),
  )
}

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
  const grouped = Object.groupBy(items, (item) => item.item_type)
  const groups: Record<string, LibraryItem[]> = {
    game: grouped.game ?? [],
    video: grouped.video ?? [],
    music: grouped.music ?? [],
    anime: grouped.anime ?? [],
    tv_series: grouped.tv_series ?? [],
    book: grouped.book ?? [],
  }
  for (const type of Object.keys(groups)) {
    groups[type] = groups[type].toSorted(() => Math.random() - 0.5)
  }

  const result: LibraryItem[] = []
  const maxLength = Math.max(
    0,
    ...Object.values(groups).map((group) => group.length),
  )
  for (let index = 0; index < maxLength; index++) {
    Object.keys(groups)
      .toSorted(() => Math.random() - 0.5)
      .forEach((type) => {
        const item = groups[type][index]
        if (item) result.push(item)
      })
  }
  return result
}

export function computeLibraryListLayout(
  items: LibraryItem[],
  containerWidth: number,
  getGridSize: (item: LibraryItem) => { w: number; h: number },
): Map<string, LibraryCanvasLayout> {
  const gap = 16
  let columns: number

  if (containerWidth < 640) columns = 2
  else if (containerWidth < 768) columns = 3
  else if (containerWidth < 1024) columns = 4
  else if (containerWidth < 1536) columns = 5
  else columns = 6

  const baseWidth = (containerWidth - gap * (columns + 1)) / columns
  const uniformHeight = baseWidth

  let startOffset = 0
  let layoutColumns = columns

  const allWideCards =
    items.length > 0 && items.every((item) => getGridSize(item).w === 2)
  if (allWideCards && columns % 2 !== 0 && columns > 1) {
    layoutColumns = columns - 1
    startOffset = (baseWidth + gap) / 2
  }

  const queues: Record<string, { item: LibraryItem; originalIndex: number }[]> =
    {
      '1x1': [],
      '1x2': [],
      '2x1': [],
    }

  items.forEach((item, index) => {
    const size = getGridSize(item)
    const key = `${size.w}x${size.h}`
    if (queues[key]) {
      queues[key].push({ item, originalIndex: index })
    } else {
      queues['1x1'].push({ item, originalIndex: index })
    }
  })

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

  let y = 0
  while (placedCount < totalItems) {
    for (let x = 0; x < layoutColumns; x++) {
      if (isOccupied(x, y)) continue

      const candidates: {
        type: string
        index: number
        item: LibraryItem
        w: number
        h: number
      }[] = []

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
        // 装不下就标记占用跳过，否则下次循环还是空格。
        continue
      }

      const best = candidates.toSorted((a, b) => a.index - b.index)[0]

      const queue = queues[best.type as keyof typeof queues]
      queue.shift()

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

      const itemBottom = top + height
      if (itemBottom > maxY) maxY = itemBottom
    }

    y++

    if (y > totalItems * 2) break
  }

  return newLayouts
}
