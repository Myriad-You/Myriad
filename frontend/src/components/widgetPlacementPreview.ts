/**
 * Library → grid placement: the ghost is the catalog preview.
 * On drop the live tile mounts and starts loading. After the preview
 * lands, the live tile uncovers under it, then the preview dissolves.
 * The live tile is not a preview.
 */

import type { WidgetConfig, WidgetType } from './widgetGridTypes'
import { GRID_WIDGET_PAD_PX, widgetSizeSpan } from '../utils/widgetSizeScale'
import { widgetPreviewConfig } from './widgetLibraryModel'

export type WidgetDragKind = 'existing' | 'new'

export interface WidgetDragSession {
  type: WidgetDragKind
  widgetId?: string
  widgetTypeId?: string
  /** Tile already notified to the parent; ghost stays until that cell commits. */
  pendingId?: string
  pendingCell?: { x: number; y: number }
}

export function shouldSkipWidgetEntrance(isEditMode: boolean): boolean {
  return isEditMode
}

export function gridCellFromPoint(input: {
  point: { x: number; y: number }
  gridRect: { left: number; top: number; width: number; height: number }
  gridWidth: number
  gridHeight: number
  size: { w: number; h: number }
}): { x: number; y: number } {
  const cellWidth = input.gridRect.width / input.gridWidth
  const cellHeight = input.gridRect.height / input.gridHeight
  const mouseX =
    input.point.x - input.gridRect.left - (input.size.w * cellWidth) / 2
  const mouseY =
    input.point.y - input.gridRect.top - (input.size.h * cellHeight) / 2
  return {
    x: Math.max(
      0,
      Math.min(input.gridWidth - input.size.w, Math.floor(mouseX / cellWidth)),
    ),
    y: Math.max(
      0,
      Math.min(input.gridHeight - input.size.h, Math.floor(mouseY / cellHeight)),
    ),
  }
}

export const DRAG_GHOST_SETTLE_MS = 240
export const DRAG_GHOST_SIT_MS = 90
export const DRAG_GHOST_EXIT_MS = 260

export function dragGhostSettleMs(reducedMotion: boolean): number {
  return reducedMotion ? 0 : DRAG_GHOST_SETTLE_MS
}

export function dragGhostSitMs(reducedMotion: boolean): number {
  return reducedMotion ? 0 : DRAG_GHOST_SIT_MS
}

export function dragGhostExitMs(reducedMotion: boolean): number {
  return reducedMotion ? 0 : DRAG_GHOST_EXIT_MS
}

export function dragGhostSettleWaitMs(
  reducedMotion: boolean,
  elapsedMs: number,
): number {
  return Math.max(0, dragGhostSettleMs(reducedMotion) - Math.max(0, elapsedMs))
}

export function dragGhostHandoffDelays(
  reducedMotion: boolean,
  elapsedMs: number,
): { uncoverMs: number; exitMs: number; clearMs: number } {
  const uncoverMs = dragGhostSettleWaitMs(reducedMotion, elapsedMs)
  const exitMs = uncoverMs + dragGhostSitMs(reducedMotion)
  return {
    uncoverMs,
    exitMs,
    clearMs: exitMs + dragGhostExitMs(reducedMotion),
  }
}

export function dragGhostContentSize(
  cellWidth: number,
  cellHeight: number,
  span: { w: number; h: number },
  padPx = GRID_WIDGET_PAD_PX,
): { width: number; height: number } {
  return {
    width: Math.max(0, span.w * cellWidth - padPx * 2),
    height: Math.max(0, span.h * cellHeight - padPx * 2),
  }
}

export function widgetDragGhostBox(input: {
  gridRect: { left: number; top: number; width: number; height: number }
  cell: { x: number; y: number }
  size: { w: number; h: number }
  gridWidth: number
  gridHeight: number
  padPx?: number
}): { x: number; y: number; width: number; height: number } {
  const pad = input.padPx ?? GRID_WIDGET_PAD_PX
  const cellWidth = input.gridRect.width / input.gridWidth
  const cellHeight = input.gridRect.height / input.gridHeight
  const { width, height } = dragGhostContentSize(
    cellWidth,
    cellHeight,
    input.size,
    pad,
  )
  return {
    x: input.gridRect.left + input.cell.x * cellWidth + pad + width / 2,
    y: input.gridRect.top + input.cell.y * cellHeight + pad + height / 2,
    width,
    height,
  }
}

export function widgetDragGhostAnchor(
  gridRect: { left: number; top: number; width: number; height: number },
  cell: { x: number; y: number },
  size: { w: number; h: number },
  gridWidth: number,
  gridHeight: number,
): { x: number; y: number } {
  const box = widgetDragGhostBox({
    gridRect,
    cell,
    size,
    gridWidth,
    gridHeight,
  })
  return { x: box.x, y: box.y }
}

export function resolveDragGhostWidget(input: {
  dragged: WidgetDragSession
  widgets: WidgetConfig[]
  widgetTypeById: Map<string, WidgetType>
}): {
  widgetType?: WidgetType
  widgetConfig?: WidgetConfig
  size: { w: number; h: number }
  fromLibrary: boolean
} | null {
  if (input.dragged.type === 'existing' && input.dragged.widgetId) {
    const widget = input.widgets.find((item) => item.id === input.dragged.widgetId)
    if (!widget) return null
    return {
      widgetType: input.widgetTypeById.get(widget.type),
      widgetConfig: widget,
      size: widgetSizeSpan(widget.size),
      fromLibrary: false,
    }
  }
  if (input.dragged.type === 'new' && input.dragged.widgetTypeId) {
    const widgetType = input.widgetTypeById.get(input.dragged.widgetTypeId)
    if (!widgetType) return null
    return {
      widgetType,
      widgetConfig: widgetPreviewConfig(widgetType),
      size: widgetSizeSpan(widgetType.defaultSize),
      fromLibrary: true,
    }
  }
  return null
}

export function heldWidgetId(input: {
  dragged: WidgetDragSession | null
  settling: boolean
  uncovered?: boolean
}): string | undefined {
  if (!input.dragged || input.uncovered) return undefined
  if (input.settling) return input.dragged.pendingId
  if (input.dragged.type === 'existing') return input.dragged.widgetId
  return undefined
}

export function coveringWidgetId(input: {
  dragged: WidgetDragSession | null
  settling: boolean
}): string | undefined {
  if (!input.dragged || !input.settling) return undefined
  return input.dragged.pendingId
}

export function placementHasCommitted(
  widgets: WidgetConfig[],
  dragged: WidgetDragSession | null,
): boolean {
  const id = dragged?.pendingId
  const cell = dragged?.pendingCell
  if (!id || !cell) return false
  const found = widgets.find((widget) => widget.id === id)
  return Boolean(
    found && found.position.x === cell.x && found.position.y === cell.y,
  )
}
