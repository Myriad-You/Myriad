/**
 * 可视化编辑的网格小组件系统
 * 16x4 网格布局，支持拖拽编辑
 */

import type { TappSettingItem } from '../tapp/types'

import { FaChevronRight, FaCog, FaSearch, FaTimes } from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import React, {
  Suspense,
  useCallback,
  useEffect,
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
  getStandardWidgetDimensions,
  LIBRARY_PREVIEW_DISPLAY_SCALE,
} from '../hooks/useWidgetSize'
import { resolveHomeGridColumns } from '../utils/viewportBands'
import { widgetTypeMatchesLibrarySearch } from './widgetLibrarySearch'
import { preloadBuiltinWidgets } from './widgets/builtinWidgets'
import './WidgetGrid.css'

// ⚗ 移动端检测 - 使用统一的性能检测系统
function getIsMobile(): boolean {
  return getPerformanceProfileSync().isMobile
}

// 小组件尺寸配置
export type WidgetSize =
  | '1x1'
  | '2x1'
  | '1x2'
  | '2x2'
  | '2x3'
  | '3x2'
  | '3x3'
  | '2x4'
  | '4x1'
  | '4x2'
  | '4x4'

// 小组件配置接口
export interface WidgetConfig {
  id: string
  type: string // 小组件类型标识
  size: WidgetSize
  position: { x: number; y: number } // 网格坐标 (0-15, 0-3)
  config?: any // 小组件特定配置
}

// 小组件组件Props
export interface WidgetComponentProps {
  config: WidgetConfig
  isEditMode: boolean
  isPreview?: boolean
  onConfigChange?: (newConfig: any) => void
}

// 网格尺寸常量（列数阈值见 utils/viewportBands.ts，与主页壳 / Tailwind lg 统一）
const GRID_WIDTH = 16
const GRID_HEIGHT = 4

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

// 尺寸到宽高的映射
const SIZE_TO_DIMENSIONS: Record<WidgetSize, { w: number; h: number }> = {
  '1x1': { w: 1, h: 1 },
  '2x1': { w: 2, h: 1 },
  '1x2': { w: 1, h: 2 },
  '2x2': { w: 2, h: 2 },
  '2x3': { w: 2, h: 3 },
  '3x2': { w: 3, h: 2 },
  '3x3': { w: 3, h: 3 },
  '2x4': { w: 2, h: 4 },
  '4x1': { w: 4, h: 1 },
  '4x2': { w: 4, h: 2 },
  '4x4': { w: 4, h: 4 },
}

function WidgetSettingsDialog({
  title,
  settings,
  value,
  onSave,
  onClose,
}: {
  title: string
  settings: TappSettingItem[]
  value: Record<string, unknown>
  onSave: (value: Record<string, unknown>) => void
  onClose: () => void
}) {
  const { t } = useI18n()
  const [draft, setDraft] = useState<Record<string, unknown>>(() => ({
    ...Object.fromEntries(
      settings
        .filter((setting) => setting.defaultValue !== undefined)
        .map((setting) => [setting.key, setting.defaultValue]),
    ),
    ...value,
  }))

  const update = (key: string, next: unknown) =>
    setDraft((current) => ({ ...current, [key]: next }))

  return createPortal(
    <div
      className="fixed inset-0 z-[10000] flex items-center justify-center bg-black/35 p-4 backdrop-blur-sm"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose()
      }}
    >
      <div
        className="w-full max-w-md rounded-2xl border border-black/10 bg-white p-5 shadow-2xl dark:border-white/10 dark:bg-neutral-900"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="mb-4 text-base font-semibold text-neutral-900 dark:text-white">
          {title}
        </div>
        <div className="max-h-[60vh] space-y-4 overflow-y-auto pr-1">
          {settings.map((setting) => {
            const current = draft[setting.key] ?? setting.defaultValue
            return (
              <label key={setting.key} className="block space-y-1.5">
                <span className="block text-sm font-medium text-neutral-800 dark:text-neutral-200">
                  {setting.label}
                </span>
                {setting.description && (
                  <span className="block text-xs text-neutral-500 dark:text-neutral-400">
                    {setting.description}
                  </span>
                )}
                {setting.type === 'toggle' ? (
                  <input
                    type="checkbox"
                    checked={current === true}
                    onChange={(event) =>
                      update(setting.key, event.target.checked)
                    }
                    className="h-5 w-5 accent-[var(--color-primary)]"
                  />
                ) : setting.type === 'select' ? (
                  <select
                    value={String(current ?? '')}
                    onChange={(event) =>
                      update(setting.key, event.target.value)
                    }
                    className="w-full rounded-lg border border-black/10 bg-white px-3 py-2 text-sm dark:border-white/10 dark:bg-neutral-800"
                  >
                    {setting.options?.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    type={
                      setting.type === 'number'
                        ? 'number'
                        : setting.type === 'color'
                          ? 'color'
                          : 'text'
                    }
                    value={String(current ?? '')}
                    min={setting.min}
                    max={setting.max}
                    step={setting.step}
                    placeholder={setting.placeholder}
                    onChange={(event) =>
                      update(
                        setting.key,
                        setting.type === 'number'
                          ? event.target.value === ''
                            ? null
                            : Number(event.target.value)
                          : event.target.value,
                      )
                    }
                    className={`${setting.type === 'color' ? 'h-10' : 'px-3 py-2'} w-full rounded-lg border border-black/10 bg-white text-sm dark:border-white/10 dark:bg-neutral-800`}
                  />
                )}
              </label>
            )
          })}
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg px-3 py-2 text-sm text-neutral-600 hover:bg-black/5 dark:text-neutral-300 dark:hover:bg-white/10"
          >
            {t.common.cancel}
          </button>
          <button
            type="button"
            onClick={() => onSave(draft)}
            className="rounded-lg bg-[var(--color-primary)] px-4 py-2 text-sm font-medium text-white"
          >
            {t.common.save}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  )
}

// Memoized Widget Item Component
const WidgetGridItem = React.memo(
  ({
    widget,
    widgetType,
    isEditMode,
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
  }: {
    widget: WidgetConfig
    widgetType: WidgetType
    isEditMode: boolean
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
  }) => {
    const anim = useAnimationLevel()
    const { t } = useI18n()
    const [showSettings, setShowSettings] = useState(false)
    const instanceSettings = widgetType.settings || []

    // 使用统一动画协调系统；exlight 模式直接显示且不进入调度队列。
    // Entrance timing intentionally eased after feedback that 80ms / stiff-300 felt too fast.
    const animationsEnabled = !isExlight(anim)
    const { canAnimate, onComplete } = useStaggerAnimation({
      groupId: 'widget-grid',
      index: index || 0,
      baseDelay: 115,
      enabled: animationsEnabled,
    })

    const dim = SIZE_TO_DIMENSIONS[widget.size]
    const WidgetComponent = widgetType.component

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
      zIndex: isHovered ? 20 : 10,
      // 只提示 transform：left/top 是布局属性，will-change 对它们没有
      // 加速作用，写上去只是让编辑模式下每个小组件白白多提升一层合成层。
      willChange: isEditMode ? 'transform' : 'auto',
    }

    // 检查是否支持调整大小
    const canResize =
      !widgetType.supportedSizes || widgetType.supportedSizes.length > 1

    // 低性能模式 / 低端设备：禁用 spring，改用轻量 tween
    const useLiteTransition =
      !anim.spring || !isStandardAnimation(anim)

    return (
      <motion.div
        className={`widget-grid-item absolute ${
          layoutMotion && animationsEnabled
            ? 'widget-grid-item--layout-motion'
            : ''
        }`}
        style={style}
        initial={animationsEnabled ? { opacity: 0, scale: 0.9, y: 14 } : false}
        animate={
          !animationsEnabled || canAnimate
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
        <div className="relative h-full w-full p-1 group">
          <div
            className={`relative h-full w-full rounded-xl overflow-hidden transition-all ${
              isEditMode
                ? 'cursor-move ring-1 ring-transparent hover:ring-blue-400/50'
                : ''
            } ${isHovered && isEditMode ? 'ring-blue-400/50 shadow-lg' : ''}`}
            onMouseDown={(e) => onDragStart(e, widget.id)}
            onMouseEnter={() => isEditMode && onMouseEnter(widget.id)}
            onMouseLeave={onMouseLeave}
          >
            {/* 非报告类 lazy 小组件的 Suspense 兜底；ReportCard 已同步加载不会挂起 */}
            <Suspense fallback={null}>
              <WidgetComponent
                config={widget}
                isEditMode={isEditMode}
                onConfigChange={onConfigChange}
              />
            </Suspense>
          </div>

          {/* 删除按钮（编辑模式） */}
          {isEditMode && (
            <>
              <button
                onClick={(e) => {
                  e.stopPropagation()
                  onRemove(widget.id)
                }}
                className="absolute top-1.5 right-1.5 w-5 h-5 rounded-full bg-red-500/90 hover:bg-red-600 text-white flex items-center justify-center shadow-md z-30 transition-all hover:scale-110 opacity-0 group-hover:opacity-100"
                title={t.widgetGrid.deleteWidget}
                aria-label={t.widgetGrid.deleteWidget}
              >
                <FaTimes size={10} />
              </button>

              {instanceSettings.length > 0 && onConfigChange && (
                <button
                  type="button"
                  onMouseDown={(event) => event.stopPropagation()}
                  onClick={(event) => {
                    event.stopPropagation()
                    setShowSettings(true)
                  }}
                  className="absolute top-1.5 right-8 flex h-5 w-5 items-center justify-center rounded-full bg-neutral-700/85 text-white opacity-0 shadow-md transition-all hover:scale-110 hover:bg-neutral-800 group-hover:opacity-100 z-30"
                  title={t.widgetGrid.widgetSettings}
                  aria-label={t.widgetGrid.widgetSettings}
                >
                  <FaCog size={10} />
                </button>
              )}

              {/* 调整大小手柄 - 明显的倒L型设计，触控时区域更大 */}
              {canResize && (
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
              )}
            </>
          )}
        </div>
        {showSettings && instanceSettings.length > 0 && onConfigChange && (
          <WidgetSettingsDialog
            title={`${widgetType.name} · ${t.widgetGrid.widgetSettings}`}
            settings={instanceSettings}
            value={(widget.config || {}) as Record<string, unknown>}
            onClose={() => setShowSettings(false)}
            onSave={(next) => {
              onConfigChange(next)
              setShowSettings(false)
            }}
          />
        )}
      </motion.div>
    )
  },
  (prev, next) => {
    return (
      prev.widget === next.widget &&
      prev.isEditMode === next.isEditMode &&
      prev.isHovered === next.isHovered &&
      prev.widgetType === next.widgetType &&
      prev.gridWidth === next.gridWidth &&
      prev.gridHeight === next.gridHeight &&
      prev.layoutMotion === next.layoutMotion &&
      prev.index === next.index
    )
  },
)

/**
 * 库条带的按需预览槽。
 *
 * 此前一进编辑模式就把目录里**全部**小组件（22 个内置 + 所有 Tapp）
 * 的真实实现同时挂载：每个都带 `.glass` 的 backdrop-filter、光晕的
 * blur(24~64px)，外层还套了 scale(0.65)（缩放的模糊层要重新光栅化），
 * 屏幕外的那些也照样在合成。
 *
 * 现在只有滚动到附近时才挂真实组件；挂上之后不再卸载——来回滚动时
 * 反复卸载/重挂会让预览闪烁，且预览本身没有持续开销（数据请求都被
 * isPreview 挡掉了）。
 */
const LibraryPreviewSlot = React.memo(
  ({
    scrollRef,
    renderWidth,
    renderHeight,
    displayScale,
    children,
  }: {
    scrollRef: React.RefObject<HTMLDivElement | null>
    renderWidth: number
    renderHeight: number
    displayScale: number
    children: React.ReactNode
  }) => {
    const [mounted, setMounted] = useState(false)
    const slotRef = useRef<HTMLDivElement | null>(null)

    useEffect(() => {
      if (mounted) return
      const node = slotRef.current
      if (!node) return

      // 不支持 IO 的环境退回「立即挂载」，行为与改造前一致
      if (typeof IntersectionObserver === 'undefined') {
        setMounted(true)
        return
      }

      const observer = new IntersectionObserver(
        (entries) => {
          if (entries.some((entry) => entry.isIntersecting)) {
            setMounted(true)
            observer.disconnect()
          }
        },
        {
          root: scrollRef.current ?? null,
          // 提前一屏挂载，滚动时不会看到空框
          rootMargin: '0px 320px',
        },
      )
      observer.observe(node)
      return () => observer.disconnect()
    }, [mounted, scrollRef])

    return (
      <div
        ref={slotRef}
        className="absolute top-0 left-0 origin-top-left pointer-events-none shadow-sm rounded-xl overflow-hidden ring-1 ring-black/5 dark:ring-white/5 widget-library-preview"
        style={{
          width: renderWidth,
          height: renderHeight,
          transform: `scale(${displayScale})`,
        }}
      >
        {mounted ? <Suspense fallback={null}>{children}</Suspense> : null}
      </div>
    )
  },
)
LibraryPreviewSlot.displayName = 'LibraryPreviewSlot'

// 小组件库右侧滚动提示 - 独立组件，隔离滚动状态，
// 避免每次滚动都重渲染整个小组件库（含所有预览小组件）导致卡顿
const LibraryScrollHint = React.memo(
  ({
    scrollRef,
    availableWidgets,
  }: {
    scrollRef: React.RefObject<HTMLDivElement | null>
    availableWidgets: WidgetType[]
  }) => {
    const [canScrollRight, setCanScrollRight] = useState(false)
    // 用户一旦手动滑动过，本次编辑期间就不再提示
    const [hasScrolled, setHasScrolled] = useState(false)
    const rafRef = useRef<number | null>(null)

    const update = useCallback(() => {
      rafRef.current = null
      const el = scrollRef.current
      if (!el) return
      const hasOverflow = el.scrollWidth - el.clientWidth > 4
      const atEnd = el.scrollLeft + el.clientWidth >= el.scrollWidth - 4
      setCanScrollRight(hasOverflow && !atEnd)
      if (el.scrollLeft > 4) setHasScrolled(true)
    }, [scrollRef])

    useEffect(() => {
      const el = scrollRef.current
      if (!el) return

      const onScroll = () => {
        if (rafRef.current) return
        rafRef.current = requestAnimationFrame(update)
      }

      el.addEventListener('scroll', onScroll, { passive: true })
      window.addEventListener('resize', update)
      return () => {
        el.removeEventListener('scroll', onScroll)
        window.removeEventListener('resize', update)
        if (rafRef.current) cancelAnimationFrame(rafRef.current)
      }
    }, [scrollRef, update])

    // 小组件列表内容变化时（如切换 1 行/2 行模式）重新计算是否溢出
    useEffect(() => {
      update()
    }, [availableWidgets, update])

    const visible = canScrollRight && !hasScrolled

    return (
      <div
        className={`pointer-events-none absolute inset-y-0 right-0 flex items-center justify-end pr-3 transition-opacity duration-300 ${
          visible ? 'opacity-100' : 'opacity-0'
        }`}
      >
        <motion.div
          className="flex h-8 w-8 items-center justify-center rounded-full bg-white/95 dark:bg-neutral-900/90 shadow-lg ring-1 ring-black/5 dark:ring-white/10"
          animate={
            // 不可见时停止循环动画，避免编辑期间一直空跑 rAF
            visible ? { x: [0, 5, 0], scale: [1, 1.08, 1] } : { x: 0, scale: 1 }
          }
          transition={
            visible
              ? {
                  duration: 1.3,
                  repeat: Number.POSITIVE_INFINITY,
                  ease: 'easeInOut',
                }
              : { duration: 0.2 }
          }
        >
          <FaChevronRight
            className="text-gray-500 dark:text-white/70"
            size={16}
          />
        </motion.div>
      </div>
    )
  },
)
LibraryScrollHint.displayName = 'LibraryScrollHint'

// 可用小组件类型定义
export interface WidgetType {
  id: string
  name: string
  defaultSize: WidgetSize
  component: React.ComponentType<WidgetComponentProps>
  supportedSizes?: WidgetSize[] // 支持的尺寸列表，如果未定义则支持所有尺寸
  settings?: TappSettingItem[] // Tapp Widget 每实例设置声明
}

// 将 widget id 转换为翻译键 (kebab-case -> camelCase)
function getWidgetTranslationKey(id: string): string {
  return id.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())
}

/** Optional display label when host i18n has a key; never required for search. */
function getWidgetDisplayLabel(
  widgetType: WidgetType,
  widgetsI18n: Record<string, unknown>,
): string {
  const key = getWidgetTranslationKey(widgetType.id)
  const translated = widgetsI18n[key]
  return typeof translated === 'string' && translated.trim()
    ? translated
    : widgetType.name
}

/** Free-form extras from third-party / Tapp widgets when present. */
function getWidgetSearchExtras(
  widgetType: WidgetType,
): Array<string | null | undefined> {
  const extra = widgetType as WidgetType & {
    category?: string
    tappId?: string
    description?: string
  }
  return [extra.category, extra.tappId, extra.description]
}

interface WidgetGridProps {
  widgets: WidgetConfig[]
  availableWidgets: WidgetType[]
  onWidgetsChange?: (widgets: WidgetConfig[]) => void
  isEditMode: boolean
  onToggleEditMode: (isEdit: boolean) => void
  children?: React.ReactNode
  customGridColumns?: number // Optional prop to override responsive grid columns
  customGridRows?: number // Optional prop to override default grid rows
  libraryContainerClassName?: string
  libraryContentClassName?: string
  libraryStyle?: React.CSSProperties
  libraryAnimation?: {
    initial: any
    animate: any
    exit: any
  }
  autoHeight?: boolean
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
  const dim = SIZE_TO_DIMENSIONS[widget.size]
  const { x, y } = widget.position

  // 检查是否超出边界
  if (x < 0 || y < 0 || x + dim.w > gridWidth || y + dim.h > gridHeight) {
    return true
  }

  // 检查与其他小组件的重叠
  for (const other of allWidgets) {
    if (other.id === excludeId || other.id === widget.id) continue

    const otherDim = SIZE_TO_DIMENSIONS[other.size]
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

export default function WidgetGrid({
  widgets,
  availableWidgets,
  onWidgetsChange,
  isEditMode,
  onToggleEditMode,
  children,
  customGridColumns,
  customGridRows,
  libraryContainerClassName,
  libraryContentClassName,
  libraryStyle,
  libraryAnimation,
  autoHeight,
}: WidgetGridProps) {
  const { t } = useI18n()
  // Seed from real viewport immediately — never paint desktop 16-col on a
  // phone-width DevTools session then “morph” into 4-col (looks broken).
  const [gridColumns, setGridColumns] = useState(() =>
    readInitialHomeGridColumns(customGridColumns),
  )
  // Only enable compact mode (auto-layout) if we are in responsive mode (no custom columns) AND width is small
  const isCompact = !customGridColumns && gridColumns < GRID_WIDTH
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
  const bandTimersRef = useRef<{ out?: number; in?: number }>({})
  /**
   * null = settled; 'out' | 'in' = cross-band fade (no geometry lerp).
   */
  const [bandSwitch, setBandSwitch] = useState<'out' | 'in' | null>(null)
  const anim = useAnimationLevel()
  /**
   * Same-band only: drag/resize polish. Cross-band uses opacity crossfade —
   * never interpolate compact packing ↔ desktop saved coords.
   */
  const geometryMotion = !isExlight(anim) && bandSwitch === null

  // 计算内容高度 (用于 autoHeight)
  const contentHeight = useMemo(() => {
    if (!autoHeight) return 0
    let maxY = 0
    widgets.forEach((w) => {
      const dim = SIZE_TO_DIMENSIONS[w.size]
      maxY = Math.max(maxY, w.position.y + dim.h)
    })
    return maxY
  }, [widgets, autoHeight])

  // 响应式列档：防抖宽度 + 迟滞；tablet↔desktop 可淡入淡出；含 phone 则硬切
  const { width: windowWidth } = useDebouncedWindowSize(150)
  const animHardCut = isExlight(anim)

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
    }

    if (customGridColumns) {
      clearBandTimers()
      bandSwitchingRef.current = false
      setBandSwitch(null)
      setGridColumns(customGridColumns)
      prevColumnsRef.current = customGridColumns
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
          requestAnimationFrame(() => tryBandMorph())
        }, GRID_BAND_IN_MS)
      }, GRID_BAND_OUT_MS)
    }

    tryBandMorph()
  }, [windowWidth, customGridColumns, animHardCut])

  // Unmount only: drop pending band morph timers
  useEffect(() => {
    return () => {
      if (bandTimersRef.current.out) clearTimeout(bandTimersRef.current.out)
      if (bandTimersRef.current.in) clearTimeout(bandTimersRef.current.in)
      bandSwitchingRef.current = false
    }
  }, [])

  // 紧凑模式布局计算 (自动重排)
  const compactLayout = useMemo(() => {
    if (!isCompact) return null

    // 按原始位置排序 (y 优先, 然后 x)
    const sortedWidgets = [...widgets].sort((a, b) => {
      if (a.position.y === b.position.y) return a.position.x - b.position.x
      return a.position.y - b.position.y
    })

    const occupied = new Set<string>()
    const newWidgets: WidgetConfig[] = []
    let maxY = 0

    const isOccupied = (x: number, y: number, w: number, h: number) => {
      for (let i = 0; i < w; i++) {
        for (let j = 0; j < h; j++) {
          if (occupied.has(`${x + i},${y + j}`)) return true
        }
      }
      return false
    }

    const markOccupied = (x: number, y: number, w: number, h: number) => {
      for (let i = 0; i < w; i++) {
        for (let j = 0; j < h; j++) {
          occupied.add(`${x + i},${y + j}`)
        }
      }
    }

    for (const widget of sortedWidgets) {
      const dim = SIZE_TO_DIMENSIONS[widget.size]
      // 限制宽度不超过当前网格列数
      const w = Math.min(dim.w, gridColumns)
      const h = dim.h

      // 寻找第一个可用位置
      let x = 0
      let y = 0
      let placed = false

      while (!placed) {
        if (x + w <= gridColumns && !isOccupied(x, y, w, h)) {
          markOccupied(x, y, w, h)
          newWidgets.push({
            ...widget,
            position: { x, y },
          })
          maxY = Math.max(maxY, y + h)
          placed = true
        } else {
          x++
          if (x >= gridColumns) {
            x = 0
            y++
          }
        }
        // 防止死循环
        if (y > 100) break
      }
    }

    return { widgets: newWidgets, height: Math.max(4, maxY) }
  }, [widgets, isCompact, gridColumns])

  const currentWidgets =
    isCompact && compactLayout ? compactLayout.widgets : widgets
  const currentGridWidth = gridColumns
  const currentGridHeight =
    isCompact && compactLayout
      ? compactLayout.height
      : autoHeight
        ? Math.max(customGridRows || 0, contentHeight)
        : customGridRows || GRID_HEIGHT

  // Explicit height from cols/rows. Cross-band: snap (no height transition).
  const gridPixelHeight =
    containerWidth > 0
      ? (containerWidth * currentGridHeight) / currentGridWidth
      : undefined

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

  const [draggedWidget, setDraggedWidget] = useState<{
    type: 'existing' | 'new'
    widgetId?: string
    widgetTypeId?: string
    offset: { x: number; y: number }
  } | null>(null)
  const [resizingWidget, setResizingWidget] = useState<{
    widgetId: string
    startPos: { x: number; y: number }
    startSize: WidgetSize
    direction?: 'se' | 's'
  } | null>(null)
  const [hoveredCell, setHoveredCell] = useState<{
    x: number
    y: number
  } | null>(null)
  const [widgetHistory, setWidgetHistory] = useState<WidgetConfig[][]>([])
  const [historyIndex, setHistoryIndex] = useState(-1)
  const [hoveredWidgetId, setHoveredWidgetId] = useState<string | null>(null)
  const [dragCursorPosition, setDragCursorPosition] = useState<{
    x: number
    y: number
  } | null>(null)

  // 小组件库横向滚动 - ref 本身不触发重渲染，滚动状态由独立子组件管理，
  // 避免每次滚动都重渲染整个小组件库（含所有预览小组件）导致卡顿
  const libraryScrollRef = useRef<HTMLDivElement>(null)
  const [librarySearchQuery, setLibrarySearchQuery] = useState('')

  // 离开编辑模式时清空搜索，避免下次进入带着旧筛选
  useEffect(() => {
    if (!isEditMode) setLibrarySearchQuery('')
  }, [isEditMode])

  // 进入编辑模式：预热目录内全部类型（含报告壳 + 全 report-* 对应 face）
  useEffect(() => {
    if (!isEditMode) return
    void preloadBuiltinWidgets(availableWidgets.map((w) => w.id)).catch(
      () => {},
    )
  }, [isEditMode, availableWidgets])

  /*
   * id → WidgetType 索引。
   * 此前每处都 `availableWidgets.find(...)`：渲染循环里每个格子一次，
   * 拖拽/缩放的 rAF 回调里每帧一次，而 availableWidgets 含全部 Tapp 小组件。
   */
  const widgetTypeById = useMemo(() => {
    const map = new Map<string, WidgetType>()
    for (const widgetType of availableWidgets) map.set(widgetType.id, widgetType)
    return map
  }, [availableWidgets])

  // 按运行时元数据过滤（内置 + 第三方 Tapp 同一路径，不依赖预置名单）
  const libraryWidgets = useMemo(() => {
    const widgetsI18n = t.widgets as Record<string, unknown>
    return availableWidgets.filter((widgetType) =>
      widgetTypeMatchesLibrarySearch(librarySearchQuery, {
        id: widgetType.id,
        name: widgetType.name,
        label: getWidgetDisplayLabel(widgetType, widgetsI18n),
        extras: getWidgetSearchExtras(widgetType),
      }),
    )
  }, [availableWidgets, librarySearchQuery, t.widgets])

  // 搜索结果变化时滚回列表起点，避免停在空区域
  useEffect(() => {
    const el = libraryScrollRef.current
    if (!el) return
    el.scrollLeft = 0
    el.scrollTop = 0
  }, [librarySearchQuery])

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
  })
  latestRef.current = {
    widgets,
    onWidgetsChange,
    widgetHistory,
    historyIndex,
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
      if (!isEditMode) return
      e.stopPropagation()
      e.preventDefault()

      const widget = latestRef.current.widgets.find((w) => w.id === widgetId)
      if (!widget) return

      // 拖拽开始时更新 gridRect 缓存
      updateGridRectCache()

      // 立即设置光标位置
      setDragCursorPosition({ x: e.clientX, y: e.clientY })

      // 设置拖拽状态
      setDraggedWidget({
        type: 'existing',
        widgetId,
        offset: { x: 0, y: 0 }, // offset 现在不再使用
      })
    },
    [isEditMode, updateGridRectCache],
  )

  // 开始拖拽新小组件
  const handleNewWidgetDragStart = useCallback(
    (e: React.MouseEvent | React.TouchEvent, widgetTypeId: string) => {
      e.stopPropagation()
      e.preventDefault()

      // 拖拽开始时更新 gridRect 缓存
      updateGridRectCache()

      // 获取初始位置
      const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
      const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY

      // 立即设置光标位置
      setDragCursorPosition({ x: clientX, y: clientY })

      // 设置拖拽状态
      setDraggedWidget({
        type: 'new',
        widgetTypeId,
        offset: { x: 0, y: 0 },
      })
    },
    [updateGridRectCache],
  )

  // 开始调整大小
  const handleResizeStart = useCallback(
    (
      e: React.MouseEvent | React.TouchEvent,
      widgetId: string,
      direction: 'se' | 's' = 'se',
    ) => {
      if (!isEditMode) return
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
        direction,
      })
    },
    [isEditMode, updateGridRectCache],
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
          rawW = SIZE_TO_DIMENSIONS[widget.size].w
        }

        // Find closest valid size
        let bestSize = widget.size
        let minDistance = Infinity

        // 获取该组件类型支持的尺寸列表
        const widgetType = widgetTypeById.get(widget.type)

        // 如果找不到组件类型定义，或者没有定义 supportedSizes，则不允许调整大小（锁定当前尺寸）
        // 这是一个安全措施，防止意外拉伸到不支持的尺寸
        if (!widgetType) {
          rafRef.current = null
          return
        }

        const supportedSizes =
          widgetType.supportedSizes ||
          (Object.keys(SIZE_TO_DIMENSIONS) as WidgetSize[])

        // 过滤出有效的尺寸
        const validSizes = supportedSizes.filter(
          (size) => SIZE_TO_DIMENSIONS[size],
        )

        for (const size of validSizes) {
          const dim = SIZE_TO_DIMENSIONS[size]

          // 如果是底部调整，只考虑宽度相同的尺寸
          if (
            resizingWidget.direction === 's' &&
            dim.w !== SIZE_TO_DIMENSIONS[widget.size].w
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

        if (bestSize !== widget.size) {
          const newWidget = { ...widget, size: bestSize }
          // Check collision excluding itself
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
      onWidgetsChange,
      widgetTypeById,
    ],
  )

  // 结束调整大小
  const handleResizeEnd = useCallback(() => {
    if (resizingWidget) {
      saveToHistory(widgets)
      setResizingWidget(null)
    }
    if (rafRef.current) {
      cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }
  }, [resizingWidget, widgets, saveToHistory])

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
        setDragCursorPosition({ x: clientX, y: clientY })

        // 计算单元格尺寸
        const cellWidth = gridRect.width / currentGridWidth
        const cellHeight = gridRect.height / currentGridHeight

        // 获取当前拖拽的小组件尺寸
        let size: WidgetSize = '1x1'
        if (draggedWidget.type === 'existing' && draggedWidget.widgetId) {
          const widget = widgets.find((w) => w.id === draggedWidget.widgetId)
          size = widget?.size || '1x1'
        } else if (draggedWidget.type === 'new' && draggedWidget.widgetTypeId) {
          const widgetType = widgetTypeById.get(draggedWidget.widgetTypeId)
          size = widgetType?.defaultSize || '1x1'
        }
        const dim = SIZE_TO_DIMENSIONS[size]

        // 计算鼠标在网格中的位置
        let mouseX = clientX - gridRect.left
        let mouseY = clientY - gridRect.top

        // 所有小组件都以中心点为参考
        // 减去小组件尺寸的一半，使光标位于中心
        mouseX -= (dim.w * cellWidth) / 2
        mouseY -= (dim.h * cellHeight) / 2

        // 转换为网格坐标
        let gridX = Math.floor(mouseX / cellWidth)
        let gridY = Math.floor(mouseY / cellHeight)

        // 确保小组件不会超出边界（考虑小组件尺寸）
        gridX = Math.max(0, Math.min(currentGridWidth - dim.w, gridX))
        gridY = Math.max(0, Math.min(currentGridHeight - dim.h, gridY))

        setHoveredCell((prev) => {
          if (prev?.x === gridX && prev?.y === gridY) return prev
          return { x: gridX, y: gridY }
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
    if (rafRef.current) {
      cancelAnimationFrame(rafRef.current)
      rafRef.current = null
    }

    if (!draggedWidget || !hoveredCell) {
      setDraggedWidget(null)
      setHoveredCell(null)
      setDragCursorPosition(null)
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
      }
    } else if (draggedWidget.type === 'new' && draggedWidget.widgetTypeId) {
      // 添加新小组件
      const widgetType = widgetTypeById.get(draggedWidget.widgetTypeId)
      if (!widgetType) return

      const newWidget: WidgetConfig = {
        id: `widget_${Date.now()}`,
        type: widgetType.id,
        size: widgetType.defaultSize,
        position: hoveredCell,
        config:
          widgetType.settings && widgetType.settings.length > 0
            ? Object.fromEntries(
                widgetType.settings
                  .filter((setting) => setting.defaultValue !== undefined)
                  .map((setting) => [setting.key, setting.defaultValue]),
              )
            : undefined,
      }

      // 为特定类型的小组件自动设置配置
      if (widgetType.id.startsWith('platform-')) {
        // 平台卡片小组件
        const platformId = widgetType.id.replace('platform-', '')
        newWidget.config = { platformId }
      } else if (widgetType.id.startsWith('report-')) {
        // 报告卡片小组件
        const platformId = widgetType.id.replace('report-', '')
        newWidget.config = { platformId }
      }

      // 检查碰撞
      if (
        !checkCollision(newWidget, widgets, currentGridWidth, currentGridHeight)
      ) {
        const newWidgets = [...widgets, newWidget]
        onWidgetsChange?.(newWidgets)
        saveToHistory(newWidgets)
      }
    }

    setDraggedWidget(null)
    setHoveredCell(null)
    setDragCursorPosition(null)
  }, [
    draggedWidget,
    hoveredCell,
    widgets,
    widgetTypeById,
    onWidgetsChange,
    saveToHistory,
    currentGridWidth,
    currentGridHeight,
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
    if (draggedWidget) {
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
  }, [draggedWidget]) // 只依赖 draggedWidget 是否存在

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
      // ESC: 取消编辑
      if (e.key === 'Escape') {
        onToggleEditMode(false)
      }
    }

    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [isEditMode, handleUndo, handleRedo, onToggleEditMode])

  // 预览拖拽位置和组件信息
  const dragPreview = useMemo(() => {
    if (!draggedWidget || !hoveredCell) return null

    let size: WidgetSize = '1x1'
    let widgetType: WidgetType | undefined
    let widgetConfig: WidgetConfig | undefined

    if (draggedWidget.type === 'existing' && draggedWidget.widgetId) {
      const widget = widgets.find((w) => w.id === draggedWidget.widgetId)
      size = widget?.size || '1x1'
      widgetConfig = widget
      widgetType = widget ? widgetTypeById.get(widget.type) : undefined
    } else if (draggedWidget.type === 'new' && draggedWidget.widgetTypeId) {
      widgetType = widgetTypeById.get(draggedWidget.widgetTypeId)
      size = widgetType?.defaultSize || '1x1'

      // 创建预览配置
      widgetConfig = {
        id: 'drag-preview',
        type: draggedWidget.widgetTypeId,
        size,
        position: hoveredCell,
        config: draggedWidget.widgetTypeId.startsWith('platform-')
          ? { platformId: draggedWidget.widgetTypeId.replace('platform-', '') }
          : draggedWidget.widgetTypeId.startsWith('report-')
            ? { platformId: draggedWidget.widgetTypeId.replace('report-', '') }
            : undefined,
      }
    }

    const dim = SIZE_TO_DIMENSIONS[size]
    const testWidget: WidgetConfig = {
      id: 'preview',
      type: widgetConfig?.type || '',
      size,
      position: hoveredCell,
    }

    const hasCollision = checkCollision(
      testWidget,
      widgets,
      currentGridWidth,
      currentGridHeight,
      draggedWidget.type === 'existing' ? draggedWidget.widgetId : undefined,
    )

    return {
      position: hoveredCell,
      size: dim,
      hasCollision,
      widgetType,
      widgetConfig,
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
        className="widget-grid-background absolute inset-0 pointer-events-none z-0"
        style={
          {
            '--widget-grid-cell-w': `${100 / currentGridWidth}%`,
            '--widget-grid-cell-h': `${100 / currentGridHeight}%`,
          } as React.CSSProperties
        }
      />
    ),
    [currentGridWidth, currentGridHeight],
  )

  // 小组件库内容
  const libraryContent = (
    <motion.div
      initial={libraryAnimation?.initial || { y: '-100%' }}
      animate={libraryAnimation?.animate || { y: 0 }}
      exit={libraryAnimation?.exit || { y: '-100%' }}
      transition={{ type: 'spring', damping: 25, stiffness: 200 }}
      className={
        libraryContainerClassName ||
        'fixed top-0 left-0 right-0 z-50 bg-white/80 dark:bg-black/80 backdrop-blur-xl border-b border-gray-200/50 dark:border-white/5 shadow-2xl'
      }
      style={libraryStyle}
    >
      <div className="w-full max-w-480 mx-auto">
        {/* 控制栏：标题 → 搜索 */}
        <div className="flex flex-wrap items-center gap-3 px-4 sm:px-6 py-3 border-b border-gray-200/30 dark:border-white/5">
          <div className="flex items-center gap-2 text-gray-800 dark:text-gray-100 shrink-0">
            <img
              src="/icons/widgets/library.webp"
              alt=""
              aria-hidden="true"
              className="h-5 w-5 object-contain"
              draggable={false}
              decoding="async"
            />
            <span className="font-bold">{t.widgetGrid.widgetLibrary}</span>
          </div>

          <div
            className="relative w-44 sm:w-52 min-w-0"
            onMouseDown={(e) => e.stopPropagation()}
            onTouchStart={(e) => e.stopPropagation()}
          >
            <FaSearch
              className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-400 dark:text-white/40"
              size={12}
              aria-hidden
            />
            <input
              type="search"
              value={librarySearchQuery}
              onChange={(e) => setLibrarySearchQuery(e.target.value)}
              placeholder={t.widgetGrid.searchWidgets}
              aria-label={t.widgetGrid.searchWidgets}
              autoComplete="off"
              className="widget-library-search-input w-full rounded-lg border border-gray-200/70 dark:border-white/10 bg-white/70 dark:bg-white/5 py-1.5 pl-8 pr-8 text-sm text-gray-800 dark:text-gray-100 placeholder:text-gray-400 dark:placeholder:text-white/35 outline-none focus:border-blue-400/60 focus:ring-2 focus:ring-blue-400/20 transition-[border-color,box-shadow]"
            />
            {librarySearchQuery ? (
              <button
                type="button"
                onClick={() => setLibrarySearchQuery('')}
                className="absolute right-1.5 top-1/2 -translate-y-1/2 rounded-md p-1 text-gray-400 hover:text-gray-700 dark:hover:text-white/80 hover:bg-black/5 dark:hover:bg-white/10 transition-colors"
                title={t.widgetGrid.clearSearch}
                aria-label={t.widgetGrid.clearSearch}
              >
                <FaTimes size={10} />
              </button>
            ) : null}
          </div>
        </div>

        {/* 组件列表 - 横向滚动 */}
        <div className="relative">
          <div
            ref={libraryScrollRef}
            className={
              libraryContentClassName ||
              'flex items-center gap-6 p-6 overflow-x-auto scrollbar-hide min-h-40'
            }
            onWheel={(e) => {
              if (libraryContentClassName) return
              // 只接管纯垂直滚轮手势（deltaX 恒为 0，鼠标滚轮特征）；
              // 只要带有 deltaX（触控板横滑及其惯性尾段都会带一点）就完全交给浏览器原生处理，
              // 否则会在惯性阶段跟原生横向滚动打架，造成内容位置概率性闪现
              if (e.deltaX !== 0 || e.deltaY === 0) return
              const el = e.currentTarget
              const maxScrollLeft = el.scrollWidth - el.clientWidth
              if (maxScrollLeft <= 0) return
              e.preventDefault()
              el.scrollLeft = Math.max(
                0,
                Math.min(maxScrollLeft, el.scrollLeft + e.deltaY),
              )
            }}
          >
            {libraryWidgets.length === 0 ? (
              <div className="flex w-full min-h-28 items-center justify-center px-4 text-sm text-gray-500 dark:text-white/50">
                {t.widgetGrid.noSearchResults}
              </div>
            ) : (
              libraryWidgets.map((widgetType) => {
              const WidgetComponent = widgetType.component
              // 与 useWidgetSize 标准尺寸同步：内部按 scale=1 设计稿渲染，
              // 外层仅用 LIBRARY_PREVIEW_DISPLAY_SCALE 压缩条带展示。
              const standard = getStandardWidgetDimensions(
                widgetType.defaultSize,
              )
              const renderWidth = standard.width
              const renderHeight = standard.height
              const displayScale = LIBRARY_PREVIEW_DISPLAY_SCALE
              const wrapperWidth = renderWidth * displayScale
              const wrapperHeight = renderHeight * displayScale

              // 构造预览配置
              const previewConfig: WidgetConfig = {
                id: `preview-${widgetType.id}`,
                type: widgetType.id,
                size: widgetType.defaultSize,
                position: { x: 0, y: 0 },
                config: widgetType.id.startsWith('platform-')
                  ? { platformId: widgetType.id.replace('platform-', '') }
                  : widgetType.id.startsWith('report-')
                    ? { platformId: widgetType.id.replace('report-', '') }
                    : undefined,
              }

              const libraryLabel = getWidgetDisplayLabel(
                widgetType,
                t.widgets as Record<string, unknown>,
              )
              const isLibrary1x1 = widgetType.defaultSize === '1x1'

              return (
                <motion.div
                  key={widgetType.id}
                  className={`relative group cursor-move shrink-0${isLibrary1x1 ? ' flex flex-col items-center' : ''}`}
                  style={
                    isLibrary1x1
                      ? { width: wrapperWidth }
                      : {
                          width: wrapperWidth,
                          height: wrapperHeight,
                        }
                  }
                  draggable
                  onMouseDown={(e: React.MouseEvent) =>
                    handleNewWidgetDragStart(e, widgetType.id)
                  }
                  onTouchStart={(e: React.TouchEvent) =>
                    handleNewWidgetDragStart(e, widgetType.id)
                  }
                  whileHover={{ scale: 1.05, zIndex: 10 }}
                  whileTap={{ scale: 0.95 }}
                >
                  {isLibrary1x1 ? (
                    <>
                      {/* 1x1：预览框固定尺寸；名称放框外下方，避免内叠 tip 溢出 */}
                      <div
                        className="relative"
                        style={{
                          width: wrapperWidth,
                          height: wrapperHeight,
                        }}
                      >
                        <LibraryPreviewSlot
                          scrollRef={libraryScrollRef}
                          renderWidth={renderWidth}
                          renderHeight={renderHeight}
                          displayScale={displayScale}
                        >
                          <WidgetComponent
                            config={previewConfig}
                            isEditMode={true}
                            isPreview={true}
                          />
                        </LibraryPreviewSlot>
                        <div className="absolute inset-0 z-20 rounded-xl ring-1 ring-black/5 dark:ring-white/10 group-hover:ring-2 group-hover:ring-blue-500 transition-all bg-transparent" />
                      </div>
                      <div
                        className="mt-5 w-full px-0.5 text-center text-[10px] font-bold leading-tight text-gray-600 dark:text-gray-300 line-clamp-2 break-words pointer-events-none"
                        title={libraryLabel}
                      >
                        {libraryLabel}
                      </div>
                    </>
                  ) : (
                    <>
                      {/* 缩放容器（按需挂载，见 LibraryPreviewSlot） */}
                      <LibraryPreviewSlot
                        scrollRef={libraryScrollRef}
                        renderWidth={renderWidth}
                        renderHeight={renderHeight}
                        displayScale={displayScale}
                      >
                        <WidgetComponent
                          config={previewConfig}
                          isEditMode={true}
                          isPreview={true}
                        />
                      </LibraryPreviewSlot>

                      {/* 遮罩层 - 用于拖拽交互和高亮 */}
                      <div className="absolute inset-0 z-20 rounded-xl ring-1 ring-black/5 dark:ring-white/10 group-hover:ring-2 group-hover:ring-blue-500 transition-all bg-transparent" />

                      {/* 悬浮提示（非 1x1 维持原样） */}
                      <div className="absolute bottom-2 left-1/2 -translate-x-1/2 whitespace-nowrap text-xs font-bold text-gray-600 dark:text-gray-300 opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none bg-white/90 dark:bg-neutral-900/90 px-3 py-1 rounded-full backdrop-blur-sm shadow-sm border border-gray-200/50 dark:border-neutral-700/50">
                        {libraryLabel}
                      </div>
                    </>
                  )}
                </motion.div>
              )
            })
            )}

            {/* 占位符，确保最后一个元素右侧有间距 */}
            {libraryWidgets.length > 0 ? (
              <div className="w-2 shrink-0" />
            ) : null}
          </div>

          {/* 右侧提示：还有更多小组件可滚动查看，一旦手动滑动过就不再出现 */}
          <LibraryScrollHint
            scrollRef={libraryScrollRef}
            availableWidgets={libraryWidgets}
          />
        </div>
      </div>
    </motion.div>
  )

  return (
    <div
      className={`widget-grid-root flex flex-col gap-2 min-h-0 ${
        isCompact ? 'h-auto flex-none' : 'h-full flex-1'
      }`}
      data-grid-cols={currentGridWidth}
      data-grid-compact={isCompact ? 'true' : 'false'}
      data-band-switch={bandSwitch ?? undefined}
    >
      {/* 编辑模式：小组件库（顶部悬浮） */}
      {libraryContainerClassName ? (
        createPortal(
          <AnimatePresence>
            {isEditMode && !isCompact && libraryContent}
          </AnimatePresence>,
          document.body,
        )
      ) : (
        <AnimatePresence>
          {isEditMode && !isCompact && libraryContent}
        </AnimatePresence>
      )}

      {/* 网格区域 */}
      <div
        className={`relative w-full flex flex-col min-h-0 ${
          isCompact ? 'justify-start pb-20' : 'flex-1 justify-end'
        }`}
      >
        {/* 插入 children (InfoBar) */}
        {children}

        <div
          ref={gridRef}
          className={`widget-grid-container relative w-full rounded-xl ${
            geometryMotion && rowCountMorphing
              ? 'widget-grid-container--layout-motion'
              : ''
          } ${isEditMode ? 'edit-mode' : ''}`}
          data-band-switch={bandSwitch ?? undefined}
          style={
            gridPixelHeight
              ? { height: gridPixelHeight }
              : {
                  aspectRatio: `${currentGridWidth} / ${currentGridHeight}`,
                }
          }
        >
          {/* 背景网格线（编辑模式） */}
          {isEditMode && !isCompact && gridBackground}

          {/* 拖拽位置指示器 - 网格中的目标位置预览 */}
          {dragPreview && !isCompact && (
            <motion.div
              initial={{ scale: 0.95, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              transition={{ type: 'spring', stiffness: 500, damping: 35 }}
              className={`absolute z-20 overflow-visible rounded-xl transition-all pointer-events-none ${
                dragPreview.hasCollision
                  ? 'bg-red-500/10 ring-2 ring-red-500/50'
                  : 'bg-blue-500/10 ring-2 ring-blue-500/50'
              }`}
              style={{
                left: `${(dragPreview.position.x / currentGridWidth) * 100}%`,
                top: `${(dragPreview.position.y / currentGridHeight) * 100}%`,
                width: `${(dragPreview.size.w / currentGridWidth) * 100}%`,
                height: `${(dragPreview.size.h / currentGridHeight) * 100}%`,
              }}
            >
              {/* 状态提示：1x1 格子窄，强制单行并允许溢出，避免「位置冲突」折行 */}
              <div className="absolute inset-0 flex items-center justify-center overflow-visible">
                <div
                  className={`rounded-full font-bold shadow-lg backdrop-blur-sm whitespace-nowrap ${
                    dragPreview.size.w === 1 && dragPreview.size.h === 1
                      ? 'px-1.5 py-0.5 text-[9px] leading-none'
                      : 'px-3 py-1 text-xs'
                  } ${
                    dragPreview.hasCollision
                      ? 'bg-red-500/90 text-white'
                      : 'bg-blue-500/90 text-white'
                  }`}
                >
                  {dragPreview.hasCollision
                    ? t.widgetGrid.positionConflict
                    : t.widgetGrid.canPlace}
                </div>
              </div>
            </motion.div>
          )}

          {/* 小组件 */}
          <div className="absolute inset-0 z-10">
            {currentWidgets.map((widget, index) => {
              const widgetType = widgetTypeById.get(widget.type)
              if (!widgetType) {
                // 未知/未注册组件：渲染轻量占位而非静默跳过（issue #72）。
                // 此前 return null 导致 Tapp widget 在注册表尚未同步/同步
                // 失败时整卡空白且无任何提示，用户无法区分"加载中/失败/被
                // 过滤"；占位至少暴露该格子的 widget 类型，便于诊断。
                const dim =
                  SIZE_TO_DIMENSIONS[widget.size] || SIZE_TO_DIMENSIONS['2x2']
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
                />
              )
            })}
          </div>
        </div>
      </div>

      {/* 跟随光标的真实小组件预览 */}
      <AnimatePresence>
        {draggedWidget &&
          dragCursorPosition &&
          dragPreview &&
          dragPreview.widgetType &&
          dragPreview.widgetConfig &&
          (() => {
            // 计算实际的网格单元格尺寸
            const gridRect = containerRef.current?.getBoundingClientRect()
            let cellWidth = 100
            let cellHeight = 100

            if (gridRect) {
              cellWidth = gridRect.width / currentGridWidth
              cellHeight = gridRect.height / currentGridHeight
            }

            // 计算预览的实际像素尺寸
            const previewWidth = dragPreview.size.w * cellWidth
            const previewHeight = dragPreview.size.h * cellHeight

            return (
              <motion.div
                initial={{ opacity: 0, scale: 0.9 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.9 }}
                transition={{ type: 'spring', stiffness: 400, damping: 30 }}
                className="fixed pointer-events-none z-9999"
                style={{
                  left: `${dragCursorPosition.x}px`,
                  top: `${dragCursorPosition.y}px`,
                }}
              >
                {/* 小组件预览 - 中心对齐光标 */}
                <div
                  className={`absolute rounded-xl shadow-2xl ring-2 transition-all ${
                    dragPreview.hasCollision
                      ? 'ring-red-500/70'
                      : 'ring-blue-500/70'
                  }`}
                  style={{
                    left: '50%',
                    top: '50%',
                    transform: 'translate(-50%, -50%)',
                    width: `${previewWidth}px`,
                    height: `${previewHeight}px`,
                    opacity: 0.95,
                  }}
                >
                  <Suspense fallback={null}>
                    <dragPreview.widgetType.component
                      config={dragPreview.widgetConfig}
                      isEditMode={false}
                      isPreview={true}
                    />
                  </Suspense>
                </div>
              </motion.div>
            )
          })()}
      </AnimatePresence>
    </div>
  )
}
