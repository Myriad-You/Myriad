import type { MouseEvent as ReactMouseEvent, RefObject } from 'react'
import type { StickerDragSpan } from './widgetGridStickerPick'
import type { WidgetConfig, WidgetSize } from './widgetGridTypes'
import { useEffect, useState } from 'react'
import { placeHomeStickerSelection } from '../utils/homeStickerSize'
import {
  gridCellFromPointer,
  homeSlotAnchor,
  sameGridCell,

  stickerPickCollides,
  stickerDragRect as stickerRectOf,
} from './widgetGridStickerPick'

export function useWidgetGridStickerPick(input: {
  widgets: WidgetConfig[]
  currentGridWidth: number
  currentGridHeight: number
  gridRectRef: RefObject<DOMRect | null>
  onPickStickerSlot?: (slot: {
    x: number
    y: number
    size: WidgetSize
    anchor: {
      top: number
      left: number
      width: number
      height: number
      right: number
      bottom: number
    }
  }) => void
}) {
  const [stickerDrag, setStickerDrag] = useState<StickerDragSpan | null>(null)
  const [stickerHover, setStickerHover] = useState<{
    x: number
    y: number
  } | null>(null)

  useEffect(() => {
    if (!stickerDrag) return
    const onMove = (event: MouseEvent) => {
      const rect = input.gridRectRef.current
      if (!rect) return
      const cell = gridCellFromPointer({
        point: { x: event.clientX, y: event.clientY },
        gridRect: rect,
        gridWidth: input.currentGridWidth,
        gridHeight: input.currentGridHeight,
      })
      if (!cell) return
      setStickerDrag((prev) =>
        prev && (prev.end.x !== cell.x || prev.end.y !== cell.y)
          ? { ...prev, end: cell }
          : prev,
      )
    }
    const onUp = () => {
      setStickerDrag((prev) => {
        if (!prev) return null
        if (
          input.onPickStickerSlot &&
          !stickerPickCollides(
            prev,
            input.widgets,
            input.currentGridWidth,
            input.currentGridHeight,
          )
        ) {
          const placed = placeHomeStickerSelection(
            Math.min(prev.start.x, prev.end.x),
            Math.min(prev.start.y, prev.end.y),
            Math.abs(prev.end.x - prev.start.x) + 1,
            Math.abs(prev.end.y - prev.start.y) + 1,
          )
          input.onPickStickerSlot({
            x: placed.x,
            y: placed.y,
            size: placed.size,
            anchor: homeSlotAnchor(
              input.gridRectRef.current,
              { x: placed.x, y: placed.y },
              placed.size,
              input.currentGridWidth,
              input.currentGridHeight,
            ),
          })
        }
        return null
      })
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
    return () => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
  }, [
    input.currentGridHeight,
    input.currentGridWidth,
    input.gridRectRef,
    input.onPickStickerSlot,
    input.widgets,
    stickerDrag,
  ])

  const pointerCell = (event: { clientX: number; clientY: number }) => {
    const nodeRect = input.gridRectRef.current
    if (!nodeRect) return null
    return gridCellFromPointer({
      point: { x: event.clientX, y: event.clientY },
      gridRect: nodeRect,
      gridWidth: input.currentGridWidth,
      gridHeight: input.currentGridHeight,
    })
  }

  return {
    stickerDrag,
    stickerHover,
    stickerDragRect: stickerDrag ? stickerRectOf(stickerDrag) : null,
    stickerDragCollision: Boolean(
      stickerDrag &&
        stickerPickCollides(
          stickerDrag,
          input.widgets,
          input.currentGridWidth,
          input.currentGridHeight,
        ),
    ),
    onPickMove: (event: ReactMouseEvent) => {
      if (stickerDrag) return
      const cell = pointerCell(event)
      if (!cell) return
      setStickerHover((prev) => (sameGridCell(prev, cell) ? prev : cell))
    },
    onPickLeave: () => setStickerHover(null),
    onPickDown: (event: ReactMouseEvent) => {
      event.preventDefault()
      event.stopPropagation()
      const node = input.gridRectRef.current
      if (!node) return
      const cell = pointerCell(event)
      if (!cell) return
      setStickerDrag({ start: cell, end: cell })
    },
  }
}
