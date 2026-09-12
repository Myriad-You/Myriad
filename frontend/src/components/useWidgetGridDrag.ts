import type { MouseEvent as ReactMouseEvent, RefObject } from 'react'
import type { WidgetConfig, WidgetType } from './widgetGridTypes'
import type { WidgetDragSession } from './widgetPlacementPreview'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { getPerformanceProfileSync } from '../hooks/usePerformanceProfile'
import { setWidgetDragCursor } from '../utils/widgetDragCursor'
import { widgetSizeSpan } from '../utils/widgetSizeScale'
import {
  beginExistingWidgetDrag,
  beginLibraryWidgetDrag,
  buildWidgetDragPreview,
  idleWidgetDrag,
  libraryDragFitsBudget,
  resolveExistingWidgetDrop,
  resolveLibraryWidgetDrop,
  settleWidgetDrag,
  shouldClearDragOnEditExit,
} from './widgetGridDrag'
import {
  dragGhostHandoffDelays,
  gridCellFromPoint,
  placementHasCommitted,
} from './widgetPlacementPreview'

function getIsMobile(): boolean {
  return getPerformanceProfileSync().isMobile
}

export function useWidgetGridDrag(input: {
  isEditMode: boolean
  stickerPickActive: boolean
  isFreeLayout: boolean
  widgets: WidgetConfig[]
  widgetTypeById: Map<string, WidgetType>
  currentGridWidth: number
  currentGridHeight: number
  gridRectRef: RefObject<DOMRect | null>
  updateGridRectCache: () => void
  reducedMotion: boolean
  onWidgetsChange?: (widgets: WidgetConfig[]) => void
  saveToHistory: (widgets: WidgetConfig[]) => void
}) {
  const [draggedWidget, setDraggedWidget] = useState<WidgetDragSession | null>(
    null,
  )
  const [dragSettling, setDragSettling] = useState(false)
  const [previewUncovered, setPreviewUncovered] = useState(false)
  const [previewExiting, setPreviewExiting] = useState(false)
  const [hoveredCell, setHoveredCell] = useState<{
    x: number
    y: number
  } | null>(null)
  const settleStartedAtRef = useRef(0)
  const rafRef = useRef<number | null>(null)
  const latestRef = useRef(input)
  latestRef.current = input

  const applyIdle = useCallback(() => {
    const idle = idleWidgetDrag()
    setDraggedWidget(idle.dragged)
    setDragSettling(idle.settling)
    setPreviewUncovered(idle.previewUncovered)
    setPreviewExiting(idle.previewExiting)
    setHoveredCell(idle.hoveredCell)
    setWidgetDragCursor(null)
  }, [])

  const applySession = useCallback(
    (session: ReturnType<typeof beginExistingWidgetDrag>) => {
      setDraggedWidget(session.dragged)
      setDragSettling(session.settling)
      setPreviewUncovered(session.previewUncovered)
      setPreviewExiting(session.previewExiting)
      setHoveredCell(session.hoveredCell)
    },
    [],
  )

  const startNewWidgetDrag = useCallback(
    (widgetTypeId: string, point: { x: number; y: number }) => {
      const latest = latestRef.current
      const widgetType = latest.widgetTypeById.get(widgetTypeId)
      if (
        !libraryDragFitsBudget(
          latest.isFreeLayout,
          latest.widgets,
          widgetType?.defaultSize ?? '2x2',
        )
      ) {
        return
      }
      latest.updateGridRectCache()
      setWidgetDragCursor(point)
      const gridRect = latest.gridRectRef.current
      const cell =
        gridRect && widgetType
          ? gridCellFromPoint({
              point,
              gridRect,
              gridWidth: latest.currentGridWidth,
              gridHeight: latest.currentGridHeight,
              size: widgetSizeSpan(widgetType.defaultSize),
            })
          : null
      applySession(beginLibraryWidgetDrag(widgetTypeId, cell))
    },
    [applySession],
  )

  const handleWidgetDragStart = useCallback(
    (event: ReactMouseEvent, widgetId: string) => {
      const latest = latestRef.current
      if (!latest.isEditMode || latest.stickerPickActive) return
      event.stopPropagation()
      event.preventDefault()
      const widget = latest.widgets.find((item) => item.id === widgetId)
      if (!widget) return
      latest.updateGridRectCache()
      setWidgetDragCursor({ x: event.clientX, y: event.clientY })
      const gridRect = latest.gridRectRef.current
      applySession(
        beginExistingWidgetDrag(
          widgetId,
          gridRect
            ? gridCellFromPoint({
                point: { x: event.clientX, y: event.clientY },
                gridRect,
                gridWidth: latest.currentGridWidth,
                gridHeight: latest.currentGridHeight,
                size: widgetSizeSpan(widget.size),
              })
            : null,
        ),
      )
    },
    [applySession],
  )

  const handleDragMove = useCallback(
    (event: MouseEvent | TouchEvent) => {
      if (!draggedWidget) return
      if (rafRef.current) return
      rafRef.current = requestAnimationFrame(() => {
        const latest = latestRef.current
        const gridRect = latest.gridRectRef.current
        if (!gridRect) {
          rafRef.current = null
          return
        }
        const clientX =
          'touches' in event ? event.touches[0].clientX : event.clientX
        const clientY =
          'touches' in event ? event.touches[0].clientY : event.clientY
        setWidgetDragCursor({ x: clientX, y: clientY })
        const size = draggedWidget.type === 'existing' && draggedWidget.widgetId
          ? latest.widgets.find((item) => item.id === draggedWidget.widgetId)
              ?.size || '1x1'
          : latest.widgetTypeById.get(draggedWidget.widgetTypeId ?? '')
              ?.defaultSize || '1x1'
        const nextCell = gridCellFromPoint({
          point: { x: clientX, y: clientY },
          gridRect,
          gridWidth: latest.currentGridWidth,
          gridHeight: latest.currentGridHeight,
          size: widgetSizeSpan(size),
        })
        setHoveredCell((prev) =>
          prev?.x === nextCell.x && prev?.y === nextCell.y ? prev : nextCell,
        )
        rafRef.current = null
      })
    },
    [draggedWidget],
  )

  const handleDragEnd = useCallback(() => {
    if (dragSettling) return
    if (rafRef.current) {
      cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }
    const latest = latestRef.current
    if (!draggedWidget || !hoveredCell) {
      applyIdle()
      return
    }
    if (draggedWidget.type === 'existing' && draggedWidget.widgetId) {
      const widget = latest.widgets.find(
        (item) => item.id === draggedWidget.widgetId,
      )
      if (!widget) return
      const drop = resolveExistingWidgetDrop({
        widget,
        widgets: latest.widgets,
        cell: hoveredCell,
        gridWidth: latest.currentGridWidth,
        gridHeight: latest.currentGridHeight,
      })
      if (drop.type === 'settle') {
        latest.onWidgetsChange?.(drop.widgets)
        latest.saveToHistory(drop.widgets)
        settleStartedAtRef.current = performance.now()
        const settled = settleWidgetDrag(
          draggedWidget,
          drop.pendingId,
          hoveredCell,
        )
        applySession(settled)
        return
      }
    } else if (draggedWidget.type === 'new' && draggedWidget.widgetTypeId) {
      const widgetType = latest.widgetTypeById.get(draggedWidget.widgetTypeId)
      if (!widgetType) return
      const drop = resolveLibraryWidgetDrop({
        widgetType,
        widgets: latest.widgets,
        cell: hoveredCell,
        gridWidth: latest.currentGridWidth,
        gridHeight: latest.currentGridHeight,
        isFreeLayout: latest.isFreeLayout,
        id: `widget_${Date.now()}`,
      })
      if (drop.type === 'settle') {
        latest.onWidgetsChange?.(drop.widgets)
        latest.saveToHistory(drop.widgets)
        settleStartedAtRef.current = performance.now()
        applySession(settleWidgetDrag(draggedWidget, drop.pendingId, hoveredCell))
        return
      }
    }
    applyIdle()
  }, [applyIdle, applySession, dragSettling, draggedWidget, hoveredCell])

  const handleDragMoveRef = useRef(handleDragMove)
  const handleDragEndRef = useRef(handleDragEnd)
  handleDragMoveRef.current = handleDragMove
  handleDragEndRef.current = handleDragEnd

  useEffect(() => {
    if (!draggedWidget || dragSettling) return
    const isMobile = getIsMobile()
    const moveHandler = (event: MouseEvent | TouchEvent) =>
      handleDragMoveRef.current(event)
    const endHandler = () => handleDragEndRef.current()
    window.addEventListener('mousemove', moveHandler)
    window.addEventListener('mouseup', endHandler)
    window.addEventListener('touchmove', moveHandler, {
      passive: isMobile,
    })
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
  }, [dragSettling, draggedWidget])

  const wasEditModeRef = useRef(input.isEditMode)
  useEffect(() => {
    const wasEditMode = wasEditModeRef.current
    wasEditModeRef.current = input.isEditMode
    if (shouldClearDragOnEditExit(wasEditMode, input.isEditMode)) {
      applyIdle()
    }
  }, [applyIdle, input.isEditMode])

  const handoffId = draggedWidget?.pendingId
  const handoffCommitted = placementHasCommitted(input.widgets, draggedWidget)
  useEffect(() => {
    if (!dragSettling || !handoffId || !handoffCommitted) return
    const reduced =
      input.reducedMotion ||
      window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const delays = dragGhostHandoffDelays(
      reduced,
      performance.now() - settleStartedAtRef.current,
    )
    const uncoverTimer = window.setTimeout(
      setPreviewUncovered,
      delays.uncoverMs,
      true,
    )
    const exitTimer = window.setTimeout(setPreviewExiting, delays.exitMs, true)
    const clearTimer = window.setTimeout(applyIdle, delays.clearMs)
    return () => {
      window.clearTimeout(uncoverTimer)
      window.clearTimeout(exitTimer)
      window.clearTimeout(clearTimer)
    }
  }, [applyIdle, dragSettling, handoffCommitted, handoffId, input.reducedMotion])

  const dragPreview = useMemo(
    () =>
      buildWidgetDragPreview({
        dragged: draggedWidget,
        hoveredCell,
        widgets: input.widgets,
        widgetTypeById: input.widgetTypeById,
        gridWidth: input.currentGridWidth,
        gridHeight: input.currentGridHeight,
      }),
    [
      draggedWidget,
      hoveredCell,
      input.currentGridHeight,
      input.currentGridWidth,
      input.widgetTypeById,
      input.widgets,
    ],
  )

  return {
    draggedWidget,
    dragSettling,
    previewUncovered,
    previewExiting,
    hoveredCell,
    dragPreview,
    startNewWidgetDrag,
    handleWidgetDragStart,
  }
}
