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

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

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
function writeLayoutCache(
  scope: string,
  cols: number,
  rows: number,
  firstPage: PackedCard[],
): void {
  try {
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
}: BrewTileWallProps) {
  const band = useViewportBand()
  const cols = homeGridColsForBand(band)
  const cell = standardCellSizeForBand(band)
  const viewportRef = useRef<HTMLDivElement | null>(null)

  /**
   * 按可用高度派生行数。格子边长不变（还是首页那一套），只是一页多放几行 ——
   * /brew 是整页视图，固定 4 行会白扔掉半屏，还把源硬拆成更多页。
   */
  const [rows, setRows] = useState(ROWS_MIN)
  useEffect(() => {
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

  const goToPage = useCallback(
    (next: number) => {
      const bounded = Math.max(0, Math.min(next, pageCount - 1))
      setPage(bounded)
      writeStoredPage(scope, bounded)
    },
    [pageCount, scope],
  )

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

  /**
   * 翻页手势。只有 ←→ 和一排小圆点太难发现也太难用（用户反馈「很难切页」），
   * 这里补上滚轮/触控板横向滚动、Shift+滚轮、触屏横滑，外加两侧的箭头按钮。
   */
  const wheelLockRef = useRef(0)
  const onWheel = useCallback(
    (e: React.WheelEvent) => {
      if (isSearching || pageCount <= 1) return
      // 只吃横向意图，纵向留给页面滚动
      const dx = e.shiftKey ? e.deltaY : e.deltaX
      if (Math.abs(dx) < 12 || Math.abs(dx) < Math.abs(e.deltaY) * 0.8) return
      const now = Date.now()
      // 触控板一次滑动会连发几十个事件，加个节流免得一路翻到底
      if (now - wheelLockRef.current < 420) return
      wheelLockRef.current = now
      e.preventDefault()
      goToPage(clampedPage + (dx > 0 ? 1 : -1))
    },
    [isSearching, pageCount, clampedPage, goToPage],
  )

  const touchStartRef = useRef<{ x: number; y: number } | null>(null)
  const onTouchStart = useCallback((e: React.TouchEvent) => {
    const t = e.touches[0]
    touchStartRef.current = { x: t.clientX, y: t.clientY }
  }, [])
  const onTouchEnd = useCallback(
    (e: React.TouchEvent) => {
      const start = touchStartRef.current
      touchStartRef.current = null
      if (!start || isSearching || pageCount <= 1) return
      const t = e.changedTouches[0]
      const dx = start.x - t.clientX
      const dy = Math.abs(start.y - t.clientY)
      // 横向位移要明显压过纵向，否则是在滚页面
      if (Math.abs(dx) < 48 || Math.abs(dx) < dy) return
      goToPage(clampedPage + (dx > 0 ? 1 : -1))
    },
    [isSearching, pageCount, clampedPage, goToPage],
  )

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

  return (
    <div className="relative">
      <div
        className="brew-wall-viewport"
        ref={viewportRef}
        onWheel={onWheel}
        onTouchStart={onTouchStart}
        onTouchEnd={onTouchEnd}
      >
        <div
          className="brew-wall-track"
          style={{
            width: `${pageCount * 100}%`,
            transform: `translateX(-${(clampedPage * 100) / pageCount}%)`,
          }}
        >
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
                            color: src.theme_color ?? 'currentcolor',
                          }}
                        />
                      ) : null}
                      {isEditMode && onToggleSizeLock ? (
                        <button
                          type="button"
                          className="absolute z-[3] rounded-md bg-black/45 px-1.5 py-0.5 text-[10px] leading-none text-white/90 backdrop-blur-sm"
                          style={{
                            right: TILE_PADDING + 6,
                            bottom: TILE_PADDING + 6,
                          }}
                          title={src.card_size ? undefined : card.size}
                          onMouseDown={(e) => e.stopPropagation()}
                          onTouchStart={(e) => e.stopPropagation()}
                          onClick={(e) => {
                            e.stopPropagation()
                            onToggleSizeLock(
                              src,
                              src.card_size ? null : card.size,
                            )
                          }}
                        >
                          {src.card_size ? '🔒' : card.size}
                        </button>
                      ) : null}
                      <TileSlot
                        size={card.size}
                        render={({ scale, fontScale, containerRef }) => (
                          <BrewSourceTile
                            source={src}
                            size={card.size}
                            role={role}
                            now={now}
                            scale={scale}
                            fontScale={fontScale}
                            containerRef={containerRef}
                            onOpenSource={isEditMode ? undefined : onSourceClick}
                            onOpenItem={isEditMode ? undefined : onOpenItem}
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
