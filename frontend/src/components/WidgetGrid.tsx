import type { HomeLayoutMode } from '../utils/homeLayout'
import type {
  WidgetConfig,
  WidgetGridHandle,
  WidgetSize,
  WidgetType,
} from './widgetGridTypes'

import React, {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useI18n } from '../contexts/I18nContext'
import { useHomeResizeObserver } from '../hooks/animation'
import { isExlight, useAnimationLevel } from '../hooks/useAnimationLevel'
import { useDebouncedWindowSize } from '../hooks/useSharedEventListener'
import {
  findEmptyHomeSlot,
  HOME_FREE_ROWS,
  HOME_STANDARD_COLS,
  isHomeStickerItem,
} from '../utils/homeLayout'
import { widgetSizeSpan } from '../utils/widgetSizeScale'
import { useHomeGridMotionMode, useRowCountMorphing } from './useHomeGridGeometry'
import { useWidgetGridBand } from './useWidgetGridBand'
import { useWidgetGridDrag } from './useWidgetGridDrag'
import { useWidgetGridHistory } from './useWidgetGridHistory'
import { useWidgetGridResize } from './useWidgetGridResize'
import { useWidgetGridStickerPick } from './useWidgetGridStickerPick'
import { WidgetDragGhost } from './WidgetDragGhost'
import { homeGridGeometryMotion } from './widgetGridBand'
import { STICKER_WIDGET_TYPE, WidgetGridItem } from './WidgetGridItem'
import {
  homeGridPixelHeight,
  resolveHomeGridMetrics,
} from './widgetGridMetrics'
import { homeSlotAnchor } from './widgetGridStickerPick'
import {
  coveringWidgetId,
  heldWidgetId,
} from './widgetPlacementPreview'
import './WidgetGrid.css'

export function startGridLibraryDrag(
  grid: WidgetGridHandle | null,
  event: React.MouseEvent | React.TouchEvent,
  widgetTypeId: string,
): void {
  event.stopPropagation()
  event.preventDefault()
  if ('touches' in event) {
    const touch = event.touches[0]
    if (!touch) return
    grid?.startNewWidgetDrag(widgetTypeId, {
      x: touch.clientX,
      y: touch.clientY,
    })
    return
  }
  grid?.startNewWidgetDrag(widgetTypeId, {
    x: event.clientX,
    y: event.clientY,
  })
}

interface WidgetGridProps {
  widgets: WidgetConfig[]
  availableWidgets: WidgetType[]
  onWidgetsChange?: (widgets: WidgetConfig[]) => void
  isEditMode: boolean
  children?: React.ReactNode
  customGridColumns?: number
  customGridRows?: number
  autoHeight?: boolean
  layoutMode?: HomeLayoutMode
  stickerPickActive?: boolean
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
  stickerHighlight?: { x: number; y: number; size: WidgetSize } | null
  // 仅首页教程；控制面板网格不得设。
  tourAnchor?: string
  tourFit?: string
}

const WidgetGrid = forwardRef<WidgetGridHandle, WidgetGridProps>(
  (
    {
      widgets,
      availableWidgets,
      onWidgetsChange,
      isEditMode,
      children,
      customGridColumns,
      customGridRows,
      autoHeight,
      layoutMode = 'standard',
      stickerPickActive = false,
      onPickStickerSlot,
      stickerHighlight = null,
      tourAnchor,
      tourFit,
    },
    ref,
  ) => {
    const { t } = useI18n()
    const isFreeLayout = layoutMode === 'free'
    const anim = useAnimationLevel()
    const animHardCut = isExlight(anim)
    const { width: windowWidth } = useDebouncedWindowSize(
      150,
      !isFreeLayout && !customGridColumns,
    )
    const { gridColumns, bandSwitch } = useWidgetGridBand({
      customGridColumns,
      isFreeLayout,
      hardCut: animHardCut,
      windowWidth,
    })
    const motionMode = useHomeGridMotionMode(layoutMode)
    const geometryMotion = homeGridGeometryMotion({
      exlight: animHardCut,
      bandSwitch,
      motionMode,
      layoutMode,
    })
    const metrics = resolveHomeGridMetrics({
      widgets,
      layoutMode,
      customGridColumns,
      customGridRows,
      autoHeight,
      gridColumns,
    })
    const {
      isCompact,
      currentWidgets,
      currentGridWidth,
      currentGridHeight,
    } = metrics
    const [containerWidth, setContainerWidth] = useState(0)
    const containerRef = useRef<HTMLDivElement | null>(null)
    const gridRectRef = useRef<DOMRect | null>(null)
    const gridPixelHeight = homeGridPixelHeight(
      containerWidth,
      currentGridWidth,
      currentGridHeight,
      isFreeLayout,
    )
    const rowCountMorphing = useRowCountMorphing(currentGridHeight)
    const { saveToHistory } = useWidgetGridHistory(isEditMode, onWidgetsChange)

    const widgetTypeById = useMemo(() => {
      const map = new Map<string, WidgetType>()
      for (const widgetType of availableWidgets)
        map.set(widgetType.id, widgetType)
      map.set(STICKER_WIDGET_TYPE.id, STICKER_WIDGET_TYPE)
      return map
    }, [availableWidgets])

    const updateGridRectCache = useCallback(() => {
      if (containerRef.current) {
        gridRectRef.current = containerRef.current.getBoundingClientRect()
      }
    }, [])

    const drag = useWidgetGridDrag({
      isEditMode,
      stickerPickActive,
      isFreeLayout,
      widgets,
      widgetTypeById,
      currentGridWidth,
      currentGridHeight,
      gridRectRef,
      updateGridRectCache,
      reducedMotion: animHardCut,
      onWidgetsChange,
      saveToHistory,
    })
    const { resizingWidget, handleResizeStart } = useWidgetGridResize({
      isEditMode,
      stickerPickActive,
      isFreeLayout,
      widgets,
      widgetTypeById,
      currentGridWidth,
      currentGridHeight,
      gridRectRef,
      updateGridRectCache,
      onWidgetsChange,
      saveToHistory,
    })
    const sticker = useWidgetGridStickerPick({
      widgets,
      currentGridWidth,
      currentGridHeight,
      gridRectRef,
      onPickStickerSlot,
    })

    const latestRef = useRef({
      widgets,
      onWidgetsChange,
    })
    latestRef.current = { widgets, onWidgetsChange }

    useImperativeHandle(
      ref,
      () => ({
        startNewWidgetDrag: drag.startNewWidgetDrag,
      }),
      [drag.startNewWidgetDrag],
    )

    const { observeHomeResize, unobserveHomeResize } = useHomeResizeObserver()
    const gridRef = useCallback(
      (node: HTMLDivElement | null) => {
        if (containerRef.current) {
          unobserveHomeResize(containerRef.current)
        }
        containerRef.current = node
        if (node) {
          observeHomeResize(node, (entry) => {
            setContainerWidth(entry.contentRect.width)
            gridRectRef.current = node.getBoundingClientRect()
          })
          setContainerWidth(node.getBoundingClientRect().width)
        }
      },
      [observeHomeResize, unobserveHomeResize],
    )

    useEffect(() => {
      return () => {
        if (containerRef.current) {
          unobserveHomeResize(containerRef.current)
        }
      }
    }, [unobserveHomeResize])

    const handleRemoveWidget = useCallback(
      (widgetId: string) => {
        const { widgets: current, onWidgetsChange: notify } = latestRef.current
        const next = current.filter((widget) => widget.id !== widgetId)
        notify?.(next)
        saveToHistory(next)
      },
      [saveToHistory],
    )

    const handleWidgetConfigChange = useCallback(
      (widgetId: string, newConfig: unknown) => {
        const { widgets: current, onWidgetsChange: notify } = latestRef.current
        const next = current.map((widget) =>
          widget.id === widgetId ? { ...widget, config: newConfig } : widget,
        )
        notify?.(next)
        saveToHistory(next)
      },
      [saveToHistory],
    )

    const [hoveredWidgetId, setHoveredWidgetId] = useState<string | null>(null)
    const handleWidgetMouseLeave = useCallback(
      () => setHoveredWidgetId(null),
      [],
    )

    const gridBackground = useMemo(
      () => (
        <div
          className={`widget-grid-background absolute inset-0 pointer-events-none z-0${
            stickerPickActive ? ' is-sticker-pick' : ''
          }`}
          style={
            {
              '--widget-grid-cell-w': `${100 / currentGridWidth}%`,
              '--widget-grid-cell-h': `${100 / currentGridHeight}%`,
            } as React.CSSProperties
          }
        />
      ),
      [currentGridWidth, currentGridHeight, stickerPickActive],
    )

    return (
      <div
        className={`widget-grid-root flex flex-col gap-2 min-h-0 ${
          isCompact ? 'h-auto flex-none' : 'h-full flex-1'
        }`}
        data-grid-cols={currentGridWidth}
        data-grid-compact={isCompact ? 'true' : 'false'}
        data-layout-mode={layoutMode}
        data-band-switch={bandSwitch ?? undefined}
      >
        <div
          className={`relative w-full flex flex-col min-h-0 ${
            isCompact
              ? 'justify-start pb-20'
              : isFreeLayout
                ? 'flex-1 items-center justify-center'
                : 'flex-1 justify-end'
          }`}
        >
          {children}

          <div
            className={
              isFreeLayout
                ? 'widget-grid-host widget-grid-host--free'
                : 'widget-grid-host'
            }
          >
            <div
              ref={gridRef}
              className={`widget-grid-container relative rounded-xl w-full ${
                geometryMotion && rowCountMorphing && !isFreeLayout
                  ? 'widget-grid-container--layout-motion'
                  : ''
              } ${isEditMode ? 'edit-mode' : ''}`}
              data-tour={tourAnchor}
              data-tour-fit={tourFit}
              data-band-switch={bandSwitch ?? undefined}
              style={
                isFreeLayout
                  ? {
                      aspectRatio: `${HOME_STANDARD_COLS} / ${HOME_FREE_ROWS}`,
                    }
                  : gridPixelHeight
                    ? { height: gridPixelHeight }
                    : {
                        aspectRatio: `${currentGridWidth} / ${currentGridHeight}`,
                      }
              }
            >
              {isEditMode && !isCompact && gridBackground}

              {drag.dragPreview?.position && !isCompact && (
                <div
                  className={`widget-grid-drop-slot${
                    drag.dragPreview.hasCollision ? ' is-blocked' : ''
                  }`}
                  style={{
                    left: `${(drag.dragPreview.position.x / currentGridWidth) * 100}%`,
                    top: `${(drag.dragPreview.position.y / currentGridHeight) * 100}%`,
                    width: `${(drag.dragPreview.size.w / currentGridWidth) * 100}%`,
                    height: `${(drag.dragPreview.size.h / currentGridHeight) * 100}%`,
                  }}
                >
                  {drag.dragPreview.hasCollision ? (
                    <div className="widget-grid-drop-slot-flag">
                      {t.widgetGrid.positionConflict}
                    </div>
                  ) : null}
                </div>
              )}

              {stickerPickActive && isFreeLayout && isEditMode && !isCompact ? (
                <div
                  data-sticker-pick=""
                  className="absolute inset-0 z-30 cursor-crosshair bg-transparent"
                  onMouseMove={sticker.onPickMove}
                  onMouseLeave={sticker.onPickLeave}
                  onMouseDown={(event) => {
                    const node = containerRef.current
                    if (node) {
                      gridRectRef.current = node.getBoundingClientRect()
                    }
                    sticker.onPickDown(event)
                  }}
                />
              ) : null}
              {stickerPickActive &&
              sticker.stickerHover &&
              !sticker.stickerDrag &&
              currentGridWidth > 0 ? (
                <div
                  className="absolute z-20 pointer-events-none rounded-lg bg-[color-mix(in_srgb,var(--color-primary,#8b5cf6)_22%,transparent)] ring-2 ring-[color-mix(in_srgb,var(--color-primary,#8b5cf6)_70%,white)]"
                  style={{
                    left: `${(sticker.stickerHover.x / currentGridWidth) * 100}%`,
                    top: `${(sticker.stickerHover.y / currentGridHeight) * 100}%`,
                    width: `${(1 / currentGridWidth) * 100}%`,
                    height: `${(1 / currentGridHeight) * 100}%`,
                  }}
                />
              ) : null}
              {stickerHighlight && currentGridWidth > 0 ? (
                <div
                  className="absolute z-20 pointer-events-none rounded-xl bg-[color-mix(in_srgb,var(--color-primary,#8b5cf6)_18%,transparent)] ring-2 ring-[color-mix(in_srgb,var(--color-primary,#8b5cf6)_70%,white)]"
                  style={{
                    left: `${(stickerHighlight.x / currentGridWidth) * 100}%`,
                    top: `${(stickerHighlight.y / currentGridHeight) * 100}%`,
                    width: `${(widgetSizeSpan(stickerHighlight.size).w / currentGridWidth) * 100}%`,
                    height: `${(widgetSizeSpan(stickerHighlight.size).h / currentGridHeight) * 100}%`,
                  }}
                />
              ) : null}
              {sticker.stickerDrag &&
              sticker.stickerDragRect &&
              currentGridWidth > 0 ? (
                <div
                  className={`absolute z-20 pointer-events-none rounded-xl ${
                    sticker.stickerDragCollision
                      ? 'bg-red-500/10 ring-2 ring-red-500/50'
                      : 'bg-[color-mix(in_srgb,var(--color-primary,#8b5cf6)_18%,transparent)] ring-2 ring-[color-mix(in_srgb,var(--color-primary,#8b5cf6)_70%,white)]'
                  }`}
                  style={{
                    left: `${(sticker.stickerDragRect.x / currentGridWidth) * 100}%`,
                    top: `${(sticker.stickerDragRect.y / currentGridHeight) * 100}%`,
                    width: `${(sticker.stickerDragRect.w / currentGridWidth) * 100}%`,
                    height: `${(sticker.stickerDragRect.h / currentGridHeight) * 100}%`,
                  }}
                />
              ) : null}

              <div
                className={`absolute inset-0 z-10${
                  stickerPickActive ? ' pointer-events-none' : ''
                }`}
              >
                {currentWidgets.map((rawWidget, index) => {
                  const widget =
                    resizingWidget?.widgetId === rawWidget.id &&
                    resizingWidget.draftSize !== rawWidget.size
                      ? { ...rawWidget, size: resizingWidget.draftSize }
                      : rawWidget
                  const widgetType = isHomeStickerItem(widget)
                    ? STICKER_WIDGET_TYPE
                    : widgetTypeById.get(widget.type)
                  if (!widgetType) {
                    const dim = widgetSizeSpan(widget.size)
                    const gw = currentGridWidth
                    const gh = currentGridHeight
                    return (
                      <div
                        key={widget.id}
                        className="widget-grid-item absolute flex items-center justify-center"
                        style={{
                          left: `${(widget.position.x / gw) * 100}%`,
                          top: `${(widget.position.y / gh) * 100}%`,
                          width: `${(dim.w / gw) * 100}%`,
                          height: `${(dim.h / gh) * 100}%`,
                          zIndex: 10,
                        }}
                      >
                        <div className="relative h-full w-full p-1">
                          <div className="h-full w-full rounded-xl border border-dashed border-gray-300/60 dark:border-white/15 bg-white/40 dark:bg-white/5 flex items-center justify-center px-4">
                            <span className="text-xs text-gray-400 dark:text-white/35 text-center break-all">
                              {widget.type}
                            </span>
                          </div>
                        </div>
                      </div>
                    )
                  }

                  const handleConfigChange = (newConfig: unknown) =>
                    handleWidgetConfigChange(widget.id, newConfig)

                  return (
                    <WidgetGridItem
                      key={widget.id}
                      widget={widget}
                      widgetType={widgetType}
                      isEditMode={isEditMode && !isCompact}
                      isHeld={
                        heldWidgetId({
                          dragged: drag.draggedWidget,
                          settling: drag.dragSettling,
                          uncovered:
                            drag.previewUncovered || drag.previewExiting,
                        }) === widget.id
                      }
                      isCovered={
                        coveringWidgetId({
                          dragged: drag.draggedWidget,
                          settling: drag.dragSettling,
                        }) === widget.id
                      }
                      isHovered={hoveredWidgetId === widget.id}
                      onDragStart={drag.handleWidgetDragStart}
                      onMouseEnter={setHoveredWidgetId}
                      onMouseLeave={handleWidgetMouseLeave}
                      onRemove={handleRemoveWidget}
                      onResizeStart={handleResizeStart}
                      gridWidth={currentGridWidth}
                      gridHeight={currentGridHeight}
                      onConfigChange={handleConfigChange}
                      index={index}
                      layoutMotion={geometryMotion}
                      onRequestSticker={(item) => {
                        const slot = findEmptyHomeSlot(
                          widgets,
                          item.size,
                          currentGridWidth,
                          currentGridHeight,
                        )
                        if (!slot) return
                        onPickStickerSlot?.({
                          ...slot,
                          size: item.size,
                          anchor: homeSlotAnchor(
                            gridRectRef.current,
                            slot,
                            item.size,
                            currentGridWidth,
                            currentGridHeight,
                          ),
                        })
                      }}
                      allowSticker={Boolean(
                        isFreeLayout &&
                          isEditMode &&
                          stickerPickActive &&
                          onPickStickerSlot,
                      )}
                    />
                  )
                })}
              </div>
            </div>
          </div>
        </div>

        <WidgetDragGhost
          active={Boolean(drag.draggedWidget)}
          settling={drag.dragSettling}
          exiting={drag.previewExiting}
          reducedMotion={isExlight(anim)}
          dragPreview={drag.dragPreview}
          gridWidth={currentGridWidth}
          gridHeight={currentGridHeight}
          gridRectRef={gridRectRef}
        />
      </div>
    )
  },
)

export default WidgetGrid
