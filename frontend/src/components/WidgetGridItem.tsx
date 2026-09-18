import type { StickerCrop } from '../utils/homeStickerCrop'
import type {
  WidgetConfig,
  WidgetDragStart,
  WidgetType,
} from './widgetGridTypes'
import { LuSparkles, LuX } from '@lib/chromeStrokeIcons'
import { motionShim as motion } from '@lib/motionShim'
import React, {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import { useI18n } from '../contexts/I18nContext'
import { useStaggerAnimation } from '../hooks/animation'
import {
  isExlight,
  isStandardAnimation,
  useAnimationLevel,
} from '../hooks/useAnimationLevel'
import {
  HOME_STANDARD_COLS,
  HOME_STANDARD_ROWS,
  isHomeStickerItem,
  isHomeWidgetItem,
} from '../utils/homeLayout'
import {
  defaultStickerCrop,
  parseStickerCrop,
  stickerSlotAspect,
} from '../utils/homeStickerCrop'
import { stickerSizesSharingAspect } from '../utils/homeStickerSize'
import { widgetSizeSpan } from '../utils/widgetSizeScale'
import { RenderErrorBoundary } from './RenderErrorBoundary'
import { widgetDisplayLabel } from './widgetLibraryModel'
import { shouldSkipWidgetEntrance } from './widgetPlacementPreview'
import {
  widgetCrashDetail,
  WidgetCrashFallback,
} from './widgets/shared/WidgetCrashFallback'
import StickerWidget, {
  stickerFloatMode,
  stickerFloatPatch,
} from './widgets/StickerWidget'

const WidgetInstanceSettings = lazy(() =>
  import('./widgets/shared/WidgetInstanceSettings').then((module) => ({
    default: module.WidgetInstanceSettings,
  })),
)
const HomeStickerCrop = lazy(() =>
  import('./home/HomeStickerCrop').then((module) => ({
    default: module.HomeStickerCrop,
  })),
)
const HomeStickerCropTip = lazy(() =>
  import('./home/HomeStickerCropTip').then((module) => ({
    default: module.HomeStickerCropTip,
  })),
)
const WidgetLongPressHint = lazy(() =>
  import('./widgets/shared/WidgetLongPressHint').then((module) => ({
    default: module.WidgetLongPressHint,
  })),
)

export const STICKER_WIDGET_TYPE: WidgetType = {
  id: 'sticker',
  name: 'Sticker',
  defaultSize: '2x2',
  component: StickerWidget,
}

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

export const WidgetGridItem = React.memo(
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
    onDragStart: (start: WidgetDragStart, id: string) => void
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
    index?: number
    layoutMotion?: boolean
    onRequestSticker?: (widget: WidgetConfig) => void
    allowSticker?: boolean
  }) => {
    const anim = useAnimationLevel()
    const { t, format } = useI18n()
    const [showSettings, setShowSettings] = useState(false)
    const [settingsLoaded, setSettingsLoaded] = useState(false)
    const [settingsAnchor, setSettingsAnchor] = useState<DOMRect | null>(null)
    const [stickerCropOpen, setStickerCropOpen] = useState(false)
    const [stickerCropDraft, setStickerCropDraft] =
      useState<StickerCrop>(defaultStickerCrop)
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
    const instanceSettings = widgetType.settings ?? []
    useEffect(() => {
      if (showSettings) setSettingsLoaded(true)
    }, [showSettings])
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

    const animationsEnabled = !isExlight(anim)
    const skipEntrance = shouldSkipWidgetEntrance(isEditMode)
    const { canAnimate, onComplete } = useStaggerAnimation({
      groupId: 'widget-grid',
      index: index || 0,
      baseDelay: 115,
      enabled: animationsEnabled && !skipEntrance,
    })

    const dim = widgetSizeSpan(widget.size)

    const gw = gridWidth || HOME_STANDARD_COLS
    const gh = gridHeight || HOME_STANDARD_ROWS

    // 几何用 %，与列数无关；跨档不插值 left/top。
    const style: React.CSSProperties = {
      left: `${(widget.position.x / gw) * 100}%`,
      top: `${(widget.position.y / gh) * 100}%`,
      width: `${(dim.w / gw) * 100}%`,
      height: `${(dim.h / gh) * 100}%`,
      zIndex: stickerCropOpen || (isHovered && !isCovered) ? 40 : 10,
      // will-change 只写 transform；left/top 是布局属性，写上只会多一层合成层。
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

    const useLiteTransition = !anim.spring || !isStandardAnimation(anim)

    const dragStart = (
      point: { x: number; y: number },
      origin = point,
    ): WidgetDragStart => {
      const rect = stickerItemRef.current?.getBoundingClientRect()
      return {
        point,
        grab: rect
          ? {
              x: Math.max(0, Math.min(1, (origin.x - rect.left) / rect.width)),
              y: Math.max(0, Math.min(1, (origin.y - rect.top) / rect.height)),
            }
          : { x: 0.5, y: 0.5 },
      }
    }

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
              const componentLongPress =
                isEditMode && Boolean(widgetType.componentLongPress)
              if (!holdSticker && !holdSettings && !componentLongPress) {
                event.stopPropagation()
                event.preventDefault()
                onDragStart(
                  dragStart({ x: event.clientX, y: event.clientY }),
                  widget.id,
                )
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
                  dragStart({ x, y }, { x: startX, y: startY }),
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
                if (componentLongPress) return
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
              const componentLongPress = Boolean(widgetType.componentLongPress)
              if (!holdSticker && !holdSettings && !componentLongPress) return
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
                  dragStart({ x, y }, { x: startX, y: startY }),
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
                if (componentLongPress) return
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
            <RenderErrorBoundary
              source="widget"
              resetKey={`${widget.id}:${widget.size}:${isPreview ? 'preview' : 'live'}:${isEditMode ? 'edit' : 'view'}`}
              fallback={({ error, reset }) => (
                <WidgetCrashFallback
                  message={format(t.errors.widgetRenderFailed, {
                    id: widget.type,
                  })}
                  retryLabel={t.common.retry}
                  onRetry={reset}
                  detail={widgetCrashDetail(error)}
                />
              )}
            >
              <WidgetGridItemBody
                widget={widget}
                widgetType={widgetType}
                isEditMode={isEditMode}
                isPreview={isPreview}
                onConfigChange={onConfigChange}
              />
            </RenderErrorBoundary>
            {isEditMode && isHomeStickerItem(widget) && stickerSrc ? (
              <Suspense fallback={null}>
                <WidgetLongPressHint
                  title={t.home.stickerLongPressEdit}
                  visible={!stickerCropOpen}
                  onClick={() => {
                    setStickerCropDraft(
                      parseStickerCrop(widget.config?.crop) ??
                        defaultStickerCrop(),
                    )
                    setStickerCropOpen(true)
                  }}
                />
              </Suspense>
            ) : isEditMode && instanceSettings.length > 0 ? (
              <Suspense fallback={null}>
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
              </Suspense>
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
                {allowSticker &&
                onRequestSticker &&
                isHomeWidgetItem(widget) ? (
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
              <Suspense fallback={null}>
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
                <HomeStickerCropTip
                  open
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
              </Suspense>
            ) : null}
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
        {settingsLoaded && instanceSettings.length > 0 && onConfigChange ? (
          <Suspense fallback={null}>
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
          </Suspense>
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
