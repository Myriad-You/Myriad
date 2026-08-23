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
import { BrewPageDots } from './BrewPageDots'
import { brewMainCategory } from './constants'
import { tileSize, topicTileSize } from './logic/layout'
import { packBrewCards, parseTileSize } from './logic/pack'
import { brewScore } from './logic/score'
import { BrewSourceTile } from './tiles/BrewSourceTile'
import { BrewTopicTile } from './tiles/BrewTopicTile'
import './BrewTileWall.css'

/** 行数恒为 4，与首页一致。Demo 的 16×8 只是画布预览，不是生产。 */
const ROWS = 4
/** 磁贴之间的间距（每张卡自己吃 padding，不用 grid gap） */
const TILE_PADDING = 4
/** 布局缓存版本；改装箱规则就 +1，避免读到旧形状 */
const LAYOUT_CACHE_VERSION = 1

interface CachedSlot {
  key: string
  size: BrewTileSize
  x: number
  y: number
}

function layoutCacheKey(scope: string, cols: number): string {
  return `brew:wall:v${LAYOUT_CACHE_VERSION}:${cols}:${scope}`
}

function readLayoutCache(scope: string, cols: number): CachedSlot[] | null {
  try {
    const raw = globalThis.localStorage?.getItem(layoutCacheKey(scope, cols))
    if (!raw) return null
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? (parsed as CachedSlot[]) : null
  } catch {
    return null
  }
}

/** 只缓存第一页：骨架的意义是「首屏别重排」，后面的页翻到了再说。 */
function writeLayoutCache(
  scope: string,
  cols: number,
  firstPage: PackedCard[],
): void {
  try {
    const slim: CachedSlot[] = firstPage.map((c) => ({
      key: c.key,
      size: c.size,
      x: c.x,
      y: c.y,
    }))
    globalThis.localStorage?.setItem(
      layoutCacheKey(scope, cols),
      JSON.stringify(slim),
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
): CSSProperties {
  const span = parseTileSize(slot.size) ?? { w: 2, h: 2 }
  return {
    position: 'absolute',
    left: `${(slot.x / cols) * 100}%`,
    top: `${(slot.y / ROWS) * 100}%`,
    width: `${(span.w / cols) * 100}%`,
    height: `${(span.h / ROWS) * 100}%`,
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
}: BrewTileWallProps) {
  const band = useViewportBand()
  const cols = homeGridColsForBand(band)

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
      packBrewCards(cards, cols, ROWS, {
        breakOn: breakOnCategory
          ? (prev, next) =>
              mainCategoryOf(prev, uncategorizedLabel) !==
              mainCategoryOf(next, uncategorizedLabel)
          : undefined,
      }),
    [cards, cols, breakOnCategory, uncategorizedLabel],
  )

  // 骨架：上次的装箱结果。二次进入先按旧位置铺一层，避免整屏重排。
  const skeleton = useMemo(
    () => (sources.length === 0 ? readLayoutCache(scope, cols) : null),
    [sources.length, scope, cols],
  )

  useEffect(() => {
    if (!isSearching && pages.length > 0) {
      writeLayoutCache(scope, cols, pages[0])
    }
  }, [pages, scope, cols, isSearching])

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

  // 非当前页 / 滚出视口 / 标签页隐藏 → 暂停列表轮播
  const viewportRef = useRef<HTMLDivElement | null>(null)
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

  const pageHeight = standardCellSizeForBand(band) * ROWS

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

  // 骨架：只画位置，不画内容
  if (skeleton && skeleton.length > 0) {
    return (
      <div className="brew-wall-viewport" ref={viewportRef}>
        <div
          className="brew-wall-page"
          style={{ width: '100%', height: pageHeight }}
        >
          {skeleton.map((slot) => (
            <div key={slot.key} style={slotStyle(slot, cols)}>
              <div className="h-full w-full rounded-xl bg-black/4 dark:bg-white/5" />
            </div>
          ))}
        </div>
      </div>
    )
  }

  return (
    <div>
      <div className="brew-wall-viewport" ref={viewportRef}>
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
                  const style = slotStyle(card, cols)
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
      <BrewPageDots
        count={pageCount}
        current={clampedPage}
        onSelect={goToPage}
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

/** 一页容纳的行数（键盘 / 测试用）。 */
export { ROWS as BREW_WALL_ROWS }
