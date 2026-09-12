import type { WidgetConfig, WidgetSize, WidgetType } from './widgetGridTypes'
import {
  freeLayoutFitsCellBudget,
  homeWidgetCellCount,
  homeWidgetsOccupiedCells,
  isHomeStickerItem,
  isHomeWidgetItem,
} from '../utils/homeLayout'
import { stickerSizesSharingAspect } from '../utils/homeStickerSize'
import { WIDGET_SIZE_KEYS, widgetSizeSpan } from '../utils/widgetSizeScale'
import { widgetPlacementCollides } from './widgetPlacementPreview'

export interface WidgetResizeSession {
  widgetId: string
  startPos: { x: number; y: number }
  startSize: WidgetSize
  draftSize: WidgetSize
  direction?: 'se' | 's'
}

export function resizeSupportedSizes(
  widget: WidgetConfig,
  startSize: WidgetSize,
  widgetType?: WidgetType,
): WidgetSize[] {
  if (isHomeStickerItem(widget)) {
    return Iterator.from(stickerSizesSharingAspect(startSize)).toArray()
  }
  const supported = widgetType?.supportedSizes ?? WIDGET_SIZE_KEYS
  return supported.filter((size) =>
    WIDGET_SIZE_KEYS.includes(size as (typeof WIDGET_SIZE_KEYS)[number]),
  )
}

export function nearestResizeSize(input: {
  currentSize: WidgetSize
  startSize: WidgetSize
  direction: 'se' | 's'
  rawW: number
  rawH: number
  supportedSizes: WidgetSize[]
}): WidgetSize {
  let bestSize = input.currentSize
  let minDistance = Infinity
  const startSpan = widgetSizeSpan(input.startSize)
  for (const size of input.supportedSizes) {
    const dim = widgetSizeSpan(size)
    if (input.direction === 's' && dim.w !== startSpan.w) continue
    const dist = (dim.w - input.rawW) ** 2 + (dim.h - input.rawH) ** 2
    if (dist < minDistance) {
      minDistance = dist
      bestSize = size
    }
  }
  return bestSize
}

export function resizeDraftAllowed(input: {
  widget: WidgetConfig
  draftSize: WidgetSize
  widgets: WidgetConfig[]
  gridWidth: number
  gridHeight: number
  isFreeLayout: boolean
}): boolean {
  const next = { ...input.widget, size: input.draftSize }
  const fitsBudget =
    !input.isFreeLayout ||
    !isHomeWidgetItem(input.widget) ||
    freeLayoutFitsCellBudget(
      homeWidgetsOccupiedCells(input.widgets, input.widget.id),
      homeWidgetCellCount(input.draftSize),
    )
  return (
    fitsBudget &&
    !widgetPlacementCollides(
      next,
      input.widgets,
      input.gridWidth,
      input.gridHeight,
      input.widget.id,
    )
  )
}

export function commitWidgetResize(
  widgets: WidgetConfig[],
  widgetId: string,
  draftSize: WidgetSize,
): WidgetConfig[] | null {
  const committed = widgets.find((widget) => widget.id === widgetId)
  if (!committed || committed.size === draftSize) return null
  return widgets.map((widget) =>
    widget.id === widgetId ? { ...widget, size: draftSize } : widget,
  )
}

export function resizeRawSpan(input: {
  pointer: { x: number; y: number }
  widget: WidgetConfig
  gridRect: { left: number; top: number; width: number; height: number }
  gridWidth: number
  gridHeight: number
  startSize: WidgetSize
  direction: 'se' | 's'
}): { rawW: number; rawH: number } {
  const cellWidth = input.gridRect.width / input.gridWidth
  const cellHeight = input.gridRect.height / input.gridHeight
  const widgetLeft = input.widget.position.x * cellWidth + input.gridRect.left
  const widgetTop = input.widget.position.y * cellHeight + input.gridRect.top
  const rawH = (input.pointer.y - widgetTop) / cellHeight
  const rawW =
    input.direction === 's'
      ? widgetSizeSpan(input.startSize).w
      : (input.pointer.x - widgetLeft) / cellWidth
  return { rawW, rawH }
}
