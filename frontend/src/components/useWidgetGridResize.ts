import type {
  MouseEvent as ReactMouseEvent,
  TouchEvent as ReactTouchEvent,
  RefObject,
} from 'react'
import type { WidgetResizeSession } from './widgetGridResize'
import type { WidgetConfig, WidgetSize, WidgetType } from './widgetGridTypes'
import { useCallback, useEffect, useRef, useState } from 'react'
import { getPerformanceProfileSync } from '../hooks/usePerformanceProfile'
import {
  commitWidgetResize,
  nearestResizeSize,
  resizeDraftAllowed,
  resizeRawSpan,
  resizeSupportedSizes,

} from './widgetGridResize'

function getIsMobile(): boolean {
  return getPerformanceProfileSync().isMobile
}

export function useWidgetGridResize(input: {
  isEditMode: boolean
  stickerPickActive: boolean
  isFreeLayout: boolean
  widgets: WidgetConfig[]
  widgetTypeById: Map<string, WidgetType>
  currentGridWidth: number
  currentGridHeight: number
  gridRectRef: RefObject<DOMRect | null>
  updateGridRectCache: () => void
  onWidgetsChange?: (widgets: WidgetConfig[]) => void
  saveToHistory: (widgets: WidgetConfig[]) => void
}) {
  const [resizingWidget, setResizingWidget] =
    useState<WidgetResizeSession | null>(null)
  const rafRef = useRef<number | null>(null)
  const latestRef = useRef(input)
  latestRef.current = input

  const handleResizeStart = useCallback(
    (
      event: ReactMouseEvent | ReactTouchEvent,
      widgetId: string,
      direction: 'se' | 's' = 'se',
    ) => {
      const latest = latestRef.current
      if (!latest.isEditMode || latest.stickerPickActive) return
      event.stopPropagation()
      event.preventDefault()
      const widget = latest.widgets.find((item) => item.id === widgetId)
      if (!widget) return
      latest.updateGridRectCache()
      const clientX =
        'touches' in event ? event.touches[0].clientX : event.clientX
      const clientY =
        'touches' in event ? event.touches[0].clientY : event.clientY
      setResizingWidget({
        widgetId,
        startPos: { x: clientX, y: clientY },
        startSize: widget.size,
        draftSize: widget.size,
        direction,
      })
    },
    [],
  )

  const handleResizeMove = useCallback(
    (event: MouseEvent | TouchEvent) => {
      if (!resizingWidget) return
      if (rafRef.current) return
      rafRef.current = requestAnimationFrame(() => {
        const latest = latestRef.current
        const gridRect = latest.gridRectRef.current
        if (!gridRect) {
          rafRef.current = null
          return
        }
        const widget = latest.widgets.find(
          (item) => item.id === resizingWidget.widgetId,
        )
        if (!widget) {
          rafRef.current = null
          return
        }
        const widgetType = latest.widgetTypeById.get(widget.type)
        if (!widgetType && widget.kind !== 'sticker') {
          rafRef.current = null
          return
        }
        const supported = resizeSupportedSizes(
          widget,
          resizingWidget.startSize,
          widgetType,
        )
        if (supported.length === 0) {
          rafRef.current = null
          return
        }
        const clientX =
          'touches' in event ? event.touches[0].clientX : event.clientX
        const clientY =
          'touches' in event ? event.touches[0].clientY : event.clientY
        const { rawW, rawH } = resizeRawSpan({
          pointer: { x: clientX, y: clientY },
          widget,
          gridRect,
          gridWidth: latest.currentGridWidth,
          gridHeight: latest.currentGridHeight,
          startSize: resizingWidget.startSize,
          direction: resizingWidget.direction ?? 'se',
        })
        const bestSize = nearestResizeSize({
          currentSize: widget.size,
          startSize: resizingWidget.startSize,
          direction: resizingWidget.direction ?? 'se',
          rawW,
          rawH,
          supportedSizes: supported,
        })
        if (
          bestSize !== resizingWidget.draftSize &&
          resizeDraftAllowed({
            widget,
            draftSize: bestSize,
            widgets: latest.widgets,
            gridWidth: latest.currentGridWidth,
            gridHeight: latest.currentGridHeight,
            isFreeLayout: latest.isFreeLayout,
          })
        ) {
          setResizingWidget((prev) =>
            prev ? { ...prev, draftSize: bestSize as WidgetSize } : prev,
          )
        }
        rafRef.current = null
      })
    },
    [resizingWidget],
  )

  const handleResizeEnd = useCallback(() => {
    if (resizingWidget) {
      const latest = latestRef.current
      const next = commitWidgetResize(
        latest.widgets,
        resizingWidget.widgetId,
        resizingWidget.draftSize,
      )
      if (next) {
        latest.onWidgetsChange?.(next)
        latest.saveToHistory(next)
      }
      setResizingWidget(null)
    }
    if (rafRef.current) {
      cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }
  }, [resizingWidget])

  const handleResizeMoveRef = useRef(handleResizeMove)
  const handleResizeEndRef = useRef(handleResizeEnd)
  handleResizeMoveRef.current = handleResizeMove
  handleResizeEndRef.current = handleResizeEnd

  useEffect(() => {
    if (!resizingWidget) return
    const isMobile = getIsMobile()
    const moveHandler = (event: MouseEvent | TouchEvent) =>
      handleResizeMoveRef.current(event)
    const endHandler = () => handleResizeEndRef.current()
    window.addEventListener('mousemove', moveHandler)
    window.addEventListener('mouseup', endHandler)
    window.addEventListener('touchmove', moveHandler, { passive: isMobile })
    window.addEventListener('touchend', endHandler)
    window.addEventListener('touchcancel', endHandler)
    return () => {
      window.removeEventListener('mousemove', moveHandler)
      window.removeEventListener('mouseup', endHandler)
      window.removeEventListener('touchmove', moveHandler)
      window.removeEventListener('touchend', endHandler)
      window.removeEventListener('touchcancel', endHandler)
      if (rafRef.current) {
        cancelAnimationFrame(rafRef.current)
        rafRef.current = null
      }
    }
  }, [resizingWidget])

  return { resizingWidget, handleResizeStart }
}
