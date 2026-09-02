/**
 * 磁贴墙：虚拟坐标 + 横向分页。
 *
 * 与首页 `WidgetGrid` 用同一套格子：**16 列 × 4 行**（tablet 8 列、phone 4 列），
 * 几何一律百分比。所以同一个 4×4 磁贴在首页和这里的物理尺寸是一样的，
 * 复用不是假的。
 *
 * 三条稳定性措施（缺一个就会「每次进来都长得不一样」）：
 * 1. 分数 0.05 分档，档内按 id（见 logic/score）
 * 2. 会话内冻结 `now`：进页面算一次顺序和尺寸，未读数字可变、位置不动
 * 3. 上次装箱结果进 localStorage，下次进来先当骨架
 *
 * 坐标只活在渲染期。拖拽只改 `sort_order`，位置永远由装箱决定。
 */

import type { CSSProperties, MouseEvent, ReactNode, TouchEvent } from 'react'
import type { BrewItemPreview, BrewSource } from '../../types/brew'
import type { ViewportBand } from '../../utils/viewportBands'
import type { BrewTileSize } from './logic/layout'
import type { BrewCard, PackedCard } from './logic/pack'
import type { BrewViewerRole } from './logic/score'
import type { BrewTopic } from './logic/topics'

import {
  LuCheck as Check,
  LuEdit3 as Edit3,
  LuLock as Lock,
  LuRefreshCw as RefreshCw,
} from '@lib/icons'

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'

import { useMediaQuery } from '../../hooks/useSharedEventListener'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { homeGridColsForBand, VIEWPORT_MQ } from '../../utils/viewportBands'
import { standardCellSizeForBand } from '../../utils/widgetSizeScale'
import { BrewPager } from './BrewPageDots'
import { brewMainCategory } from './constants'
import { tileSize, topicTileSize } from './logic/layout'
import { packBrewCards, parseTileSize } from './logic/pack'
import { brewScore } from './logic/score'
import { BrewSourceTile } from './tiles/BrewSourceTile'
import { BrewTopicTile } from './tiles/BrewTopicTile'
import './BrewTileWall.css'

/**
 * 行数下限/上限。
 *
 * **格子边长与首页完全一致**（`standardCellSizeForBand`），所以一个 4×4 磁贴
 * 在首页和这里的物理尺寸仍然相同 —— 复用不打折。变的只是「一页放几行」：
 * /brew 是整页视图，固定 4 行会在 1000px 高的屏幕上只用掉 320px，剩下的全是
 * 空白，还把源硬拆成三页。按可用高度取 4~8 行（偶数，否则高 2 的磁贴会剩半行）。
 */
const ROWS_MIN = 4
const ROWS_MAX = 8
/** 底部留给控制岛 + 分页点的高度 */
const BOTTOM_RESERVE = 150
/**
 * 磁贴之间的间距（每张卡自己吃 padding，不用 grid gap）。
 *
 * 4px 时相邻卡的玻璃面几乎贴在一起，一屏磁贴会糊成一整块直角大板子，
 * 外沿看起来像凭空多了一个方框。8px（实际间隙 16px）与旧网格的 `gap-4` 一致。
 */
const TILE_PADDING = 8
/** 布局缓存版本；改装箱规则就 +1，避免读到旧形状 */
const LAYOUT_CACHE_VERSION = 2

interface CachedSlot {
  key: string
  size: BrewTileSize
  x: number
  y: number
}

/** 骨架要按当时的行数还原，否则位置会错。 */
interface CachedLayout {
  rows: number
  slots: CachedSlot[]
}

function layoutCacheKey(scope: string, cols: number): string {
  return `brew:wall:v${LAYOUT_CACHE_VERSION}:${cols}:${scope}`
}

function readLayoutCache(scope: string, cols: number): CachedLayout | null {
  try {
    const raw = globalThis.localStorage?.getItem(layoutCacheKey(scope, cols))
    if (!raw) return null
    const parsed = JSON.parse(raw)
    if (!parsed || !Array.isArray(parsed.slots)) return null
    const rows = Number(parsed.rows)
    if (!Number.isFinite(rows) || rows < ROWS_MIN) return null
    return { rows, slots: parsed.slots as CachedSlot[] }
  } catch {
    return null
  }
}

/** 只缓存第一页：骨架的意义是「首屏别重排」，后面的页翻到了再说。 */
/** 清掉旧版本的布局缓存，别让 localStorage 里堆着永远读不到的 v1/v2 键。 */
function sweepStaleLayoutCache(): void {
  try {
    const ls = globalThis.localStorage
    if (!ls) return
    const prefix = `brew:wall:v${LAYOUT_CACHE_VERSION}:`
    for (let i = ls.length - 1; i >= 0; i--) {
      const k = ls.key(i)
      if (k && k.startsWith('brew:wall:v') && !k.startsWith(prefix)) ls.removeItem(k)
    }
  } catch {
    // 读不了就算了
  }
}

function writeLayoutCache(
  scope: string,
  cols: number,
  rows: number,
  firstPage: PackedCard[],
): void {
  try {
    sweepStaleLayoutCache()
    const payload: CachedLayout = {
      rows,
      slots: firstPage.map((c) => ({ key: c.key, size: c.size, x: c.x, y: c.y })),
    }
    globalThis.localStorage?.setItem(
      layoutCacheKey(scope, cols),
      JSON.stringify(payload),
    )
  } catch {
    // 配额满 / 隐私模式：骨架只是优化，丢了不影响正确性
  }
}

const PAGE_STORAGE_KEY = 'brew:wall:page'

function readStoredPage(scope: string): number {
  try {
    const raw = globalThis.sessionStorage?.getItem(`${PAGE_STORAGE_KEY}:${scope}`)
    const n = raw === null ? 0 : Number(raw)
    return Number.isFinite(n) && n >= 0 ? n : 0
  } catch {
    return 0
  }
}

function writeStoredPage(scope: string, page: number): void {
  try {
    globalThis.sessionStorage?.setItem(
      `${PAGE_STORAGE_KEY}:${scope}`,
      String(page),
    )
  } catch {
    // 忽略
  }
}

function useViewportBand(): ViewportBand {
  const isPhone = useMediaQuery(VIEWPORT_MQ.phone)
  const isDesktop = useMediaQuery(VIEWPORT_MQ.desktop)
  return isPhone ? 'phone' : isDesktop ? 'desktop' : 'tablet'
}

/** 百分比几何。与 WidgetGrid 同一套公式。 */
function slotStyle(
  slot: { x: number; y: number; size: string },
  cols: number,
  rows: number,
): CSSProperties {
  const span = parseTileSize(slot.size) ?? { w: 2, h: 2 }
  return {
    position: 'absolute',
    left: `${(slot.x / cols) * 100}%`,
    top: `${(slot.y / rows) * 100}%`,
    width: `${(span.w / cols) * 100}%`,
    height: `${(span.h / rows) * 100}%`,
    padding: TILE_PADDING,
  }
}

/**
 * 一个格位。`useWidgetSize` 必须在这一层调用 —— 磁贴按实测宽高拿 scale /
 * fontScale，缩到 compact 才不溢出。
 */
function TileSlot({
  size,
  render,
}: {
  size: BrewTileSize
  render: (info: {
    scale: number
    fontScale: number
    containerRef: React.RefCallback<HTMLDivElement>
  }) => ReactNode
}) {
  const { scale, fontScale, containerRef } = useWidgetSize(size)
  return <>{render({ scale, fontScale, containerRef })}</>
}

export interface BrewTileWallProps {
  /** 已按当前排序模式排好的源。装箱严格按这个顺序。 */
  sources: BrewSource[]
  /** 插到最前的主题卡（智能 / 主题模式）；其它模式传空数组 */
  topics?: BrewTopic[]
  /** 主题卡尺寸的模式来源 */
  topicMode?: 'smart' | 'topic'
  role: BrewViewerRole
  /** 搜索态：不装箱，统一 2×2 规整网格，单页可滚动 */
  isSearching?: boolean
  /** 缓存与页码的作用域键（分类 + 排序模式） */
  scope: string
  /** 分类边界强制翻页（`category` 排序用） */
  breakOnCategory?: boolean
  /** 未分类的展示名（`category` 换页标题用） */
  uncategorizedLabel?: string
  onSourceClick?: (source: BrewSource) => void
  onOpenItem?: (item: BrewItemPreview, source: BrewSource) => void
  onTopicClick?: (topic: BrewTopic) => void
  /** 卡片 DOM ref 上报（FLIP + 拖拽命中测试） */
  registerCardRef?: (id: number, el: HTMLDivElement | null) => void
  isEditMode?: boolean
  onDragStart?: (e: MouseEvent | TouchEvent, sourceId: number) => void
  draggingSourceId?: number | null
  dragOverSourceId?: number | null
  /**
   * 「锁定当前尺寸」开关。右下角拖拉 resize 已废弃 —— 磁贴尺寸由分数派生，
   * 手拉一个尺寸再被下次装箱推走只会让人以为坏了。锁定写回 `card_size`，
   * 传 `null` 表示解锁。
   */
  onToggleSizeLock?: (source: BrewSource, next: BrewTileSize | null) => void
  /** 翻页控件的无障碍标签 */
  prevPageLabel?: string
  nextPageLabel?: string
  /** `{n}` 替换成页码 */
  pageLabel?: string
  /** 编辑态：批量选择（老网格的整卡点击即选中） */
  selectedIds?: ReadonlySet<number>
  onToggleSelect?: (sourceId: number) => void
  /** 编辑态：打开 EditModal */
  onEditSource?: (source: BrewSource) => void
  /** 编辑态：单源刷新（友链没有） */
  onRefreshSource?: (sourceId: number) => void
  /** 图标加载后提到的主题色写回 */
  onThemeColorExtracted?: (sourceId: number, color: string) => void
  /** 编辑态操作条的无障碍标签 */
  editLabels?: {
    select?: string
    edit?: string
    refresh?: string
    lock?: string
    unlock?: string
  }
}

export default function BrewTileWall({
  sources,
  topics = [],
  topicMode = 'smart',
  role,
  isSearching = false,
  scope,
  breakOnCategory = false,
  uncategorizedLabel,
  onSourceClick,
  onOpenItem,
  onTopicClick,
  registerCardRef,
  isEditMode = false,
  onDragStart,
  draggingSourceId = null,
  dragOverSourceId = null,
  onToggleSizeLock,
  prevPageLabel,
  nextPageLabel,
  pageLabel,
  selectedIds,
  onToggleSelect,
  onEditSource,
  onRefreshSource,
  onThemeColorExtracted,
  editLabels,
}: BrewTileWallProps) {
  const band = useViewportBand()
  const cols = homeGridColsForBand(band)
  const cell = standardCellSizeForBand(band)
  const viewportRef = useRef<HTMLDivElement | null>(null)

  /**
   * 按可用高度派生行数。格子边长不变（还是首页那一套），只是一页多放几行 ——
   * /brew 是整页视图，固定 4 行会白扔掉半屏，还把源硬拆成更多页。
   */
  // 首帧就用上次缓存的行数，再在 paint 前实测一次：useState(4) + useEffect
  // 会先画一版 4 行的墙再跳成 8 行，每次进页面都闪一下。
  const [rows, setRows] = useState(
    () => readLayoutCache(scope, cols)?.rows ?? ROWS_MIN,
  )
  useLayoutEffect(() => {
    const measure = () => {
      const el = viewportRef.current
      if (!el) return
      const top = Math.max(0, el.getBoundingClientRect().top)
      const avail = window.innerHeight - top - BOTTOM_RESERVE
      const fit = Math.floor(avail / cell)
      // 取偶数：高 2 的磁贴才不会在底部剩半行
      const even = fit - (fit % 2)
      setRows(Math.max(ROWS_MIN, Math.min(ROWS_MAX, even)))
    }
    measure()
    window.addEventListener('resize', measure)
    return () => window.removeEventListener('resize', measure)
  }, [cell])

  // 会话内冻结的时钟。scope 变化（切排序 / 切分类）才重算，未读数字变化不挪卡。
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    setNow(Date.now())
  }, [scope])

  const [page, setPage] = useState(() => readStoredPage(scope))
  useEffect(() => {
    setPage(readStoredPage(scope))
  }, [scope])

  const cards = useMemo<BrewCard[]>(() => {
    const topicCards: BrewCard[] = topics.map((topic, i) => ({
      kind: 'topic' as const,
      key: `t${topic.key}`,
      topic,
      size: topicTileSize(i, topicMode, band),
    }))

    const sourceCards: BrewCard[] = sources.map((src) => ({
      kind: 'source' as const,
      key: `s${src.id}`,
      src,
      size: tileSize(brewScore(src, role, now), src, band, sources.length),
    }))

    return [...topicCards, ...sourceCards]
  }, [sources, topics, topicMode, band, role, now])

  const pages = useMemo(
    () =>
      packBrewCards(cards, cols, rows, {
        breakOn: breakOnCategory
          ? (prev, next) =>
              mainCategoryOf(prev, uncategorizedLabel) !==
              mainCategoryOf(next, uncategorizedLabel)
          : undefined,
      }),
    [cards, cols, rows, breakOnCategory, uncategorizedLabel],
  )

  useEffect(() => {
    if (!isSearching && pages.length > 0) {
      writeLayoutCache(scope, cols, rows, pages[0])
    }
  }, [pages, scope, cols, rows, isSearching])

  const pageCount = Math.max(1, pages.length)
  const clampedPage = Math.min(page, pageCount - 1)

  /**
   * 翻页 = 让 viewport 滚到那一页。用原生滚动而不是给轨道加 transform：
   * 一个被提升成合成层的祖先包着二十多张卡，会让整面墙的区域比周围暗一截
   * （像素实测边界亮度跳 ~20，去掉 transform 后与「隐藏整面墙」的对照组一致）。
   * 原生滚动还顺带把触控板横滑、触屏滑动都交给浏览器。
   */
  const scrollToPage = useCallback((index: number, smooth: boolean) => {
    const el = viewportRef.current
    if (!el) return
    el.scrollTo({ left: index * el.clientWidth, behavior: smooth ? 'smooth' : 'auto' })
  }, [])

  const goToPage = useCallback(
    (next: number) => {
      const bounded = Math.max(0, Math.min(next, pageCount - 1))
      setPage(bounded)
      writeStoredPage(scope, bounded)
      scrollToPage(bounded, true)
    },
    [pageCount, scope, scrollToPage],
  )

  // 首次渲染 / 换 scope 时，把已记住的页码对齐到滚动位置（不带动画）
  useLayoutEffect(() => {
    // 故意不依赖 page：这里只在换 scope / 页数变化时对齐一次，
    // 用户翻页由 goToPage 自己滚，别让它再触发一次跳变
    scrollToPage(Math.min(page, Math.max(0, pageCount - 1)), false)
  }, [scope, pageCount])

  // 用户自己滑（触控板 / 触屏 / 横向滚轮）时，把页码同步回来
  useEffect(() => {
    const el = viewportRef.current
    if (!el) return
    let raf = 0
    const sync = () => {
      cancelAnimationFrame(raf)
      raf = requestAnimationFrame(() => {
        const w = el.clientWidth
        if (w <= 0) return
        const idx = Math.max(0, Math.min(pageCount - 1, Math.round(el.scrollLeft / w)))
        setPage((prev) => {
          if (prev === idx) return prev
          writeStoredPage(scope, idx)
          return idx
        })
      })
    }
    el.addEventListener('scroll', sync, { passive: true })
    // 窗口变宽变窄后 scrollLeft 会落在两页之间，按当前页重新对齐
    const realign = () => scrollToPage(Math.round(el.scrollLeft / Math.max(1, el.clientWidth)), false)
    window.addEventListener('resize', realign)
    return () => {
      cancelAnimationFrame(raf)
      el.removeEventListener('scroll', sync)
      window.removeEventListener('resize', realign)
    }
  }, [pageCount, scope, scrollToPage])

  // ←/→ 切页。这两个键原本没被占用；j/k 仍是一维、按装箱顺序。
  useEffect(() => {
    if (isSearching || pageCount <= 1) return
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null
      if (
        target &&
        (target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          target.isContentEditable)
      ) {
        return
      }
      if (e.key === 'ArrowLeft') {
        e.preventDefault()
        goToPage(clampedPage - 1)
      } else if (e.key === 'ArrowRight') {
        e.preventDefault()
        goToPage(clampedPage + 1)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [isSearching, pageCount, clampedPage, goToPage])

  // 非当前页 / 滚出视口 / 标签页隐藏 → 暂停列表轮播
  const [offscreen, setOffscreen] = useState(false)
  useEffect(() => {
    const el = viewportRef.current
    if (!el || typeof IntersectionObserver === 'undefined') return
    const io = new IntersectionObserver(
      ([entry]) => setOffscreen(!entry.isIntersecting),
      { threshold: 0 },
    )
    io.observe(el)
    return () => io.disconnect()
  }, [])
  const [tabHidden, setTabHidden] = useState(
    () => typeof document !== 'undefined' && document.hidden,
  )
  useEffect(() => {
    const onVis = () => setTabHidden(document.hidden)
    document.addEventListener('visibilitychange', onVis)
    return () => document.removeEventListener('visibilitychange', onVis)
  }, [])

  const pageHeight = cell * rows

  // 搜索态：不装箱。统一 2×2 的规整网格，单页可滚动 —— 找特定源不需要层次。
  if (isSearching) {
    const perRow = Math.max(2, cols / 2)
    return (
      <div
        className="grid"
        style={{
          gridTemplateColumns: `repeat(${perRow}, minmax(0, 1fr))`,
          gap: TILE_PADDING * 2,
        }}
      >
        {sources.map((src) => (
          <div
            key={src.id}
            ref={(el) => registerCardRef?.(src.id, el)}
            style={{ height: standardCellSizeForBand(band) * 2 }}
          >
            <TileSlot
              size="2x2"
              render={({ scale, fontScale, containerRef }) => (
                <BrewSourceTile
                  surface="solid"
                  source={src}
                  size="2x2"
                  role={role}
                  now={now}
                  scale={scale}
                  fontScale={fontScale}
                  containerRef={containerRef}
                  onOpenSource={onSourceClick}
                />
              )}
            />
          </div>
        ))}
      </div>
    )
  }

  // category 排序：每页就是一个分类，标题写在页上方，而不是网格里塞一条横幅
  const pageTitle = breakOnCategory
    ? pageCategoryTitle(pages[clampedPage] ?? [], uncategorizedLabel ?? '')
    : null

  return (
    <div className="relative">
      {pageTitle ? (
        <div className="mb-2 flex items-baseline gap-2 px-1">
          <span className="text-[13px] font-semibold text-gray-700 dark:text-gray-200">
            {pageTitle}
          </span>
          <span className="text-[11px] text-gray-400 dark:text-gray-500">
            {pages[clampedPage]?.length ?? 0}
          </span>
        </div>
      ) : null}
      <div
        className="brew-wall-viewport"
        ref={viewportRef}
      >
        <div className="brew-wall-track" style={{ width: `${pageCount * 100}%` }}>
          {pages.map((packed, pageIndex) => {
            const paused = pageIndex !== clampedPage || offscreen || tabHidden
            return (
              <div
                key={pageIndex}
                className={`brew-wall-page ${paused ? 'brew-rotate-paused' : ''}`}
                style={{ width: `${100 / pageCount}%`, height: pageHeight }}
              >
                {packed.map((card) => {
                  const style = slotStyle(card, cols, rows)
                  if (card.kind === 'topic') {
                    return (
                      <div key={card.key} style={style}>
                        <TileSlot
                          size={card.size}
                          render={({ scale, fontScale, containerRef }) => (
                            <BrewTopicTile
                              surface="solid"
                              topic={card.topic}
                              size={card.size}
                              scale={scale}
                              fontScale={fontScale}
                              containerRef={containerRef}
                              onOpenTopic={onTopicClick}
                            />
                          )}
                        />
                      </div>
                    )
                  }

                  const src = card.src
                  const dragging = draggingSourceId === src.id
                  const dropTarget = dragOverSourceId === src.id
                  const selected = Boolean(selectedIds?.has(src.id))
                  const tileColor = src.theme_color ?? 'currentcolor'
                  return (
                    <div
                      key={card.key}
                      ref={(el) => registerCardRef?.(src.id, el)}
                      style={{
                        ...style,
                        opacity: dragging ? 0.35 : 1,
                        zIndex: dragging ? 3 : undefined,
                      }}
                      onMouseDown={
                        isEditMode && onDragStart
                          ? (e) => onDragStart(e, src.id)
                          : undefined
                      }
                      onTouchStart={
                        isEditMode && onDragStart
                          ? (e) => onDragStart(e, src.id)
                          : undefined
                      }
                    >
                      {/* 插入位：松手后卡片落在装箱结果上，不一定在鼠标下 */}
                      {dropTarget ? (
                        <span
                          className="brew-wall-drop-target"
                          style={{
                            inset: TILE_PADDING,
                            color: tileColor,
                          }}
                        />
                      ) : null}
                      {/* 选中环：老网格用 inset box-shadow，这里一样 */}
                      {selected ? (
                        <span
                          className="brew-wall-selected"
                          style={{
                            inset: TILE_PADDING,
                            boxShadow: `inset 0 0 0 2px ${tileColor}`,
                          }}
                        />
                      ) : null}
                      {isEditMode ? (
                        <div
                          className="brew-wall-edit-bar"
                          style={{ right: TILE_PADDING + 6, top: TILE_PADDING + 6 }}
                          // 别让操作条上的按下变成拖拽
                          onMouseDown={(e) => e.stopPropagation()}
                          onTouchStart={(e) => e.stopPropagation()}
                          onClick={(e) => e.stopPropagation()}
                        >
                          {onToggleSelect ? (
                            <button
                              type="button"
                              aria-label={editLabels?.select}
                              aria-pressed={selected}
                              onClick={() => onToggleSelect(src.id)}
                            >
                              <Check className="h-3.5 w-3.5" />
                            </button>
                          ) : null}
                          {onEditSource ? (
                            <button
                              type="button"
                              aria-label={editLabels?.edit}
                              onClick={() => onEditSource(src)}
                            >
                              <Edit3 className="h-3.5 w-3.5" />
                            </button>
                          ) : null}
                          {onRefreshSource && src.source_type !== 'link' ? (
                            <button
                              type="button"
                              aria-label={editLabels?.refresh}
                              onClick={() => onRefreshSource(src.id)}
                            >
                              <RefreshCw className="h-3.5 w-3.5" />
                            </button>
                          ) : null}
                          {onToggleSizeLock ? (
                            <button
                              type="button"
                              aria-label={
                                src.card_size ? editLabels?.unlock : editLabels?.lock
                              }
                              aria-pressed={Boolean(src.card_size)}
                              onClick={() =>
                                onToggleSizeLock(src, src.card_size ? null : card.size)
                              }
                            >
                              {src.card_size ? (
                                <Lock className="h-3.5 w-3.5" />
                              ) : (
                                <span className="text-[10px] font-medium tabular-nums">
                                  {card.size}
                                </span>
                              )}
                            </button>
                          ) : null}
                        </div>
                      ) : null}
                      <TileSlot
                        size={card.size}
                        render={({ scale, fontScale, containerRef }) => (
                          <BrewSourceTile
                            surface="solid"
                            source={src}
                            size={card.size}
                            role={role}
                            now={now}
                            scale={scale}
                            fontScale={fontScale}
                            containerRef={containerRef}
                            editMode={isEditMode}
                            onOpenSource={
                              isEditMode
                                ? onToggleSelect
                                  ? (x) => onToggleSelect(x.id)
                                  : undefined
                                : onSourceClick
                            }
                            onOpenItem={isEditMode ? undefined : onOpenItem}
                            onThemeColorExtracted={onThemeColorExtracted}
                          />
                        )}
                      />
                    </div>
                  )
                })}
              </div>
            )
          })}
        </div>
      </div>
      <BrewPager
        count={pageCount}
        current={clampedPage}
        onSelect={goToPage}
        labels={{
          prev: prevPageLabel,
          next: nextPageLabel,
          page: pageLabel,
        }}
      />
    </div>
  )
}

/** `category` 排序的换页依据：主分类变了就翻页。 */
function mainCategoryOf(card: BrewCard, uncategorized?: string): string {
  if (card.kind === 'topic') return `topic:${card.topic.key}`
  return brewMainCategory(card.src.category, uncategorized ?? '')
}

/** 每页第一张卡所属的主分类 —— `category` 排序的页标题。 */
export function pageCategoryTitle(
  packed: PackedCard[],
  uncategorized: string,
): string | null {
  const first = packed[0]
  if (!first || first.kind !== 'source') return null
  return brewMainCategory(first.src.category, uncategorized)
}

/**
 * 首屏骨架：读上次的装箱结果，按旧位置铺一层灰块。
 *
 * 必须挂在**父级的 loading 分支**上 —— 墙自己渲染时 sources 一定非空
 * （BrewSourceGrid 在空列表时走空态分支），内部再判空是死代码。
 *
 * 没有缓存就返回 null，让调用方回落到 spinner；宁可转圈也不要画一屏假格子。
 */
export function BrewWallSkeleton({ scope }: { scope: string }) {
  const band = useViewportBand()
  const cols = homeGridColsForBand(band)
  const layout = useMemo(() => readLayoutCache(scope, cols), [scope, cols])

  if (!layout || layout.slots.length === 0) return null

  return (
    <div className="brew-wall-viewport">
      <div
        className="brew-wall-page"
        style={{ width: '100%', height: standardCellSizeForBand(band) * layout.rows }}
        aria-hidden
      >
        {layout.slots.map((slot) => (
          <div key={slot.key} style={slotStyle(slot, cols, layout.rows)}>
            <div className="h-full w-full rounded-xl bg-black/4 dark:bg-white/5" />
          </div>
        ))}
      </div>
    </div>
  )
}
