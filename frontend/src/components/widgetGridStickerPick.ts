import type { WidgetConfig, WidgetSize } from './widgetGridTypes'
import { placeHomeStickerSelection } from '../utils/homeStickerSize'
import { widgetSizeSpan } from '../utils/widgetSizeScale'
import { widgetPlacementCollides } from './widgetPlacementPreview'

export interface StickerDragSpan {
  start: { x: number; y: number }
  end: { x: number; y: number }
}

export interface StickerSlotAnchor {
  top: number
  left: number
  width: number
  height: number
  right: number
  bottom: number
}

export function gridCellFromPointer(input: {
  point: { x: number; y: number }
  gridRect: { left: number; top: number; width: number; height: number }
  gridWidth: number
  gridHeight: number
}): { x: number; y: number } | null {
  const { gridRect, gridWidth, gridHeight } = input
  if (gridRect.width <= 0 || gridRect.height <= 0) return null
  return {
    x: Math.max(
      0,
      Math.min(
        gridWidth - 1,
        Math.floor(
          ((input.point.x - gridRect.left) / gridRect.width) * gridWidth,
        ),
      ),
    ),
    y: Math.max(
      0,
      Math.min(
        gridHeight - 1,
        Math.floor(
          ((input.point.y - gridRect.top) / gridRect.height) * gridHeight,
        ),
      ),
    ),
  }
}

export function stickerDragRect(span: StickerDragSpan): {
  x: number
  y: number
  w: number
  h: number
} {
  return {
    x: Math.min(span.start.x, span.end.x),
    y: Math.min(span.start.y, span.end.y),
    w: Math.abs(span.end.x - span.start.x) + 1,
    h: Math.abs(span.end.y - span.start.y) + 1,
  }
}

export function stickerPickCandidate(span: StickerDragSpan): WidgetConfig {
  const rect = stickerDragRect(span)
  const placed = placeHomeStickerSelection(rect.x, rect.y, rect.w, rect.h)
  return {
    id: '__sticker-pick__',
    type: 'sticker',
    kind: 'sticker',
    size: placed.size,
    position: { x: placed.x, y: placed.y },
  }
}

export function stickerPickCollides(
  span: StickerDragSpan,
  widgets: WidgetConfig[],
  gridWidth: number,
  gridHeight: number,
): boolean {
  return widgetPlacementCollides(
    stickerPickCandidate(span),
    widgets,
    gridWidth,
    gridHeight,
  )
}

export function homeSlotAnchor(
  grid: { left: number; top: number; width: number; height: number } | null,
  slot: { x: number; y: number },
  size: WidgetSize,
  gridWidth: number,
  gridHeight: number,
): StickerSlotAnchor {
  const dim = widgetSizeSpan(size)
  if (!grid) {
    return { left: 0, top: 0, width: 0, height: 0, right: 0, bottom: 0 }
  }
  return {
    left: grid.left + (slot.x / gridWidth) * grid.width,
    top: grid.top + (slot.y / gridHeight) * grid.height,
    width: (dim.w / gridWidth) * grid.width,
    height: (dim.h / gridHeight) * grid.height,
    right: grid.left + ((slot.x + dim.w) / gridWidth) * grid.width,
    bottom: grid.top + ((slot.y + dim.h) / gridHeight) * grid.height,
  }
}

export function sameGridCell(
  prev: { x: number; y: number } | null,
  next: { x: number; y: number },
): boolean {
  return Boolean(prev?.x === next.x && prev.y === next.y)
}
