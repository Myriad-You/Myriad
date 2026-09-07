/**
 * 可视化编辑的网格小组件系统
 * 标准布局 16×4（窄屏紧凑重排）；自由布局同格、同宽、固定 16×8
 */

import type { HomeLayoutMode } from '../utils/homeLayout'
import type {
  WidgetConfig,
  WidgetGridHandle,
  WidgetSize,
  WidgetType,
} from './widgetGridTypes'
import { LuSparkles, LuX } from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'

import React, {
  forwardRef,
  Suspense,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { createPortal } from 'react-dom'
import { useI18n } from '../contexts/I18nContext'
import { useHomeResizeObserver, useStaggerAnimation } from '../hooks/animation'
import {
  isExlight,
  isStandardAnimation,
  useAnimationLevel,
} from '../hooks/useAnimationLevel'
import { getPerformanceProfileSync } from '../hooks/usePerformanceProfile'
import { useDebouncedWindowSize } from '../hooks/useSharedEventListener'
import {
  freeLayoutFitsCellBudget,
  HOME_FREE_ROWS,
  HOME_STANDARD_COLS,
  HOME_STANDARD_ROWS,
  homeWidgetCellCount,
  homeWidgetsOccupiedCells,
  isHomeStickerItem,
  isHomeWidgetItem,
  findEmptyHomeSlot,
  packWidgetsIntoColumns,
} from '../utils/homeLayout'
import { HomeStickerCrop } from './home/HomeStickerCrop'
import { HomeStickerCropTip } from './home/HomeStickerCropTip'
import {
  defaultStickerCrop,
  parseStickerCrop,
  stickerSlotAspect,
  type StickerCrop,
} from '../utils/homeStickerCrop'
import {
  placeHomeStickerSelection,
  stickerSizesSharingAspect,
} from '../utils/homeStickerSize'
import { WidgetInstanceSettings } from './widgets/shared/WidgetInstanceSettings'
import { WidgetLongPressHint } from './widgets/shared/WidgetLongPressHint'
import { resolveHomeGridColumns } from '../utils/viewportBands'
import { setWidgetDragCursor, useWidgetDragCursor } from '../utils/widgetDragCursor'
import {
  GRID_WIDGET_PAD_PX,
  WIDGET_SIZE_KEYS,
  widgetSizeSpan,
} from '../utils/widgetSizeScale'
import { widgetDisplayLabel, widgetHostConfig } from './widgetLibraryModel'
import {
  coveringWidgetId,
  dragGhostContentSize,
  dragGhostHandoffDelays,
  gridCellFromPoint,
  heldWidgetId,
  placementHasCommitted,
  resolveDragGhostWidget,
  shouldSkipWidgetEntrance,
  widgetDragGhostBox,
  type WidgetDragSession,
} from './widgetPlacementPreview'
import StickerWidget, {
  stickerFloatMode,
  stickerFloatPatch,
} from './widgets/StickerWidget'
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

// ⚗ 移动端检测 - 使用统一的性能检测系统
function getIsMobile(): boolean {
  return getPerformanceProfileSync().isMobile
}

// 网格尺寸常量（列数阈值见 utils/viewportBands.ts，与主页壳 / Tailwind lg 统一）
const GRID_WIDTH = HOME_STANDARD_COLS
const GRID_HEIGHT = HOME_STANDARD_ROWS

/**
 * Cross-band (tablet↔desktop) layout morph: never lerp left/top between compact
 * packing and desktop saved coords — fade out → hard swap → fade in.
 * Phone band always hard-cuts (DevTools mobile preview must not flash 16-col).
 */
const GRID_BAND_OUT_MS = 160
const GRID_BAND_IN_MS = 220

function readInitialHomeGridColumns(custom?: number): number {
  if (custom) return custom
  if (typeof window === 'undefined') return GRID_WIDTH
  return resolveHomeGridColumns(window.innerWidth, 0)
}

/** Position/grid 变时外壳要重绘，内部实现（iframe / 数据）不必跟着重挂。 */
const WidgetGridItemBody = React.memo(
  ({
    widget,
    widgetType,
    isEditMode,
    isPreview,
    onConfigChange,
  }: {
    widget: WidgetConfig
    widgetType: WidgetType
    isEditMode: boolean
    isPreview?: boolean
    onConfigChange?: (newConfig: any) => void
  }) => {
    const WidgetComponent = widgetType.component
    return (
      <Suspense fallback={null}>
        <WidgetComponent
          config={widget}
          isEditMode={isEditMode}
          isPreview={isPreview}
          onConfigChange={onConfigChange}
        />
      </Suspense>
    )
  },
  (prev, next) =>
    prev.widget.id === next.widget.id &&
    prev.widget.type === next.widget.type &&
    prev.widget.size === next.widget.size &&
    prev.widget.config === next.widget.config &&
    prev.isEditMode === next.isEditMode &&
    prev.isPreview === next.isPreview &&
    prev.widgetType === next.widgetType,
)

// Memoized Widget Item Component
const WidgetGridItem = React.memo(
  ({
    widget,
    widgetType,
    isEditMode,
    isPreview,
    isHeld,
    isCovered,
    isHovered,
    onDragStart,
    onMouseEnter,
    onMouseLeave,
    onRemove,
    onResizeStart,
    gridWidth,
    gridHeight,
    onConfigChange,
    index = 0,
    layoutMotion = true,
    onRequestSticker,
    allowSticker = false,
  }: {
    widget: WidgetConfig
    widgetType: WidgetType
    isEditMode: boolean
    isPreview?: boolean
    isHeld?: boolean
    isCovered?: boolean
    isHovered: boolean
    onDragStart: (e: React.MouseEvent, id: string) => void
    onMouseEnter: (id: string) => void
    onMouseLeave: () => void
    onRemove: (id: string) => void
    onResizeStart: (
      e: React.MouseEvent | React.TouchEvent,
      id: string,
      direction?: 'se' | 's',
    ) => void
    gridWidth?: number
    gridHeight?: number
    onConfigChange?: (newConfig: any) => void
    /** 组件索引，用于计算递增延迟 */
    index?: number
    /** Geometry (left/top/w/h) CSS transition for band reflow */
    layoutMotion?: boolean
    onRequestSticker?: (widget: WidgetConfig) => void
    allowSticker?: boolean
  }) => {
    const anim = useAnimationLevel()
    const { t } = useI18n()
    const [showSettings, setShowSettings] = useState(false)
    const [settingsAnchor, setSettingsAnchor] = useState<DOMRect | null>(null)
    const [stickerCropOpen, setStickerCropOpen] = useState(false)
    const [stickerCropDraft, setStickerCropDraft] = useState<StickerCrop>(
      defaultStickerCrop,
    )
    const stickerPressRef = useRef<{
      timer: number
      move: (event: MouseEvent) => void
      touchMove: (event: TouchEvent) => void
      up: () => void
    } | null>(null)
    const stickerItemRef = useRef<HTMLDivElement>(null)
    const [stickerTipAnchor, setStickerTipAnchor] = useState<DOMRect | null>(
      null,
    )
    const instanceSettings = widgetType.settings || []
    const stickerSrc =
      typeof widget.config?.imageUrl === 'string'
        ? widget.config.imageUrl.trim()
        : ''

    const closeStickerCrop = useCallback(
      (save: boolean) => {
        if (save && onConfigChange) {
          onConfigChange({
            ...(widget.config && typeof widget.config === 'object'
              ? widget.config
              : {}),
            crop: stickerCropDraft,
          })
        }
        setStickerCropOpen(false)
        setStickerTipAnchor(null)
      },
      [onConfigChange, stickerCropDraft, widget.config],
    )

    // 使用统一动画协调系统；exlight 模式直接显示且不进入调度队列。
    // Entrance timing intentionally eased after feedback that 80ms / stiff-300 felt too fast.
    // Edit mode skips stagger: a tile dropped from the library must paint now.
    const animationsEnabled = !isExlight(anim)
    const skipEntrance = shouldSkipWidgetEntrance(isEditMode)
    const { canAnimate, onComplete } = useStaggerAnimation({
      groupId: 'widget-grid',
      index: index || 0,
      baseDelay: 115,
      enabled: animationsEnabled && !skipEntrance,
    })

    const dim = widgetSizeSpan(widget.size)

    // 使用传入的网格尺寸或默认值
    const gw = gridWidth || GRID_WIDTH
    const gh = gridHeight || GRID_HEIGHT

    // Always % geometry: tablet compact reflow and desktop 16-col share one unit
    // system so left/top/width/height can interpolate across band changes.
    const style: React.CSSProperties = {
      left: `${(widget.position.x / gw) * 100}%`,
      top: `${(widget.position.y / gh) * 100}%`,
      width: `${(dim.w / gw) * 100}%`,
      height: `${(dim.h / gh) * 100}%`,
      zIndex: stickerCropOpen || (isHovered && !isCovered) ? 40 : 10,
      // 只提示 transform：left/top 是布局属性，will-change 对它们没有
      // 加速作用，写上去只是让编辑模式下每个小组件白白多提升一层合成层。
      willChange: isEditMode && isHovered && !isCovered ? 'transform' : 'auto',
    }

    useLayoutEffect(() => {
      if (!stickerCropOpen) return
      const sync = () => {
        setStickerTipAnchor(
          stickerItemRef.current?.getBoundingClientRect() ?? null,
        )
      }
      sync()
      window.addEventListener('resize', sync)
      window.addEventListener('scroll', sync, true)
      return () => {
        window.removeEventListener('resize', sync)
        window.removeEventListener('scroll', sync, true)
      }
    }, [stickerCropOpen])

    useEffect(() => {
      return () => {
        const press = stickerPressRef.current
        if (!press) return
        window.clearTimeout(press.timer)
        window.removeEventListener('mousemove', press.move)
        window.removeEventListener('mouseup', press.up)
        window.removeEventListener('touchmove', press.touchMove)
        window.removeEventListener('touchend', press.up)
        window.removeEventListener('touchcancel', press.up)
      }
    }, [])

    const canResize = isHomeStickerItem(widget)
      ? stickerSizesSharingAspect(widget.size).length > 1
      : !widgetType.supportedSizes || widgetType.supportedSizes.length > 1

    // 低性能模式 / 低端设备：禁用 spring，改用轻量 tween
    const useLiteTransition = !anim.spring || !isStandardAnimation(anim)

    return (
      <motion.div
        className={`widget-grid-item absolute ${
          isHomeStickerItem(widget) ? 'widget-grid-item--sticker' : ''
        } ${
          layoutMotion && animationsEnabled
            ? 'widget-grid-item--layout-motion'
            : ''
        }`}
        style={style}
        initial={
          animationsEnabled && !skipEntrance
            ? { opacity: 0, scale: 0.9, y: 14 }
            : false
        }
        animate={
          !animationsEnabled || skipEntrance || canAnimate
            ? { opacity: 1, scale: 1, y: 0 }
            : { opacity: 0, scale: 0.9, y: 14 }
        }
        exit={animationsEnabled ? { opacity: 0, scale: 0.9 } : undefined}
        onAnimationComplete={onComplete}
        transition={
          !animationsEnabled
            ? { duration: 0 }
            : useLiteTransition
              ? { type: 'tween', duration: 0.48 }
              : {
                  type: 'spring',
                  stiffness: 230,
                  damping: 29,
                }
        }
      >
        <div
          className={`relative h-full w-full group ${
            isHomeStickerItem(widget) ? 'p-0' : 'p-1'
          }${isHeld ? ' widget-grid-item-handoff is-held' : ''}${
            isCovered ? ' widget-grid-item-handoff is-covered' : ''
          }`}
        >
          <div
            ref={stickerItemRef}
            className={`relative h-full w-full rounded-xl transition-shadow ${
              isHomeStickerItem(widget) ? 'overflow-visible' : 'overflow-hidden'
            } ${
              isEditMode && !isHomeStickerItem(widget) && !isCovered
                ? 'cursor-move ring-1 ring-transparent hover:ring-blue-400/50'
                : isEditMode
                  ? 'cursor-move'
                  : ''
            } ${isHovered && isEditMode && !isHomeStickerItem(widget) && !isCovered ? 'ring-blue-400/50 shadow-lg' : ''}`}
            onMouseDown={(event) => {
              if (stickerCropOpen || showSettings) {
                event.stopPropagation()
                return
              }
              const holdSticker =
                isEditMode && isHomeStickerItem(widget) && Boolean(stickerSrc)
              const holdSettings =
                isEditMode &&
                instanceSettings.length > 0 &&
                Boolean(onConfigChange)
              if (!holdSticker && !holdSettings) {
                onDragStart(event, widget.id)
                return
              }
              event.stopPropagation()
              const startX = event.clientX
              const startY = event.clientY
              const clearPress = () => {
                const press = stickerPressRef.current
                if (!press) return
                window.clearTimeout(press.timer)
                window.removeEventListener('mousemove', press.move)
                window.removeEventListener('mouseup', press.up)
                window.removeEventListener('touchmove', press.touchMove)
                window.removeEventListener('touchend', press.up)
                window.removeEventListener('touchcancel', press.up)
                stickerPressRef.current = null
              }
              const moved = (x: number, y: number) => {
                if (Math.hypot(x - startX, y - startY) < 8) return
                clearPress()
                onDragStart(
                  {
                    clientX: x,
                    clientY: y,
                    stopPropagation() {},
                    preventDefault() {},
                  } as React.MouseEvent,
                  widget.id,
                )
              }
              const move = (moveEvent: MouseEvent) =>
                moved(moveEvent.clientX, moveEvent.clientY)
              const touchMove = (touchEvent: TouchEvent) => {
                const touch = touchEvent.touches[0]
                if (touch) moved(touch.clientX, touch.clientY)
              }
              const up = () => clearPress()
              const timer = window.setTimeout(() => {
                clearPress()
                if (holdSticker) {
                  setStickerCropDraft(
                    parseStickerCrop(widget.config?.crop) ??
                      defaultStickerCrop(),
                  )
                  setStickerCropOpen(true)
                  return
                }
                setSettingsAnchor(
                  stickerItemRef.current?.getBoundingClientRect() ?? null,
                )
                setShowSettings(true)
              }, 500)
              stickerPressRef.current = { timer, move, touchMove, up }
              window.addEventListener('mousemove', move)
              window.addEventListener('mouseup', up)
              window.addEventListener('touchmove', touchMove, { passive: true })
              window.addEventListener('touchend', up)
              window.addEventListener('touchcancel', up)
            }}
            onTouchStart={(event) => {
              if (!isEditMode || stickerCropOpen || showSettings) return
              const holdSticker =
                isHomeStickerItem(widget) && Boolean(stickerSrc)
              const holdSettings =
                instanceSettings.length > 0 && Boolean(onConfigChange)
              if (!holdSticker && !holdSettings) return
              const touch = event.touches[0]
              if (!touch) return
              event.stopPropagation()
              const startX = touch.clientX
              const startY = touch.clientY
              const clearPress = () => {
                const press = stickerPressRef.current
                if (!press) return
                window.clearTimeout(press.timer)
                window.removeEventListener('mousemove', press.move)
                window.removeEventListener('mouseup', press.up)
                window.removeEventListener('touchmove', press.touchMove)
                window.removeEventListener('touchend', press.up)
                window.removeEventListener('touchcancel', press.up)
                stickerPressRef.current = null
              }
              const moved = (x: number, y: number) => {
                if (Math.hypot(x - startX, y - startY) < 8) return
                clearPress()
                onDragStart(
                  {
                    clientX: x,
                    clientY: y,
                    stopPropagation() {},
                    preventDefault() {},
                  } as React.MouseEvent,
                  widget.id,
                )
              }
              const move = (moveEvent: MouseEvent) =>
                moved(moveEvent.clientX, moveEvent.clientY)
              const touchMove = (touchEvent: TouchEvent) => {
                const next = touchEvent.touches[0]
                if (next) moved(next.clientX, next.clientY)
              }
              const up = () => clearPress()
              const timer = window.setTimeout(() => {
                clearPress()
                if (holdSticker) {
                  setStickerCropDraft(
                    parseStickerCrop(widget.config?.crop) ??
                      defaultStickerCrop(),
                  )
                  setStickerCropOpen(true)
                  return
                }
                setSettingsAnchor(
                  stickerItemRef.current?.getBoundingClientRect() ?? null,
                )
                setShowSettings(true)
              }, 500)
              stickerPressRef.current = { timer, move, touchMove, up }
              window.addEventListener('mousemove', move)
              window.addEventListener('mouseup', up)
              window.addEventListener('touchmove', touchMove, { passive: true })
              window.addEventListener('touchend', up)
              window.addEventListener('touchcancel', up)
            }}
            onMouseEnter={() => isEditMode && onMouseEnter(widget.id)}
            onMouseLeave={onMouseLeave}
          >
            <WidgetGridItemBody
              widget={widget}
              widgetType={widgetType}
              isEditMode={isEditMode}
              isPreview={isPreview}
              onConfigChange={onConfigChange}
            />
            {isEditMode && isHomeStickerItem(widget) && stickerSrc ? (
              <WidgetLongPressHint
                title={t.home.stickerLongPressEdit}
                visible={!stickerCropOpen}
                onClick={() => {
                  setStickerCropDraft(
                    parseStickerCrop(widget.config?.crop) ?? defaultStickerCrop(),
                  )
                  setStickerCropOpen(true)
                }}
              />
            ) : isEditMode && instanceSettings.length > 0 ? (
              <WidgetLongPressHint
                title={t.widgetGrid.longPressToEdit}
                visible={!showSettings}
                onClick={() => {
                  setSettingsAnchor(
                    stickerItemRef.current?.getBoundingClientRect() ?? null,
                  )
                  setShowSettings(true)
                }}
              />
            ) : null}
            {isEditMode && !stickerCropOpen ? (
              <>
                <button
                  type="button"
                  className="widget-grid-item-remove"
                  onMouseDown={(event) => event.stopPropagation()}
                  onClick={(e) => {
                    e.stopPropagation()
                    onRemove(widget.id)
                  }}
                  title={t.widgetGrid.deleteWidget}
                  aria-label={t.widgetGrid.deleteWidget}
                >
                  <LuX className="widget-grid-item-remove__icon" aria-hidden />
                </button>
                {allowSticker && onRequestSticker && isHomeWidgetItem(widget) ? (
                  <button
                    type="button"
                    className="widget-grid-item-sticker"
                    onMouseDown={(event) => event.stopPropagation()}
                    onClick={(event) => {
                      event.stopPropagation()
                      onRequestSticker(widget)
                    }}
                    title={t.widgetGrid.createSticker}
                    aria-label={t.widgetGrid.createSticker}
                  >
                    <LuSparkles
                      className="widget-grid-item-sticker__icon"
                      aria-hidden
                    />
                  </button>
                ) : null}
              </>
            ) : null}
            {stickerCropOpen && stickerSrc ? (
              <div
                className="home-sticker-crop-overlay"
                onMouseDown={(event) => event.stopPropagation()}
              >
                <HomeStickerCrop
                  src={stickerSrc}
                  aspect={stickerSlotAspect(widget.size)}
                  crop={stickerCropDraft}
                  fill
                  onChange={setStickerCropDraft}
                />
              </div>
            ) : null}
            <HomeStickerCropTip
              open={stickerCropOpen && Boolean(stickerSrc)}
              anchor={stickerTipAnchor}
              src={stickerSrc}
              mode={stickerFloatMode(widget.config)}
              ignoreRef={stickerItemRef}
              onMode={(mode) => {
                const current =
                  widget.config && typeof widget.config === 'object'
                    ? widget.config
                    : {}
                onConfigChange?.({
                  ...current,
                  ...stickerFloatPatch(mode),
                })
              }}
              onClose={() => closeStickerCrop(true)}
            />
          </div>

          {isEditMode && !stickerCropOpen && canResize ? (
            <div
              className={`absolute bottom-0 right-0 cursor-se-resize z-50 flex items-end justify-end transition-transform hover:scale-110 active:scale-95 group/resize touch-none ${
                widget.size === '1x1'
                  ? 'w-8 h-8 p-0.5 md:w-6 md:h-6'
                  : 'w-14 h-14 p-2 md:w-12 md:h-12'
              }`}
              onMouseDown={(e) => onResizeStart(e, widget.id, 'se')}
              onTouchStart={(e) => onResizeStart(e, widget.id, 'se')}
            >
              {/* L 型条 - 适配主题色，1x1组件更小 */}
              <div
                className={`border-b-8 border-r-8 rounded-br-xl drop-shadow-[0_4px_4px_color-mix(in_srgb,var(--color-primary),transparent_70%)] opacity-60 group-hover/resize:opacity-100 transition-all duration-200 border-[color-mix(in_srgb,var(--color-primary),white_60%)] group-hover/resize:border-[color-mix(in_srgb,var(--color-primary),white_30%)] dark:border-[color-mix(in_srgb,var(--color-primary),black_60%)] dark:group-hover/resize:border-[color-mix(in_srgb,var(--color-primary),black_30%)] ${
                  widget.size === '1x1'
                    ? 'w-4 h-4 border-b-5 border-r-5'
                    : 'w-6 h-6'
                }`}
              />
            </div>
          ) : null}
        </div>
        {instanceSettings.length > 0 && onConfigChange ? (
          <WidgetInstanceSettings
            open={showSettings}
            title={widgetDisplayLabel(
              widgetType,
              t.widgets as unknown as Record<string, unknown>,
            )}
            settings={instanceSettings}
            value={(widget.config || {}) as Record<string, unknown>}
            anchor={settingsAnchor}
            ignoreRef={stickerItemRef}
            onClose={() => setShowSettings(false)}
            onSave={(next) => {
              onConfigChange(next)
              setShowSettings(false)
            }}
          />
        ) : null}
      </motion.div>
    )
  },
  (prev, next) => {
    return (
      prev.widget === next.widget &&
      prev.isEditMode === next.isEditMode &&
      prev.isPreview === next.isPreview &&
      prev.isHeld === next.isHeld &&
      prev.isCovered === next.isCovered &&
      prev.isHovered === next.isHovered &&
      prev.widgetType === next.widgetType &&
      prev.gridWidth === next.gridWidth &&
      prev.gridHeight === next.gridHeight &&
      prev.layoutMotion === next.layoutMotion &&
      prev.index === next.index &&
      prev.onRequestSticker === next.onRequestSticker &&
      prev.allowSticker === next.allowSticker
    )
  },
)

interface WidgetGridProps {
  widgets: WidgetConfig[]
  availableWidgets: WidgetType[]
  onWidgetsChange?: (widgets: WidgetConfig[]) => void
  isEditMode: boolean
  children?: React.ReactNode
  customGridColumns?: number // Optional prop to override responsive grid columns
  customGridRows?: number // Optional prop to override default grid rows
  autoHeight?: boolean
  /** Home only: free layout fills the stage with standard cell size. */
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
  /** Home tour only. Control-panel grids must not set this. */
  tourAnchor?: string
  tourFit?: string
}

const STICKER_WIDGET_TYPE: WidgetType = {
  id: 'sticker',
  name: 'Sticker',
  defaultSize: '2x2',
  component: StickerWidget,
}

/**
 * 检查小组件位置是否与其他小组件冲突
 */
function checkCollision(
  widget: WidgetConfig,
  allWidgets: WidgetConfig[],
  gridWidth: number,
  gridHeight: number,
  excludeId?: string,
): boolean {
  const dim = widgetSizeSpan(widget.size)
  const { x, y } = widget.position

  // 检查是否超出边界
  if (x < 0 || y < 0 || x + dim.w > gridWidth || y + dim.h > gridHeight) {
    return true
  }

  // 检查与其他小组件的重叠
  for (const other of allWidgets) {
    if (other.id === excludeId || other.id === widget.id) continue

    const otherDim = widgetSizeSpan(other.size)
    const { x: ox, y: oy } = other.position

    // AABB 碰撞检测
    if (
      x < ox + otherDim.w &&
      x + dim.w > ox &&
      y < oy + otherDim.h &&
      y + dim.h > oy
    ) {
      return true
    }
  }

  return false
}

const WidgetDragGhost = React.memo(({
  active,
  settling,
  exiting,
  reducedMotion,
  dragPreview,
  gridWidth,
  gridHeight,
  gridRectRef,
}: {
  active: boolean
  settling: boolean
  exiting: boolean
  reducedMotion: boolean
  dragPreview: {
    size: { w: number; h: number }
    hasCollision: boolean
    position?: { x: number; y: number } | null
    settleCell?: { x: number; y: number } | null
    fromLibrary?: boolean
    padded?: boolean
    widgetType?: WidgetType
    widgetConfig?: WidgetConfig
  } | null
  gridWidth: number
  gridHeight: number
  gridRectRef: React.RefObject<DOMRect | null>
}) => {
  const pos = useWidgetDragCursor()
  const [settleLanded, setSettleLanded] = useState(false)
  useLayoutEffect(() => {
    if (!settling || reducedMotion) {
      setSettleLanded(settling)
      return
    }
    setSettleLanded(false)
    const frame = requestAnimationFrame(() => setSettleLanded(true))
    return () => cancelAnimationFrame(frame)
  }, [reducedMotion, settling])

  if (
    !active ||
    !pos ||
    !dragPreview?.widgetType ||
    !dragPreview.widgetConfig
  ) {
    return null
  }
  const gridRect = gridRectRef.current
  const cellWidth = gridRect ? gridRect.width / gridWidth : 100
  const cellHeight = gridRect ? gridRect.height / gridHeight : 100
  const padPx = dragPreview.padded === false ? 0 : GRID_WIDGET_PAD_PX
  const settleBox =
    settling &&
    settleLanded &&
    dragPreview.settleCell &&
    gridRect
      ? widgetDragGhostBox({
          gridRect,
          cell: dragPreview.settleCell,
          size: dragPreview.size,
          gridWidth,
          gridHeight,
          padPx,
        })
      : null
  const floating = dragGhostContentSize(
    cellWidth,
    cellHeight,
    dragPreview.size,
    padPx,
  )
  const left = settleBox?.x ?? pos.x
  const top = settleBox?.y ?? pos.y
  const width = settleBox?.width ?? floating.width
  const height = settleBox?.height ?? floating.height
  const WidgetComponent = dragPreview.widgetType.component
  const tileState = dragPreview.hasCollision
    ? 'is-blocked'
    : exiting
      ? 'is-exiting'
      : settling
        ? 'is-settling'
        : 'is-floating'
  return createPortal(
    <div
      className={`widget-grid-drag-ghost${
        settling ? ' is-settling' : ''
      }${exiting ? ' is-exiting' : ''}`}
      style={{ left, top }}
    >
      <div
        className={`widget-grid-drag-ghost-tile ${tileState}`}
        style={{ width, height }}
      >
        <Suspense fallback={null}>
          <WidgetComponent
            config={dragPreview.widgetConfig}
            isEditMode={false}
            isPreview={true}
          />
        </Suspense>
      </div>
    </div>,
    document.body,
  )
})
WidgetDragGhost.displayName = 'WidgetDragGhost'

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
  const [stickerDrag, setStickerDrag] = useState<{
    start: { x: number; y: number }
    end: { x: number; y: number }
  } | null>(null)
  const [stickerHover, setStickerHover] = useState<{
    x: number
    y: number
  } | null>(null)
  const [gridColumns, setGridColumns] = useState(() =>
    readInitialHomeGridColumns(customGridColumns),
  )
  const isCompact =
    !isFreeLayout && !customGridColumns && gridColumns < GRID_WIDTH
  const containerRef = useRef<HTMLDivElement | null>(null)
  // 缓存 gridRect 避免频繁调用 getBoundingClientRect
  const gridRectRef = useRef<DOMRect | null>(null)
  /** Container width for height = width * rows/cols (not for item geometry). */
  const [containerWidth, setContainerWidth] = useState(0)
  /** Track applied column band for hysteresis + morph. */
  const prevColumnsRef = useRef(gridColumns)
  const bandSwitchingRef = useRef(false)
  /** After first successful apply; only then allow tablet↔desktop fade. */
  const bandSettledOnceRef = useRef(false)
  const bandTimersRef = useRef<{ out?: number; in?: number; raf?: number }>({})
  /**
   * null = settled; 'out' | 'in' = cross-band fade (no geometry lerp).
   */
  const [bandSwitch, setBandSwitch] = useState<'out' | 'in' | null>(null)
  const anim = useAnimationLevel()
  /**
   * Same-band only: drag/resize polish. Cross-band uses opacity crossfade —
   * never interpolate compact packing ↔ desktop saved coords.
   */
  const [motionMode, setMotionMode] = useState(layoutMode)
  /** 切模式后两帧内关掉几何缓动，避免标准 16×4 与自由 16×8 互插。 */
  const geometryMotion =
    !isExlight(anim) &&
    bandSwitch === null &&
    motionMode === layoutMode

  // 计算内容高度 (用于 autoHeight)
  const contentHeight = useMemo(() => {
    if (!autoHeight) return 0
    let maxY = 0
    widgets.forEach((w) => {
      const dim = widgetSizeSpan(w.size)
      maxY = Math.max(maxY, w.position.y + dim.h)
    })
    return maxY
  }, [widgets, autoHeight])

  // 响应式列档：防抖宽度 + 迟滞；tablet↔desktop 可淡入淡出；含 phone 则硬切
  // 控制面板固定 12 列，不必为窗口 resize 重绘整棵网格。
  const { width: windowWidth } = useDebouncedWindowSize(
    150,
    !isFreeLayout && !customGridColumns,
  )
  const animHardCut = isExlight(anim)

  useLayoutEffect(() => {
    if (motionMode === layoutMode) return
    let second = 0
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => setMotionMode(layoutMode))
    })
    return () => {
      cancelAnimationFrame(first)
      cancelAnimationFrame(second)
    }
  }, [layoutMode, motionMode])

  useEffect(() => {
    const clearBandTimers = () => {
      if (bandTimersRef.current.out) {
        clearTimeout(bandTimersRef.current.out)
        bandTimersRef.current.out = undefined
      }
      if (bandTimersRef.current.in) {
        clearTimeout(bandTimersRef.current.in)
        bandTimersRef.current.in = undefined
      }
      if (bandTimersRef.current.raf) {
        cancelAnimationFrame(bandTimersRef.current.raf)
        bandTimersRef.current.raf = undefined
      }
    }

    if (isFreeLayout || customGridColumns) {
      clearBandTimers()
      bandSwitchingRef.current = false
      setBandSwitch(null)
      if (customGridColumns) {
        setGridColumns(customGridColumns)
        prevColumnsRef.current = customGridColumns
      }
      bandSettledOnceRef.current = true
      return
    }

    const readDesired = () =>
      resolveHomeGridColumns(
        typeof window !== 'undefined' ? window.innerWidth : windowWidth,
        prevColumnsRef.current,
      )

    const hardApply = (cols: number) => {
      clearBandTimers()
      bandSwitchingRef.current = false
      setBandSwitch(null)
      prevColumnsRef.current = cols
      setGridColumns(cols)
      bandSettledOnceRef.current = true
    }

    const tryBandMorph = () => {
      const desired = readDesired()
      if (desired === prevColumnsRef.current) {
        bandSettledOnceRef.current = true
        return
      }
      if (bandSwitchingRef.current) return

      const from = prevColumnsRef.current
      // Snap without fade: first paint, exlight, or any transition involving phone
      // (DevTools mobile width must never sit on a half-faded 16-col layout).
      const mustHardCut =
        animHardCut ||
        !bandSettledOnceRef.current ||
        from === 4 ||
        desired === 4

      if (mustHardCut) {
        hardApply(desired)
        return
      }

      // tablet (8) ↔ desktop (16) only: short opacity crossfade
      bandSwitchingRef.current = true
      setBandSwitch('out')
      clearBandTimers()

      bandTimersRef.current.out = window.setTimeout(() => {
        const target = readDesired()
        prevColumnsRef.current = target
        setGridColumns(target)
        setBandSwitch('in')

        bandTimersRef.current.in = window.setTimeout(() => {
          setBandSwitch(null)
          bandSwitchingRef.current = false
          bandSettledOnceRef.current = true
          bandTimersRef.current.raf = requestAnimationFrame(() => {
            bandTimersRef.current.raf = undefined
            tryBandMorph()
          })
        }, GRID_BAND_IN_MS)
      }, GRID_BAND_OUT_MS)
    }

    tryBandMorph()
    return clearBandTimers
  }, [windowWidth, customGridColumns, animHardCut, isFreeLayout])

  // Unmount only: drop pending band morph timers
  useEffect(() => {
    return () => {
      if (bandTimersRef.current.out) clearTimeout(bandTimersRef.current.out)
      if (bandTimersRef.current.in) clearTimeout(bandTimersRef.current.in)
      if (bandTimersRef.current.raf) {
        cancelAnimationFrame(bandTimersRef.current.raf)
      }
      bandSwitchingRef.current = false
    }
  }, [])

  // 紧凑模式布局计算 (自动重排)
  const compactLayout = useMemo(() => {
    if (!isCompact) return null
    return packWidgetsIntoColumns(widgets, gridColumns)
  }, [widgets, isCompact, gridColumns])

  const freeGrid = isFreeLayout
    ? { cols: HOME_STANDARD_COLS, rows: HOME_FREE_ROWS }
    : null

  const currentWidgets =
    isCompact && compactLayout ? compactLayout.widgets : widgets
  const currentGridWidth = freeGrid?.cols ?? gridColumns
  const currentGridHeight = freeGrid
    ? freeGrid.rows
    : isCompact && compactLayout
      ? compactLayout.height
      : autoHeight
        ? Math.max(customGridRows || 0, contentHeight)
        : customGridRows || GRID_HEIGHT

  const stickerDragRect = stickerDrag
    ? {
        x: Math.min(stickerDrag.start.x, stickerDrag.end.x),
        y: Math.min(stickerDrag.start.y, stickerDrag.end.y),
        w: Math.abs(stickerDrag.end.x - stickerDrag.start.x) + 1,
        h: Math.abs(stickerDrag.end.y - stickerDrag.start.y) + 1,
      }
    : null
  const stickerDragPlacement = stickerDragRect
    ? placeHomeStickerSelection(
        stickerDragRect.x,
        stickerDragRect.y,
        stickerDragRect.w,
        stickerDragRect.h,
      )
    : null
  const stickerDragCollision = Boolean(
    stickerDragPlacement &&
      checkCollision(
        {
          id: '__sticker-pick__',
          type: 'sticker',
          kind: 'sticker',
          size: stickerDragPlacement.size,
          position: {
            x: stickerDragPlacement.x,
            y: stickerDragPlacement.y,
          },
        },
        widgets,
        currentGridWidth,
        currentGridHeight,
      ),
  )


  // Explicit height from cols/rows. Cross-band: snap (no height transition).
  // Free layout sizes the plate to N×cell instead of stretching with the host.
  const gridPixelHeight =
    freeGrid || containerWidth <= 0
      ? undefined
      : (containerWidth * currentGridHeight) / currentGridWidth

  /*
   * 高度过渡只表达「行数变了」，不表达「窗口宽度变了」。
   *
   * 高度是从 containerWidth 算出来的内联 px，缩放窗口时它每帧都在变；
   * 过渡它意味着 450ms 内每帧重排全部小组件，进而反复唤醒各 widget 的
   * ResizeObserver（useWidgetSize 重渲染 + FitText 整轮强制重排重测）。
   * 行数变化是离散事件，才值得缓动。
   */
  const [rowCountMorphing, setRowCountMorphing] = useState(false)
  const prevRowCountRef = useRef(currentGridHeight)
  useEffect(() => {
    if (prevRowCountRef.current === currentGridHeight) return
    prevRowCountRef.current = currentGridHeight
    setRowCountMorphing(true)
    const timer = window.setTimeout(setRowCountMorphing, 500, false)
    return () => window.clearTimeout(timer)
  }, [currentGridHeight])

  const [draggedWidget, setDraggedWidget] = useState<WidgetDragSession | null>(
    null,
  )
  const [dragSettling, setDragSettling] = useState(false)
  const [previewUncovered, setPreviewUncovered] = useState(false)
  const [previewExiting, setPreviewExiting] = useState(false)
  const settleStartedAtRef = useRef(0)
  const [resizingWidget, setResizingWidget] = useState<{
    widgetId: string
    startPos: { x: number; y: number }
    startSize: WidgetSize
    draftSize: WidgetSize
    direction?: 'se' | 's'
  } | null>(null)
  const [hoveredCell, setHoveredCell] = useState<{
    x: number
    y: number
  } | null>(null)
  const [widgetHistory, setWidgetHistory] = useState<WidgetConfig[][]>([])
  const [historyIndex, setHistoryIndex] = useState(-1)
  const [hoveredWidgetId, setHoveredWidgetId] = useState<string | null>(null)

  /*
   * id → WidgetType 索引。
   * 此前每处都 `availableWidgets.find(...)`：渲染循环里每个格子一次，
   * 拖拽/缩放的 rAF 回调里每帧一次，而 availableWidgets 含全部 Tapp 小组件。
   */
  const widgetTypeById = useMemo(() => {
    const map = new Map<string, WidgetType>()
    for (const widgetType of availableWidgets)
      map.set(widgetType.id, widgetType)
    map.set(STICKER_WIDGET_TYPE.id, STICKER_WIDGET_TYPE)
    return map
  }, [availableWidgets])

  // RAF ref for drag handling
  const rafRef = useRef<number | null>(null)

  /*
   * 最新状态镜像。
   *
   * WidgetGridItem 的 memo 比较函数刻意不比回调（比了就等于不 memo），
   * 于是被拦下的格子会一直握着**首次通过比较那一帧**的回调闭包。
   * 若回调直接闭包 widgets，就会读到过期数组——改配置或删除某个格子时，
   * 会把此后新增的小组件一并抹掉。
   *
   * 因此下面所有传给 item 的回调都必须：引用恒定 + 从这里读最新值。
   * （文件里 handleDragMoveRef 等已是同一约定。）
   */
  const latestRef = useRef({
    widgets,
    onWidgetsChange,
    widgetHistory,
    historyIndex,
    isFreeLayout,
    widgetTypeById,
    currentGridWidth: GRID_WIDTH,
    currentGridHeight: GRID_HEIGHT,
  })
  latestRef.current = {
    widgets,
    onWidgetsChange,
    widgetHistory,
    historyIndex,
    isFreeLayout,
    widgetTypeById,
    currentGridWidth,
    currentGridHeight,
  }

  // 保存到历史记录
  const saveToHistory = useCallback((newWidgets: WidgetConfig[]) => {
    const { widgetHistory: history, historyIndex: index } = latestRef.current
    const newHistory = history.slice(0, index + 1)
    newHistory.push(newWidgets)
    // 限制历史记录数量为20
    if (newHistory.length > 20) {
      newHistory.shift()
    } else {
      setHistoryIndex(index + 1)
    }
    setWidgetHistory(newHistory)
  }, [])

  // 更新 gridRect 缓存（在拖拽开始时调用）
  const updateGridRectCache = useCallback(() => {
    if (containerRef.current) {
      gridRectRef.current = containerRef.current.getBoundingClientRect()
    }
  }, [])

  useImperativeHandle(
    ref,
    () => ({
      startNewWidgetDrag(widgetTypeId, point) {
        const latest = latestRef.current
        const widgetType = latest.widgetTypeById.get(widgetTypeId)
        if (latest.isFreeLayout) {
          const extra = homeWidgetCellCount(widgetType?.defaultSize ?? '2x2')
          if (
            !freeLayoutFitsCellBudget(
              homeWidgetsOccupiedCells(latest.widgets),
              extra,
            )
          ) {
            return
          }
        }
        updateGridRectCache()
        setDragSettling(false)
        setPreviewUncovered(false)
        setPreviewExiting(false)
        setWidgetDragCursor(point)
        setDraggedWidget({
          type: 'new',
          widgetTypeId,
        })
        const gridRect = gridRectRef.current
        if (!gridRect || !widgetType) return
        const size = widgetSizeSpan(widgetType.defaultSize)
        setHoveredCell(
          gridCellFromPoint({
            point,
            gridRect,
            gridWidth: latest.currentGridWidth,
            gridHeight: latest.currentGridHeight,
            size,
          }),
        )
      },
    }),
    [updateGridRectCache],
  )

  // 🆕 使用首页原子化 ResizeObserver
  const { observeHomeResize, unobserveHomeResize } = useHomeResizeObserver()

  // 监听网格容器尺寸（拖拽 hit-test 用）；布局不再依赖像素 cell 宽高
  const gridRef = useCallback(
    (node: HTMLDivElement | null) => {
      // 清理旧的 observer
      if (containerRef.current) {
        unobserveHomeResize(containerRef.current)
      }

      containerRef.current = node
      if (node) {
        observeHomeResize(node, (entry) => {
          setContainerWidth(entry.contentRect.width)
          gridRectRef.current = node.getBoundingClientRect()
        })
        // Seed width immediately so first paint has height
        setContainerWidth(node.getBoundingClientRect().width)
      }
    },
    [observeHomeResize, unobserveHomeResize],
  )


  useEffect(() => {
    if (!stickerDrag) return
    const onMove = (event: MouseEvent) => {
      const rect = gridRectRef.current
      if (!rect || rect.width <= 0 || rect.height <= 0) return
      const x = Math.max(
        0,
        Math.min(
          currentGridWidth - 1,
          Math.floor(((event.clientX - rect.left) / rect.width) * currentGridWidth),
        ),
      )
      const y = Math.max(
        0,
        Math.min(
          currentGridHeight - 1,
          Math.floor(((event.clientY - rect.top) / rect.height) * currentGridHeight),
        ),
      )
      setStickerDrag((prev) =>
        prev && (prev.end.x !== x || prev.end.y !== y)
          ? { ...prev, end: { x, y } }
          : prev,
      )
    }
    const onUp = () => {
      setStickerDrag((prev) => {
        if (!prev) return null
        const x = Math.min(prev.start.x, prev.end.x)
        const y = Math.min(prev.start.y, prev.end.y)
        const w = Math.abs(prev.end.x - prev.start.x) + 1
        const h = Math.abs(prev.end.y - prev.start.y) + 1
        const placed = placeHomeStickerSelection(x, y, w, h)
        const candidate = {
          id: '__sticker-pick__',
          type: 'sticker',
          kind: 'sticker' as const,
          size: placed.size,
          position: { x: placed.x, y: placed.y },
        }
        if (
          onPickStickerSlot &&
          !checkCollision(candidate, widgets, currentGridWidth, currentGridHeight)
        ) {
          const grid = gridRectRef.current
          const dim = widgetSizeSpan(placed.size)
          const anchor = grid
            ? {
                left:
                  grid.left + (placed.x / currentGridWidth) * grid.width,
                top:
                  grid.top + (placed.y / currentGridHeight) * grid.height,
                width: (dim.w / currentGridWidth) * grid.width,
                height: (dim.h / currentGridHeight) * grid.height,
                right:
                  grid.left +
                  ((placed.x + dim.w) / currentGridWidth) * grid.width,
                bottom:
                  grid.top +
                  ((placed.y + dim.h) / currentGridHeight) * grid.height,
              }
            : {
                left: 0,
                top: 0,
                width: 0,
                height: 0,
                right: 0,
                bottom: 0,
              }
          onPickStickerSlot({
            x: placed.x,
            y: placed.y,
            size: placed.size,
            anchor,
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
    stickerDrag,
    currentGridWidth,
    currentGridHeight,
    widgets,
    onPickStickerSlot,
  ])

  // 清理 ResizeObserver
  useEffect(() => {
    return () => {
      if (containerRef.current) {
        unobserveHomeResize(containerRef.current)
      }
    }
  }, [unobserveHomeResize])

  // 开始拖拽现有小组件
  const handleWidgetDragStart = useCallback(
    (e: React.MouseEvent, widgetId: string) => {
      if (!isEditMode || stickerPickActive) return
      e.stopPropagation()
      e.preventDefault()

      const widget = latestRef.current.widgets.find((w) => w.id === widgetId)
      if (!widget) return

      // 拖拽开始时更新 gridRect 缓存
      updateGridRectCache()

      setDragSettling(false)
      setPreviewUncovered(false)
      setPreviewExiting(false)
      setWidgetDragCursor({ x: e.clientX, y: e.clientY })

      setDraggedWidget({
        type: 'existing',
        widgetId,
      })
      const gridRect = gridRectRef.current
      if (gridRect) {
        const latest = latestRef.current
        setHoveredCell(
          gridCellFromPoint({
            point: { x: e.clientX, y: e.clientY },
            gridRect,
            gridWidth: latest.currentGridWidth,
            gridHeight: latest.currentGridHeight,
            size: widgetSizeSpan(widget.size),
          }),
        )
      }
    },
    [isEditMode, stickerPickActive, updateGridRectCache],
  )

  // 开始调整大小
  const handleResizeStart = useCallback(
    (
      e: React.MouseEvent | React.TouchEvent,
      widgetId: string,
      direction: 'se' | 's' = 'se',
    ) => {
      if (!isEditMode || stickerPickActive) return
      e.stopPropagation()
      e.preventDefault()

      const widget = latestRef.current.widgets.find((w) => w.id === widgetId)
      if (!widget) return

      // 调整大小开始时更新 gridRect 缓存
      updateGridRectCache()

      // 获取初始位置（支持鼠标和触控）
      const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
      const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY

      setResizingWidget({
        widgetId,
        startPos: { x: clientX, y: clientY },
        startSize: widget.size,
        draftSize: widget.size,
        direction,
      })
    },
    [isEditMode, stickerPickActive, updateGridRectCache],
  )

  // 调整大小移动
  const handleResizeMove = useCallback(
    (e: MouseEvent | TouchEvent) => {
      if (!resizingWidget) return

      if (rafRef.current) return

      rafRef.current = requestAnimationFrame(() => {
        // 使用缓存的 gridRect，避免在 RAF 回调中调用 getBoundingClientRect
        const gridRect = gridRectRef.current
        if (!gridRect) {
          rafRef.current = null
          return
        }

        const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
        const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY

        const cellWidth = gridRect.width / currentGridWidth
        const cellHeight = gridRect.height / currentGridHeight

        const widget = widgets.find((w) => w.id === resizingWidget.widgetId)
        if (!widget) {
          rafRef.current = null
          return
        }

        // Calculate new dimensions based on mouse position relative to widget top-left
        const widgetLeft = widget.position.x * cellWidth + gridRect.left
        const widgetTop = widget.position.y * cellHeight + gridRect.top

        const newWidthPx = clientX - widgetLeft
        const newHeightPx = clientY - widgetTop

        // Convert to grid units (float)
        let rawW = newWidthPx / cellWidth
        const rawH = newHeightPx / cellHeight

        // 如果是底部调整，锁定宽度
        if (resizingWidget.direction === 's') {
          rawW = widgetSizeSpan(resizingWidget.startSize).w
        }

        // Find closest valid size
        let bestSize = widget.size
        let minDistance = Infinity

        const sticker = isHomeStickerItem(widget)
        const widgetType = widgetTypeById.get(widget.type)
        if (!sticker && !widgetType) {
          rafRef.current = null
          return
        }

        const supportedSizes = sticker
          ? stickerSizesSharingAspect(resizingWidget.startSize)
          : widgetType?.supportedSizes || WIDGET_SIZE_KEYS

        const validSizes = sticker
          ? supportedSizes
          : supportedSizes.filter((size) =>
              WIDGET_SIZE_KEYS.includes(
                size as (typeof WIDGET_SIZE_KEYS)[number],
              ),
            )

        for (const size of validSizes) {
          const dim = widgetSizeSpan(size)

          // 如果是底部调整，只考虑宽度相同的尺寸
          if (
            resizingWidget.direction === 's' &&
            dim.w !== widgetSizeSpan(resizingWidget.startSize).w
          ) {
            continue
          }

          // Calculate Euclidean distance in grid units
          const dist = (dim.w - rawW) ** 2 + (dim.h - rawH) ** 2

          if (dist < minDistance) {
            minDistance = dist
            bestSize = size
          }
        }

        if (bestSize !== resizingWidget.draftSize) {
          const newWidget = { ...widget, size: bestSize }
          const fitsBudget =
            !isFreeLayout ||
            !isHomeWidgetItem(widget) ||
            freeLayoutFitsCellBudget(
              homeWidgetsOccupiedCells(widgets, widget.id),
              homeWidgetCellCount(bestSize),
            )
          if (
            fitsBudget &&
            !checkCollision(
              newWidget,
              widgets,
              currentGridWidth,
              currentGridHeight,
              widget.id,
            )
          ) {
            setResizingWidget((prev) =>
              prev ? { ...prev, draftSize: bestSize as WidgetSize } : prev,
            )
          }
        }

        rafRef.current = null
      })
    },
    [
      resizingWidget,
      widgets,
      currentGridWidth,
      currentGridHeight,
      widgetTypeById,
      isFreeLayout,
    ],
  )

  // 结束调整大小
  const handleResizeEnd = useCallback(() => {
    if (resizingWidget) {
      const committed = widgets.find((w) => w.id === resizingWidget.widgetId)
      if (committed && committed.size !== resizingWidget.draftSize) {
        const next = widgets.map((w) =>
          w.id === resizingWidget.widgetId
            ? { ...w, size: resizingWidget.draftSize }
            : w,
        )
        onWidgetsChange?.(next)
        saveToHistory(next)
      }
      setResizingWidget(null)
    }
    if (rafRef.current) {
      cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }
  }, [resizingWidget, widgets, onWidgetsChange, saveToHistory])

  // 拖拽移动
  const handleDragMove = useCallback(
    (e: MouseEvent | TouchEvent) => {
      if (!draggedWidget) return

      // Use requestAnimationFrame to throttle updates
      if (rafRef.current) {
        return
      }

      rafRef.current = requestAnimationFrame(() => {
        // 使用缓存的 gridRect，避免在 RAF 回调中调用 getBoundingClientRect
        // 注意：如果容器在滚动过程中位置变化，需要在滚动事件中更新缓存
        const gridRect = gridRectRef.current

        if (!gridRect) {
          rafRef.current = null
          return
        }

        // 获取鼠标/触摸位置
        const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
        const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY

        // 更新光标位置（用于渲染跟随光标的预览）
        setWidgetDragCursor({ x: clientX, y: clientY })

        let size: WidgetSize = '1x1'
        if (draggedWidget.type === 'existing' && draggedWidget.widgetId) {
          const widget = widgets.find((w) => w.id === draggedWidget.widgetId)
          size = widget?.size || '1x1'
        } else if (draggedWidget.type === 'new' && draggedWidget.widgetTypeId) {
          const widgetType = widgetTypeById.get(draggedWidget.widgetTypeId)
          size = widgetType?.defaultSize || '1x1'
        }
        const dim = widgetSizeSpan(size)
        const nextCell = gridCellFromPoint({
          point: { x: clientX, y: clientY },
          gridRect,
          gridWidth: currentGridWidth,
          gridHeight: currentGridHeight,
          size: dim,
        })

        setHoveredCell((prev) => {
          if (prev?.x === nextCell.x && prev?.y === nextCell.y) return prev
          return nextCell
        })

        rafRef.current = null
      })
    },
    [
      draggedWidget,
      widgets,
      widgetTypeById,
      currentGridWidth,
      currentGridHeight,
    ],
  )

  // 结束拖拽
  const handleDragEnd = useCallback(() => {
    if (dragSettling) return
    if (rafRef.current) {
      cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }

    if (!draggedWidget || !hoveredCell) {
      setDraggedWidget(null)
      setHoveredCell(null)
      setDragSettling(false)
      setPreviewUncovered(false)
      setPreviewExiting(false)
      setWidgetDragCursor(null)
      return
    }

    if (draggedWidget.type === 'existing' && draggedWidget.widgetId) {
      // 移动现有小组件
      const widget = widgets.find((w) => w.id === draggedWidget.widgetId)
      if (!widget) return

      const newWidget = {
        ...widget,
        position: hoveredCell,
      }

      // 检查碰撞
      // 注意：在移动端模式下，我们可能需要禁用拖拽或者使用不同的碰撞检测逻辑
      // 这里暂时保持原样，但使用 currentWidgets 进行检测可能不准确，因为 currentWidgets 是计算出来的
      // 如果在移动端拖拽，我们应该更新原始 widgets 的顺序？这比较复杂。
      // 建议：移动端禁用编辑模式
      if (
        !checkCollision(
          newWidget,
          widgets,
          currentGridWidth,
          currentGridHeight,
          widget.id,
        )
      ) {
        const updatedWidgets = widgets.map((w) =>
          w.id === widget.id ? newWidget : w,
        )
        onWidgetsChange?.(updatedWidgets)
        saveToHistory(updatedWidgets)
        settleStartedAtRef.current = performance.now()
        setDraggedWidget({
          ...draggedWidget,
          pendingId: widget.id,
          pendingCell: hoveredCell,
        })
        setHoveredCell(null)
        setDragSettling(true)
        return
      }
    } else if (draggedWidget.type === 'new' && draggedWidget.widgetTypeId) {
      // 添加新小组件
      const widgetType = widgetTypeById.get(draggedWidget.widgetTypeId)
      if (!widgetType) return

      const settingsConfig =
        widgetType.settings && widgetType.settings.length > 0
          ? Object.fromEntries(
              widgetType.settings
                .filter((setting) => setting.defaultValue !== undefined)
                .map((setting) => [setting.key, setting.defaultValue]),
            )
          : undefined
      const newWidget: WidgetConfig = {
        id: `widget_${Date.now()}`,
        type: widgetType.id,
        size: widgetType.defaultSize,
        position: hoveredCell,
        config: widgetHostConfig(widgetType.id) ?? settingsConfig,
      }

      const fitsBudget =
        !isFreeLayout ||
        freeLayoutFitsCellBudget(
          homeWidgetsOccupiedCells(widgets),
          homeWidgetCellCount(newWidget.size),
        )
      if (
        fitsBudget &&
        !checkCollision(newWidget, widgets, currentGridWidth, currentGridHeight)
      ) {
        const newWidgets = [...widgets, newWidget]
        onWidgetsChange?.(newWidgets)
        saveToHistory(newWidgets)
        settleStartedAtRef.current = performance.now()
        setDraggedWidget({
          ...draggedWidget,
          pendingId: newWidget.id,
          pendingCell: hoveredCell,
        })
        setHoveredCell(null)
        setDragSettling(true)
        return
      }
    }

    setDraggedWidget(null)
    setHoveredCell(null)
    setDragSettling(false)
    setPreviewUncovered(false)
    setPreviewExiting(false)
    setWidgetDragCursor(null)
  }, [
    draggedWidget,
    dragSettling,
    hoveredCell,
    widgets,
    widgetTypeById,
    onWidgetsChange,
    saveToHistory,
    currentGridWidth,
    currentGridHeight,
    isFreeLayout,
  ])

  // 移除小组件（引用恒定，见 latestRef 注释）
  const handleRemoveWidget = useCallback(
    (widgetId: string) => {
      const { widgets: current, onWidgetsChange: notify } = latestRef.current
      const newWidgets = current.filter((w) => w.id !== widgetId)
      notify?.(newWidgets)
      // 添加到历史记录
      saveToHistory(newWidgets)
    },
    [saveToHistory],
  )

  // 单个小组件的配置变更（引用恒定，见 latestRef 注释）
  const handleWidgetConfigChange = useCallback(
    (widgetId: string, newConfig: any) => {
      const { widgets: current, onWidgetsChange: notify } = latestRef.current
      const newWidgets = current.map((w) =>
        w.id === widgetId ? { ...w, config: newConfig } : w,
      )
      notify?.(newWidgets)
      saveToHistory(newWidgets)
    },
    [saveToHistory],
  )

  const handleWidgetMouseLeave = useCallback(() => setHoveredWidgetId(null), [])

  // 撤销功能
  const handleUndo = useCallback(() => {
    if (historyIndex > 0) {
      const prevWidgets = widgetHistory[historyIndex - 1]
      setHistoryIndex(historyIndex - 1)
      onWidgetsChange?.(prevWidgets)
    }
  }, [historyIndex, widgetHistory, onWidgetsChange])

  // 重做功能
  const handleRedo = useCallback(() => {
    if (historyIndex < widgetHistory.length - 1) {
      const nextWidgets = widgetHistory[historyIndex + 1]
      setHistoryIndex(historyIndex + 1)
      onWidgetsChange?.(nextWidgets)
    }
  }, [historyIndex, widgetHistory, onWidgetsChange])

  // 使用 ref 存储事件处理函数，避免每次状态变化时重新添加/移除事件监听器
  const handleDragMoveRef = useRef(handleDragMove)
  const handleDragEndRef = useRef(handleDragEnd)
  const handleResizeMoveRef = useRef(handleResizeMove)
  const handleResizeEndRef = useRef(handleResizeEnd)
  handleDragMoveRef.current = handleDragMove
  handleDragEndRef.current = handleDragEnd
  handleResizeMoveRef.current = handleResizeMove
  handleResizeEndRef.current = handleResizeEnd

  // 注册拖拽事件（鼠标和触屏）- 使用 ref 避免频繁重建监听器
  // 关键优化: 移动端禁用编辑模式,避免 passive: false 破坏滚动性能
  useEffect(() => {
    if (draggedWidget && !dragSettling) {
      const isMobile = getIsMobile()
      const moveHandler = (e: MouseEvent | TouchEvent) =>
        handleDragMoveRef.current(e)
      const endHandler = () => handleDragEndRef.current()

      window.addEventListener('mousemove', moveHandler)
      window.addEventListener('mouseup', endHandler)

      // 移动端使用 passive: true 避免阻塞滚动
      // 这意味着在移动端拖拽时无法调用 preventDefault,但保证了滚动流畅性
      if (isMobile) {
        window.addEventListener('touchmove', moveHandler, { passive: true })
      } else {
        window.addEventListener('touchmove', moveHandler, { passive: false })
      }
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
    }
  }, [draggedWidget, dragSettling])

  const wasEditModeRef = useRef(isEditMode)
  useEffect(() => {
    const wasEditMode = wasEditModeRef.current
    wasEditModeRef.current = isEditMode
    if (!wasEditMode || isEditMode) return
    setDraggedWidget(null)
    setHoveredCell(null)
    setDragSettling(false)
    setPreviewUncovered(false)
    setPreviewExiting(false)
    setWidgetDragCursor(null)
  }, [isEditMode])

  const handoffId = draggedWidget?.pendingId
  const handoffCommitted = placementHasCommitted(widgets, draggedWidget)
  useEffect(() => {
    if (!dragSettling || !handoffId || !handoffCommitted) return
    const reduced =
      isExlight(anim) ||
      window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const delays = dragGhostHandoffDelays(
      reduced,
      performance.now() - settleStartedAtRef.current,
    )
    const uncoverTimer = window.setTimeout(
      () => setPreviewUncovered(true),
      delays.uncoverMs,
    )
    const exitTimer = window.setTimeout(
      () => setPreviewExiting(true),
      delays.exitMs,
    )
    const clearTimer = window.setTimeout(() => {
      setDraggedWidget(null)
      setHoveredCell(null)
      setDragSettling(false)
      setPreviewUncovered(false)
      setPreviewExiting(false)
      setWidgetDragCursor(null)
    }, delays.clearMs)
    return () => {
      window.clearTimeout(uncoverTimer)
      window.clearTimeout(exitTimer)
      window.clearTimeout(clearTimer)
    }
  }, [anim, dragSettling, handoffCommitted, handoffId])

  // 注册调整大小事件 - 使用 ref 避免频繁重建监听器
  // 关键优化: 移动端使用 passive 监听避免阻塞滚动
  useEffect(() => {
    if (resizingWidget) {
      const isMobile = getIsMobile()
      const moveHandler = (e: MouseEvent | TouchEvent) =>
        handleResizeMoveRef.current(e)
      const endHandler = () => handleResizeEndRef.current()

      window.addEventListener('mousemove', moveHandler)
      window.addEventListener('mouseup', endHandler)

      // 移动端使用 passive: true 避免阻塞滚动
      if (isMobile) {
        window.addEventListener('touchmove', moveHandler, { passive: true })
      } else {
        window.addEventListener('touchmove', moveHandler, { passive: false })
      }
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
    }
  }, [resizingWidget]) // 只依赖 resizingWidget 是否存在

  // 键盘快捷键支持（编辑模式）
  useEffect(() => {
    if (!isEditMode) return

    const handleKeyDown = (e: KeyboardEvent) => {
      // Ctrl/Cmd + Z: 撤销
      if ((e.ctrlKey || e.metaKey) && e.key === 'z' && !e.shiftKey) {
        e.preventDefault()
        handleUndo()
      }
      // Ctrl/Cmd + Shift + Z 或 Ctrl/Cmd + Y: 重做
      if (
        (e.ctrlKey || e.metaKey) &&
        ((e.shiftKey && e.key === 'z') || e.key === 'y')
      ) {
        e.preventDefault()
        handleRedo()
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [handleRedo, handleUndo, isEditMode])

  // 预览拖拽位置和组件信息
  const dragPreview = useMemo(() => {
    if (!draggedWidget) return null
    const ghost = resolveDragGhostWidget({
      dragged: draggedWidget,
      widgets,
      widgetTypeById,
    })
    if (!ghost) return null

    const hasCollision = hoveredCell
      ? checkCollision(
          {
            id: 'preview',
            type: ghost.widgetConfig?.type || '',
            size: ghost.widgetConfig?.size || '1x1',
            position: hoveredCell,
          },
          widgets,
          currentGridWidth,
          currentGridHeight,
          draggedWidget.type === 'existing' ? draggedWidget.widgetId : undefined,
        )
      : false

    return {
      position: hoveredCell,
      settleCell: draggedWidget.pendingCell ?? hoveredCell ?? null,
      size: ghost.size,
      hasCollision,
      fromLibrary: ghost.fromLibrary,
      padded: ghost.widgetConfig
        ? !isHomeStickerItem(ghost.widgetConfig)
        : true,
      widgetType: ghost.widgetType,
      widgetConfig: ghost.widgetConfig,
    }
  }, [
    draggedWidget,
    hoveredCell,
    widgets,
    widgetTypeById,
    currentGridWidth,
    currentGridHeight,
  ])

  // 网格背景线：单个盒子 + repeating gradient（细节见 WidgetGrid.css）
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
      {/* 网格区域 */}
      <div
        className={`relative w-full flex flex-col min-h-0 ${
          isCompact
            ? 'justify-start pb-20'
            : isFreeLayout
              ? 'flex-1 items-center justify-center'
              : 'flex-1 justify-end'
        }`}
      >
        {/* 插入 children (InfoBar) */}
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
            {/* 背景网格线（编辑模式） */}
            {isEditMode && !isCompact && gridBackground}

            {/* 拖拽位置指示器 - 网格中的目标位置预览 */}
            {dragPreview?.position && !isCompact && (
              <div
                className={`widget-grid-drop-slot${
                  dragPreview.hasCollision ? ' is-blocked' : ''
                }`}
                style={{
                  left: `${(dragPreview.position.x / currentGridWidth) * 100}%`,
                  top: `${(dragPreview.position.y / currentGridHeight) * 100}%`,
                  width: `${(dragPreview.size.w / currentGridWidth) * 100}%`,
                  height: `${(dragPreview.size.h / currentGridHeight) * 100}%`,
                }}
              >
                {dragPreview.hasCollision ? (
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
                onMouseMove={(event) => {
                  if (stickerDrag) return
                  const node = containerRef.current
                  if (!node) return
                  const rect = node.getBoundingClientRect()
                  if (rect.width <= 0 || rect.height <= 0) return
                  const x = Math.max(
                    0,
                    Math.min(
                      currentGridWidth - 1,
                      Math.floor(
                        ((event.clientX - rect.left) / rect.width) *
                          currentGridWidth,
                      ),
                    ),
                  )
                  const y = Math.max(
                    0,
                    Math.min(
                      currentGridHeight - 1,
                      Math.floor(
                        ((event.clientY - rect.top) / rect.height) *
                          currentGridHeight,
                      ),
                    ),
                  )
                  setStickerHover((prev) =>
                    prev && prev.x === x && prev.y === y ? prev : { x, y },
                  )
                }}
                onMouseLeave={() => setStickerHover(null)}
                onMouseDown={(event) => {
                  event.preventDefault()
                  event.stopPropagation()
                  const node = containerRef.current
                  if (!node) return
                  const rect = node.getBoundingClientRect()
                  gridRectRef.current = rect
                  if (rect.width <= 0 || rect.height <= 0) return
                  const x = Math.max(
                    0,
                    Math.min(
                      currentGridWidth - 1,
                      Math.floor(
                        ((event.clientX - rect.left) / rect.width) *
                          currentGridWidth,
                      ),
                    ),
                  )
                  const y = Math.max(
                    0,
                    Math.min(
                      currentGridHeight - 1,
                      Math.floor(
                        ((event.clientY - rect.top) / rect.height) *
                          currentGridHeight,
                      ),
                    ),
                  )
                  setStickerDrag({ start: { x, y }, end: { x, y } })
                }}
              />
            ) : null}
            {stickerPickActive &&
            stickerHover &&
            !stickerDrag &&
            currentGridWidth > 0 ? (
              <div
                className="absolute z-20 pointer-events-none rounded-lg bg-[color-mix(in_srgb,var(--color-primary,#8b5cf6)_22%,transparent)] ring-2 ring-[color-mix(in_srgb,var(--color-primary,#8b5cf6)_70%,white)]"
                style={{
                  left: `${(stickerHover.x / currentGridWidth) * 100}%`,
                  top: `${(stickerHover.y / currentGridHeight) * 100}%`,
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
            {stickerDrag && stickerDragRect && currentGridWidth > 0 ? (
              <div
                className={`absolute z-20 pointer-events-none rounded-xl ${
                  stickerDragCollision
                    ? 'bg-red-500/10 ring-2 ring-red-500/50'
                    : 'bg-[color-mix(in_srgb,var(--color-primary,#8b5cf6)_18%,transparent)] ring-2 ring-[color-mix(in_srgb,var(--color-primary,#8b5cf6)_70%,white)]'
                }`}
                style={{
                  left: `${(stickerDragRect.x / currentGridWidth) * 100}%`,
                  top: `${(stickerDragRect.y / currentGridHeight) * 100}%`,
                  width: `${(stickerDragRect.w / currentGridWidth) * 100}%`,
                  height: `${(stickerDragRect.h / currentGridHeight) * 100}%`,
                }}
              />
            ) : null}

            {/* 小组件 */}
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
                  // 未知/未注册组件：渲染轻量占位而非静默跳过（issue #72）。
                  // 此前 return null 导致 Tapp widget 在注册表尚未同步/同步
                  // 失败时整卡空白且无任何提示，用户无法区分"加载中/失败/被
                  // 过滤"；占位至少暴露该格子的 widget 类型，便于诊断。
                  const dim =
                    widgetSizeSpan(widget.size)
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

                // 只闭包 widget.id（对某个格子恒定），实际读写走
                // handleWidgetConfigChange 的 latestRef，因此即便这个箭头
                // 被 memo 冻在旧的一帧，也不会写回过期的 widgets 数组。
                const handleConfigChange = (newConfig: any) =>
                  handleWidgetConfigChange(widget.id, newConfig)

                return (
                  <WidgetGridItem
                    key={widget.id}
                    widget={widget}
                    widgetType={widgetType}
                    isEditMode={isEditMode && !isCompact}
                    isHeld={
                      heldWidgetId({
                        dragged: draggedWidget,
                        settling: dragSettling,
                        uncovered: previewUncovered || previewExiting,
                      }) === widget.id
                    }
                    isCovered={
                      coveringWidgetId({
                        dragged: draggedWidget,
                        settling: dragSettling,
                      }) === widget.id
                    }
                    isHovered={hoveredWidgetId === widget.id}
                    onDragStart={handleWidgetDragStart}
                    onMouseEnter={setHoveredWidgetId}
                    onMouseLeave={handleWidgetMouseLeave}
                    onRemove={handleRemoveWidget}
                    onResizeStart={handleResizeStart}
                    gridWidth={currentGridWidth}
                    gridHeight={currentGridHeight}
                    onConfigChange={handleConfigChange}
                    index={index}
                    layoutMotion={geometryMotion}
                    onRequestSticker={(widget) => {
                      const slot = findEmptyHomeSlot(
                        widgets,
                        widget.size,
                        currentGridWidth,
                        currentGridHeight,
                      )
                      if (slot) {
                        const grid = gridRectRef.current
                        const dim = widgetSizeSpan(widget.size)
                        const gw = currentGridWidth
                        const gh = currentGridHeight
                        const anchor = grid
                          ? {
                              left: grid.left + (slot.x / gw) * grid.width,
                              top: grid.top + (slot.y / gh) * grid.height,
                              width: (dim.w / gw) * grid.width,
                              height: (dim.h / gh) * grid.height,
                              right:
                                grid.left + ((slot.x + dim.w) / gw) * grid.width,
                              bottom:
                                grid.top + ((slot.y + dim.h) / gh) * grid.height,
                            }
                          : {
                              left: 0,
                              top: 0,
                              width: 0,
                              height: 0,
                              right: 0,
                              bottom: 0,
                            }
                        onPickStickerSlot?.({
                          ...slot,
                          size: widget.size,
                          anchor,
                        })
                      }
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
        active={Boolean(draggedWidget)}
        settling={dragSettling}
        exiting={previewExiting}
        reducedMotion={isExlight(anim)}
        dragPreview={dragPreview}
        gridWidth={currentGridWidth}
        gridHeight={currentGridHeight}
        gridRectRef={gridRectRef}
      />
    </div>
  )
},
)

export default WidgetGrid
