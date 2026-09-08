import type { CSSProperties } from 'react'
import type {
  LibraryCanvasLayout,
  LibraryCanvasTransform,
} from '../utils/libraryCanvas'
import type {
  LibraryLayoutMode,
  LibraryPreferencesUpdatedDetail,
} from '../utils/libraryPreferences'

import type { LibraryItem } from './library/libraryCanvasVisible'
import { FaVideo } from '@lib/icons'
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useI18n } from '../contexts/I18nContext'
import { useLibraryIntersectionObserver } from '../hooks/animation'
import { softLockWallpaperForLibraryCanvas } from '../hooks/useEvocativeWallpaper'
import { useLibraryCanvasControls } from '../hooks/useLibraryCanvasControls'
import { usePerformanceProfile } from '../hooks/usePerformanceProfile'
import { useSharedResize } from '../hooks/useSharedEventListener'
import {
  buildCenterOutCanvasLayout,
  getLibraryCanvasViewportBinKey,
} from '../utils/libraryCanvas'
import { slimLibraryItems } from '../utils/libraryItemSlim'
import {
  LIBRARY_PREFERENCES_UPDATED_EVENT,
  resolveLibraryLayoutMode,
} from '../utils/libraryPreferences'
import { isNeteaseVipFromMeta } from '../utils/musicPlayer'
import { getLibraryDataPageDeduped } from '../utils/requestDedup'
import { userFacingError } from '../utils/userFacingError'
import {
  LibraryCanvasChrome,
  syncLibraryCanvasChrome,
} from './library/LibraryCanvasChrome'
import {
  canvasCardPaintCacheIsCurrent,
  canvasFollowTargetsNeedPaint,
  createCanvasCardPaintCache,
  paintCanvasCardFocus,
  refreshCanvasCardPaintCache,
  resetCanvasCardPaintCache,
  sameLibraryItemIds,
} from './library/libraryCanvasPaint'
import {
  balancedShuffleLibraryItems,
  CANVAS_MAX_SCALE,
  CANVAS_MIN_SCALE,
  CANVAS_SPATIAL_BIN_SIZE,
  claimCanvasCardEnter,
  computeLibraryListLayout,
  LIBRARY_PAGE_SIZE,
  pickLibraryTourCardId,
  pinLibraryTourCard,
  queryCanvasVisibleItems,
  readCanvasDefaultScale,
} from './library/libraryCanvasVisible'
import {
  LibraryCardLyrics,
  useLibraryMusicIdentity,
} from './library/libraryCardLyrics'
import {
  getItemGridSize,
  getPlatformColor,
  getRatingBadgeStyle,
  getTypeIcon,
  hasUserRatingBadge,
  isBangumiPlatform,
  LibraryCardShell,
  openLibraryItemExternal,
  useLibraryCardActions,
} from './library/libraryCardShell'
import { LIBRARY_LIVE_MS } from './library/libraryLiveMs'
import { LibraryPlayingWaveBorder } from './library/libraryWaveBorder'
import PlatformIcon from './PlatformIcon'
import { QuickTransition } from './SkeletonTransition'
import { Spinner } from './Spinner'
import { getLibraryTourSurfaceSnapshot, isTourDomActive } from './tour/tourLogic'

function injectLibraryStyle(id: string, css: string) {
  if (typeof document === 'undefined') return
  let style = document.getElementById(id) as HTMLStyleElement | null
  if (!style) {
    style = document.createElement('style')
    style.id = id
    document.head.appendChild(style)
  }
  style.textContent = css
}
injectLibraryStyle(
  'library-grid-styles',
  `
        .animate-fade-in {
            animation: fadeIn 0.5s ease-out forwards;
        }

        @keyframes fadeIn {
            from {
                opacity: 0;
            }
            to {
                opacity: 1;
            }
        }
        /* 加载按钮样式 */
        .load-more-btn {
            padding: 0.625rem 1.5rem;
            border-radius: 0.5rem;
            transition: all 0.2s;
            font-size: 0.875rem;
            font-weight: 500;
            box-shadow: 0 1px 2px 0 rgba(0, 0, 0, 0.05);
        }

        .load-more-btn:hover {
            box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1);
        }

        /* 加载按钮主题色 */
        .load-more-btn.primary-load-btn {
            background-color: color-mix(in srgb, var(--color-primary, #3b82f6) 10%, transparent);
            color: var(--color-primary, #3b82f6);
            border: 1px solid color-mix(in srgb, var(--color-primary, #3b82f6) 20%, transparent);
        }

        .load-more-btn.primary-load-btn:hover {
            background-color: color-mix(in srgb, var(--color-primary, #3b82f6) 15%, transparent);
        }

        html[data-perf-mode='exlight'] .animate-fade-in {
            animation: none;
        }
  `,
)

interface LibraryResponse {
  success: boolean
  items: LibraryItem[]
  total: number
  has_more?: boolean
  next_offset?: number | null
  preferences?: {
    layout?: 'list' | 'canvas'
  }
}

type CardLayout = LibraryCanvasLayout

interface LibraryGridProps {
  filter: 'all' | 'game' | 'video' | 'music' | 'anime' | 'tv_series' | 'book'
}

export default function LibraryGrid({ filter }: LibraryGridProps) {
  const [allItems, setAllItems] = useState<LibraryItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [prevFilter, setPrevFilter] = useState<
    'all' | 'game' | 'video' | 'music' | 'anime' | 'tv_series' | 'book'
  >('all')
  const [isTransitioning, setIsTransitioning] = useState(false)
  const transitionTimersRef = useRef<ReturnType<typeof setTimeout>[]>([])

  // 布局状态：preferred 来自用户设置；低端设备强制列表（见 resolveLibraryLayoutMode）
  const [layouts, setLayouts] = useState<Map<string, CardLayout>>(new Map())
  const [preferredLayout, setPreferredLayout] =
    useState<LibraryLayoutMode>('list')
  const { highHardware } = usePerformanceProfile()
  const layoutMode = resolveLibraryLayoutMode(preferredLayout, highHardware)
  const [visibleCount, setVisibleCount] = useState(20) // 初始显示数量
  const [libraryHasMore, setLibraryHasMore] = useState(false)
  const nextLibraryOffsetRef = useRef<number | null>(null)
  const libraryPageLoadingRef = useRef(false)
  const libraryFetchGenerationRef = useRef(0)
  const loadNextLibraryPageRef = useRef<() => Promise<void>>(async () => {})
  const containerRef = useRef<HTMLDivElement>(null)
  // Mount-time only: avoid flipping scale when rotating/resizing mid-session.
  const canvasDefaultScaleRef = useRef(readCanvasDefaultScale())
  const worldRef = useRef<HTMLDivElement | null>(null)
  const canvasViewportRef = useRef({ width: 0, height: 0 })
  const libraryHasMoreRef = useRef(false)
  const canvasLoadedRadiusRef = useRef(0)
  const loadNextLibraryPageLiveRef = useRef<() => void>(() => {})
  const layoutsRef = useRef(new Map<string, CardLayout>())
  const laidOutItemsRef = useRef<LibraryItem[]>([])
  const canvasSpatialIndexRef = useRef<{
    bins: Map<string, LibraryItem[]>
    order: Map<string, number>
  }>({ bins: new Map(), order: new Map() })
  const lastCanvasVisibleItemsRef = useRef<LibraryItem[] | null>(null)
  const lastCanvasPaintPoseRef = useRef<{
    x: number
    y: number
    scale: number
  } | null>(null)
  const lastCanvasPaintWorldRef = useRef<HTMLElement | null>(null)
  const canvasPaintCacheRef = useRef(createCanvasCardPaintCache())
  const tourCardIdRef = useRef<string | null>(null)
  /** Tracks prior canvas effective mode for one-shot paint cleanup on leave. */
  const wasCanvasLayoutRef = useRef(false)
  const canvasTourNotifiedRef = useRef(false)
  const [canvasLiveVisibleItems, setCanvasLiveVisibleItems] = useState<
    LibraryItem[]
  >([])

  const paintCanvasTransform = useCallback(
    (t: LibraryCanvasTransform, syncCards = false) => {
      const surface = containerRef.current
      const world = worldRef.current
      const lastPose = lastCanvasPaintPoseRef.current
      const poseChanged =
        !lastPose ||
        lastPose.x !== t.x ||
        lastPose.y !== t.y ||
        lastPose.scale !== t.scale
      const followChanged = canvasFollowTargetsNeedPaint(
        lastPose,
        t,
        lastCanvasPaintWorldRef.current,
        world,
      )

      if (followChanged) {
        if (world) {
          world.style.transform = `translate3d(${t.x}px, ${t.y}px, 0) scale(${t.scale})`
        }
        if (surface) {
          surface.style.backgroundSize = `${28 * t.scale}px ${28 * t.scale}px`
          surface.style.backgroundPosition = `calc(50% + ${t.x}px) calc(50% + ${t.y}px)`
        }
        lastCanvasPaintPoseRef.current = { x: t.x, y: t.y, scale: t.scale }
        lastCanvasPaintWorldRef.current = world
      }

      const viewport = canvasViewportRef.current
      if (poseChanged || syncCards) {
        const cache = canvasPaintCacheRef.current
        if (world && viewport.width > 0 && viewport.height > 0) {
          if (syncCards || !canvasCardPaintCacheIsCurrent(cache, world)) {
            refreshCanvasCardPaintCache(cache, world)
          }
          paintCanvasCardFocus(cache.nodes, t, viewport)
        }

        const nextVisible = queryCanvasVisibleItems(
          t,
          viewport,
          layoutsRef.current,
          canvasSpatialIndexRef.current,
          laidOutItemsRef.current,
        )
        if (!sameLibraryItemIds(lastCanvasVisibleItemsRef.current, nextVisible)) {
          lastCanvasVisibleItemsRef.current = nextVisible
          setCanvasLiveVisibleItems(nextVisible)
        }

        syncLibraryCanvasChrome(t, {
          minScale: CANVAS_MIN_SCALE,
          maxScale: CANVAS_MAX_SCALE,
          defaultScale: canvasDefaultScaleRef.current,
        })
      }

      if (
        poseChanged &&
        libraryHasMoreRef.current &&
        canvasLoadedRadiusRef.current > 0 &&
        viewport.width > 0 &&
        viewport.height > 0
      ) {
        const worldCenterX = -t.x / t.scale
        const worldCenterY = -t.y / t.scale
        const viewportRadius =
          Math.hypot(viewport.width, viewport.height) / 2 / t.scale
        const preloadBoundary = Math.max(
          0,
          canvasLoadedRadiusRef.current - viewportRadius * 1.25,
        )
        if (Math.hypot(worldCenterX, worldCenterY) >= preloadBoundary) {
          loadNextLibraryPageLiveRef.current()
        }
      }
    },
    [],
  )

  // React transform commits when spatial bins change (chrome props) or on force flush.
  const shouldCommitCanvasTransform = useCallback(
    (next: LibraryCanvasTransform, committed: LibraryCanvasTransform) => {
      const viewport = canvasViewportRef.current
      if (viewport.width <= 0 || viewport.height <= 0) return true
      return (
        getLibraryCanvasViewportBinKey(
          next,
          viewport,
          CANVAS_SPATIAL_BIN_SIZE,
        ) !==
        getLibraryCanvasViewportBinKey(
          committed,
          viewport,
          CANVAS_SPATIAL_BIN_SIZE,
        )
      )
    },
    [],
  )

  const {
    atMaxZoom: canvasAtMaxZoom,
    atMinZoom: canvasAtMinZoom,
    finishPointer: finishCanvasPointer,
    handleBlur: handleCanvasBlur,
    handleClickCapture: handleCanvasClickCapture,
    handleKeyDown: handleCanvasKeyDown,
    handleKeyUp: handleCanvasKeyUp,
    handlePointerDown: handleCanvasPointerDown,
    handlePointerMove: handleCanvasPointerMove,
    isDefault: canvasViewIsDefault,
    reset: resetCanvasView,
    transform: canvasTransform,
    transformRef: canvasTransformRef,
    zoom: zoomCanvas,
    zoomPercent: canvasZoomPercent,
  } = useLibraryCanvasControls({
    active: layoutMode === 'canvas',
    defaultScale: canvasDefaultScaleRef.current,
    maxScale: CANVAS_MAX_SCALE,
    minScale: CANVAS_MIN_SCALE,
    surfaceRef: containerRef,
    onPaint: paintCanvasTransform,
    shouldCommit: shouldCommitCanvasTransform,
  })
  const [canvasViewport, setCanvasViewport] = useState({
    width: 0,
    height: 0,
  })
  canvasViewportRef.current = canvasViewport

  // Layout phase: mark canvas + cache wallpaper from-frame before paint so
  // entering /library from another route eases parallax out instead of hard-cutting.
  useLayoutEffect(() => {
    const root = document.documentElement
    const active = layoutMode === 'canvas'
    if (active) {
      root.dataset.libraryCanvas = 'active'
      softLockWallpaperForLibraryCanvas()
    } else if (
      root.dataset.libraryCanvas === 'active' ||
      root.dataset.libraryCanvasSurface ||
      root.dataset.libraryEmpty
    ) {
      delete root.dataset.libraryCanvas
      delete root.dataset.libraryCanvasSurface
      delete root.dataset.libraryEmpty
      window.dispatchEvent(new Event('libraryCanvasModeChanged'))
    }

    return () => {
      if (active && root.dataset.libraryCanvas === 'active') {
        delete root.dataset.libraryCanvas
        delete root.dataset.libraryCanvasSurface
        delete root.dataset.libraryEmpty
        window.dispatchEvent(new Event('libraryCanvasModeChanged'))
      }
    }
  }, [layoutMode])

  const containerWidthRef = useRef<number>(0) // 缓存容器宽度，避免重复读取
  // 父级只跟 songId / isPlaying / musicColor，切句不重渲染整表
  const {
    songId: liveSongId,
    isPlaying: globalIsPlaying,
    musicColor,
  } = useLibraryMusicIdentity()
  const { t } = useI18n()
  const { getExtraInfo, renderWatchProgressPanel, handlePlayMusic } =
    useLibraryCardActions()

  // 换歌/播完切走：旧卡保留离场窗口（见 LIBRARY_LIVE_MS.leaveHold）
  const prevLiveSongIdRef = useRef<string | null>(liveSongId)
  const [leavingSongId, setLeavingSongId] = useState<string | null>(null)
  useEffect(() => {
    const prev = prevLiveSongIdRef.current
    prevLiveSongIdRef.current = liveSongId
    if (prev && prev !== liveSongId) {
      // 注意：此处 return 后不会执行下面的 liveSongId===null 清理，
      // 否则会立刻清掉 leavingSongId，退场动画被掐断。
      setLeavingSongId(prev)
      const t = window.setTimeout(
        setLeavingSongId,
        LIBRARY_LIVE_MS.leaveHold,
        null,
      )
      return () => window.clearTimeout(t)
    }
    // 仅「本来就没有曲 / 清空」时卸离场标记（非换歌路径）
    if (!liveSongId) setLeavingSongId(null)
  }, [liveSongId])

  // 筛选后的所有项目
  const filteredAllItems = useMemo(() => {
    return filter === 'all'
      ? allItems
      : allItems.filter((item) => item.item_type === filter)
  }, [filter, allItems])

  // 核心布局算法：完全避免空隙
  const computeLayout = useCallback(() => {
    if (!containerRef.current || filteredAllItems.length === 0) return

    if (layoutMode === 'canvas') {
      const width = containerRef.current.offsetWidth
      const height = containerRef.current.offsetHeight
      setCanvasViewport((current) =>
        current.width === width && current.height === height
          ? current
          : { width, height },
      )
      setLayouts(
        buildCenterOutCanvasLayout(filteredAllItems, (item) =>
          getItemGridSize(item.item_type, item.platform),
        ),
      )
      return
    }

    // 使用缓存的容器宽度，避免强制重排
    // 只有缓存无效时才读取
    if (containerWidthRef.current === 0) {
      containerWidthRef.current = containerRef.current.offsetWidth
    }
    setLayouts(
      computeLibraryListLayout(
        filteredAllItems,
        containerWidthRef.current,
        (item) => getItemGridSize(item.item_type, item.platform),
      ),
    )
  }, [filteredAllItems, filter, layoutMode])

  // 使用共享的 resize 监听器
  useSharedResize(
    () => {
      // resize 时刷新容器宽度缓存
      if (containerRef.current) {
        containerWidthRef.current = containerRef.current.offsetWidth
        if (layoutMode === 'canvas') {
          setCanvasViewport({
            width: containerRef.current.offsetWidth,
            height: containerRef.current.offsetHeight,
          })
        }
      }
      computeLayout()
    },
    { debounce: 150 },
  )

  // 初始计算布局
  useEffect(() => {
    computeLayout()
  }, [computeLayout])

  // 滚动加载更多 -  添加节流防止过快触发
  const loadMoreRef = useRef<number | null>(null)
  const loadMore = useCallback(() => {
    if (loadMoreRef.current) return // 防止重复触发
    loadMoreRef.current = requestAnimationFrame(() => {
      if (visibleCount < filteredAllItems.length) {
        setVisibleCount((prev) => Math.min(prev + 20, filteredAllItems.length))
      } else if (libraryHasMore) {
        void loadNextLibraryPageRef.current().then(() => {
          setVisibleCount((prev) => prev + 20)
        })
      }
      loadMoreRef.current = null
    })
  }, [filteredAllItems.length, libraryHasMore, visibleCount])

  // 清理 RAF
  useEffect(() => {
    return () => {
      if (loadMoreRef.current) {
        cancelAnimationFrame(loadMoreRef.current)
      }
    }
  }, [])

  const hasMore =
    layoutMode === 'list' &&
    (visibleCount < filteredAllItems.length || libraryHasMore)

  // 🆕 使用资料库原子化 IntersectionObserver
  const { observeLibraryIntersection, unobserveLibraryIntersection } =
    useLibraryIntersectionObserver()
  const observerTarget = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (layoutMode !== 'list') return
    const target = observerTarget.current
    if (!target) return

    observeLibraryIntersection(target, (entry) => {
      if (entry.isIntersecting && hasMore) {
        loadMore()
      }
    })

    return () => unobserveLibraryIntersection(target)
  }, [
    hasMore,
    layoutMode,
    loadMore,
    observeLibraryIntersection,
    unobserveLibraryIntersection,
  ])

  const laidOutItems = useMemo(
    () => filteredAllItems.filter((item) => layouts.has(item.id)),
    [filteredAllItems, layouts],
  )

  // 画布空间索引只在数据或布局变化时重建；拖拽时仅查询视口附近的分桶。
  const canvasSpatialIndex = useMemo(() => {
    const bins = new Map<string, LibraryItem[]>()
    const order = new Map<string, number>()
    if (layoutMode !== 'canvas') return { bins, order }

    laidOutItems.forEach((item, index) => {
      const layout = layouts.get(item.id)
      if (!layout) return
      order.set(item.id, index)
      const minBinX = Math.floor(layout.left / CANVAS_SPATIAL_BIN_SIZE)
      const maxBinX = Math.floor(
        (layout.left + layout.width) / CANVAS_SPATIAL_BIN_SIZE,
      )
      const minBinY = Math.floor(layout.top / CANVAS_SPATIAL_BIN_SIZE)
      const maxBinY = Math.floor(
        (layout.top + layout.height) / CANVAS_SPATIAL_BIN_SIZE,
      )

      for (let binX = minBinX; binX <= maxBinX; binX++) {
        for (let binY = minBinY; binY <= maxBinY; binY++) {
          const key = `${binX},${binY}`
          const bin = bins.get(key)
          if (bin) bin.push(item)
          else bins.set(key, [item])
        }
      }
    })

    return { bins, order }
  }, [laidOutItems, layoutMode, layouts])

  layoutsRef.current = layouts
  laidOutItemsRef.current = laidOutItems
  canvasSpatialIndexRef.current = canvasSpatialIndex

  // 排序后的可见项目
  const visibleItems = useMemo(() => {
    if (layouts.size === 0) return []

    if (layoutMode === 'canvas') {
      // Live set is owned by paintCanvasTransform (absolute-follow pose).
      // Fall back while viewport is measuring or before the first paint.
      if (canvasLiveVisibleItems.length > 0) return canvasLiveVisibleItems
      return queryCanvasVisibleItems(
        canvasTransform,
        canvasViewport,
        layouts,
        canvasSpatialIndex,
        laidOutItems,
      )
    }

    // 按布局位置排序 (top, then left) - 实际上布局算法已经大致按顺序了，但为了确保渲染顺序
    const sortedItems = [...laidOutItems].sort((a, b) => {
      const layoutA = layouts.get(a.id)!
      const layoutB = layouts.get(b.id)!
      if (Math.abs(layoutA.top - layoutB.top) > 10)
        return layoutA.top - layoutB.top
      return layoutA.left - layoutB.left
    })

    return sortedItems.slice(0, visibleCount)
  }, [
    canvasLiveVisibleItems,
    canvasSpatialIndex,
    canvasTransform,
    canvasViewport,
    laidOutItems,
    layoutMode,
    layouts,
    visibleCount,
  ])

  const tourCardId = useMemo(() => {
    const picked = pickLibraryTourCardId(
      visibleItems,
      layoutMode === 'canvas' ? layouts : undefined,
    )
    if (isTourDomActive() && tourCardIdRef.current) return tourCardIdRef.current
    tourCardIdRef.current = picked
    return picked
  }, [layoutMode, layouts, visibleItems])

  const renderItems = useMemo(
    () =>
      layoutMode === 'canvas'
        ? pinLibraryTourCard(visibleItems, laidOutItems, tourCardId)
        : visibleItems,
    [laidOutItems, layoutMode, tourCardId, visibleItems],
  )

  // 动态计算容器高度
  const containerHeight = useMemo(() => {
    if (layoutMode === 'canvas') return 0
    if (visibleItems.length === 0) return 400
    let maxBottom = 0
    visibleItems.forEach((item) => {
      const layout = layouts.get(item.id)
      if (layout) {
        const bottom = layout.top + layout.height
        if (bottom > maxBottom) maxBottom = bottom
      }
    })
    return maxBottom + 20
  }, [layoutMode, visibleItems, layouts])

  const fetchLibraryData = useCallback(async () => {
    const generation = ++libraryFetchGenerationRef.current
    try {
      setLoading(true)
      setError(null)
      setAllItems([])
      nextLibraryOffsetRef.current = null
      libraryPageLoadingRef.current = false
      setLibraryHasMore(false)
      // 首屏只取一批；无限画布在接近已加载边界时继续扩展。
      const data: LibraryResponse = await getLibraryDataPageDeduped(
        0,
        LIBRARY_PAGE_SIZE,
        filter,
      )
      if (generation !== libraryFetchGenerationRef.current) return

      if (data.success) {
        const balanced = balancedShuffleLibraryItems(
          slimLibraryItems(data.items as LibraryItem[]),
        )
        setPreferredLayout(
          data.preferences?.layout === 'canvas' ? 'canvas' : 'list',
        )
        setAllItems(balanced)
        nextLibraryOffsetRef.current = data.next_offset ?? null
        setLibraryHasMore(Boolean(data.has_more && data.next_offset != null))
        setLoading(false)
      } else {
        throw new Error('No library data available')
      }
    } catch (err) {
      if (generation !== libraryFetchGenerationRef.current) return
      setError(userFacingError(err, t.library.loadFailed))
      setLoading(false)
    }
  }, [filter, t.library.loadFailed])

  useEffect(() => {
    void fetchLibraryData()
  }, [fetchLibraryData])

  useEffect(() => {
    const handlePreferencesUpdated = (event: Event) => {
      const detail = (event as CustomEvent<LibraryPreferencesUpdatedDetail>)
        .detail
      if (detail?.layout) setPreferredLayout(detail.layout)
      void fetchLibraryData()
    }
    window.addEventListener(
      LIBRARY_PREFERENCES_UPDATED_EVENT,
      handlePreferencesUpdated,
    )
    return () =>
      window.removeEventListener(
        LIBRARY_PREFERENCES_UPDATED_EVENT,
        handlePreferencesUpdated,
      )
  }, [fetchLibraryData])

  const loadNextLibraryPage = useCallback(async () => {
    const offset = nextLibraryOffsetRef.current
    if (offset == null || libraryPageLoadingRef.current) return

    const generation = libraryFetchGenerationRef.current
    libraryPageLoadingRef.current = true
    try {
      const data: LibraryResponse = await getLibraryDataPageDeduped(
        offset,
        LIBRARY_PAGE_SIZE,
        filter,
      )
      if (generation !== libraryFetchGenerationRef.current) return
      if (!data.success) throw new Error('No library data available')

      const incoming = balancedShuffleLibraryItems(
        slimLibraryItems(data.items as LibraryItem[]),
      )
      setAllItems((current) => {
        if (incoming.length === 0) return current
        const known = new Set(current.map((item) => item.id))
        const unique = incoming.filter((item) => !known.has(item.id))
        return unique.length > 0 ? [...current, ...unique] : current
      })
      nextLibraryOffsetRef.current = data.next_offset ?? null
      setLibraryHasMore(Boolean(data.has_more && data.next_offset != null))
    } catch (err) {
      if (err instanceof Error && err.name !== 'AbortError') {
        console.error('Failed to load the next library page:', err)
        setError(userFacingError(err, t.library.loadFailed))
      }
    } finally {
      if (generation === libraryFetchGenerationRef.current) {
        libraryPageLoadingRef.current = false
      }
    }
  }, [filter, t.library.loadFailed])

  loadNextLibraryPageRef.current = loadNextLibraryPage

  const canvasLoadedRadius = useMemo(() => {
    let radius = 0
    layouts.forEach((layout) => {
      const centerX = layout.left + layout.width / 2
      const centerY = layout.top + layout.height / 2
      const cardRadius = Math.hypot(layout.width, layout.height) / 2
      radius = Math.max(radius, Math.hypot(centerX, centerY) + cardRadius)
    })
    return radius
  }, [layouts])

  libraryHasMoreRef.current = libraryHasMore
  canvasLoadedRadiusRef.current = canvasLoadedRadius
  loadNextLibraryPageLiveRef.current = () => {
    void loadNextLibraryPage()
  }

  // After every React commit while canvas is active, re-paint the live pose.
  // Covers: virtualized card mount, music/live re-renders stomping chrome
  // disabled attrs, and first layout after viewport measure.
  // Leaving canvas: one-shot strip of absolute-follow surface paint (cards clear
  // via React list styles: transform none / zIndex auto).
  useLayoutEffect(() => {
    if (layoutMode === 'canvas') {
      wasCanvasLayoutRef.current = true
      paintCanvasTransform(canvasTransformRef.current, true)
      return
    }
    if (!wasCanvasLayoutRef.current) return
    wasCanvasLayoutRef.current = false
    resetCanvasCardPaintCache(canvasPaintCacheRef.current)
    lastCanvasPaintPoseRef.current = null
    lastCanvasPaintWorldRef.current = null
    lastCanvasVisibleItemsRef.current = null
    const surface = containerRef.current
    if (!surface) return
    surface.style.removeProperty('background-size')
    surface.style.removeProperty('background-position')
  })

  useEffect(() => {
    if (layoutMode !== 'canvas') return
    lastCanvasVisibleItemsRef.current = null
    resetCanvasView()
  }, [filter, layoutMode, resetCanvasView])

  // 空状态图标
  const emptyIcon = useMemo(
    () => (
      <svg
        className="w-5.5 h-5.5"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={2}
          d="M3.75 6A2.25 2.25 0 016 3.75h2.25A2.25 2.25 0 0110.5 6v2.25a2.25 2.25 0 01-2.25 2.25H6a2.25 2.25 0 01-2.25-2.25V6zM3.75 15.75A2.25 2.25 0 016 13.5h2.25a2.25 2.25 0 012.25 2.25V18a2.25 2.25 0 01-2.25 2.25H6A2.25 2.25 0 013.75 18v-2.25zM13.5 6a2.25 2.25 0 012.25-2.25H18A2.25 2.25 0 0120.25 6v2.25A2.25 2.25 0 0118 10.5h-2.25a2.25 2.25 0 01-2.25-2.25V6zM13.5 15.75a2.25 2.25 0 012.25-2.25H18a2.25 2.25 0 012.25 2.25V18A2.25 2.25 0 0118 20.25h-2.25A2.25 2.25 0 0113.5 18v-2.25z"
        />
      </svg>
    ),
    [],
  )

  const emptyTitle = error ? t.library.emptyLibrary : t.library.emptyCategory
  const showEmpty = !loading && filteredAllItems.length === 0

  // 画布 surface 在首屏加载后才挂上；偏好已在上一拍写下，这里补 surface 再通知。
  useLayoutEffect(() => {
    if (typeof document === 'undefined') return
    if (layoutMode !== 'canvas') {
      canvasTourNotifiedRef.current = false
      return
    }
    const root = document.documentElement
    const surfaceLive =
      !error && !showEmpty && !(loading && allItems.length === 0)
    const prev = getLibraryTourSurfaceSnapshot()
    if (surfaceLive) {
      root.dataset.libraryCanvasSurface = '1'
      delete root.dataset.libraryEmpty
    } else {
      delete root.dataset.libraryCanvasSurface
      if (showEmpty) root.dataset.libraryEmpty = '1'
      else delete root.dataset.libraryEmpty
    }
    const changed = getLibraryTourSurfaceSnapshot() !== prev
    if (changed || !canvasTourNotifiedRef.current) {
      canvasTourNotifiedRef.current = true
      window.dispatchEvent(new Event('libraryCanvasModeChanged'))
    }
  }, [allItems.length, error, layoutMode, loading, showEmpty])

  const needsTransition = (from: string, to: string) => {
    return from !== 'all' && to !== 'all' && from !== to
  }

  useEffect(() => {
    if (filter === prevFilter) return

    transitionTimersRef.current.forEach(clearTimeout)
    transitionTimersRef.current = []

    if (needsTransition(prevFilter, filter)) {
      setIsTransitioning(true)
      const swapTimer = setTimeout(() => {
        setPrevFilter(filter)
      }, 200)
      const finishTimer = setTimeout(setIsTransitioning, 350, false)
      transitionTimersRef.current = [swapTimer, finishTimer]
    } else {
      setPrevFilter(filter)
      setIsTransitioning(false)
    }
    // 切换分类时重置显示数量
    setVisibleCount(20)
  }, [filter, prevFilter])

  useEffect(() => {
    return () => {
      transitionTimersRef.current.forEach(clearTimeout)
      transitionTimersRef.current = []
    }
  }, [])

  // 首屏加载：单一 Spinner，垂直居中（扣除顶/底安全区，与 Brew 观感一致）
  if (loading && allItems.length === 0) {
    return (
      <div
        className="flex w-full items-center justify-center min-h-[calc(100dvh-12rem)] sm:min-h-[calc(100dvh-11rem)] md:min-h-[calc(100dvh-8rem)]"
        role="status"
      >
        <Spinner size="lg" color="primary" />
      </div>
    )
  }

  return (
    <div className="animate-in fade-in slide-in-from-bottom-4 duration-700 ease-out">
      {error || showEmpty ? (
        <div className="flex flex-col items-start py-8">
          <div className="library-empty rounded-2xl glass-surface glass-90 border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10 flex items-center gap-3 px-5 py-3">
            <div className="w-9 h-9 rounded-xl bg-gray-100/80 dark:bg-white/5 flex items-center justify-center text-gray-400 dark:text-gray-500 shrink-0">
              {emptyIcon}
            </div>
            <div className="min-w-0">
              <p className="text-sm font-medium text-gray-600 dark:text-gray-300">
                {emptyTitle}
              </p>
            </div>
          </div>
        </div>
      ) : (
        <div className={layoutMode === 'canvas' ? '' : 'space-y-8'}>
          <QuickTransition transitioning={isTransitioning}>
            <div
              ref={containerRef}
              className={
                layoutMode === 'canvas'
                  ? // inset-0 铺满 fixed 视口；勿再写死 h-dvh（iOS 地址栏伸缩时会短一截）
                    'fixed inset-0 z-0 min-h-lvh w-full overflow-hidden touch-none cursor-grab bg-white/5 dark:bg-black/5'
                  : 'relative w-full'
              }
              style={
                layoutMode === 'canvas'
                  ? {
                      // Size/position painted via paintCanvasTransform for absolute follow.
                      backgroundImage:
                        'radial-gradient(circle, color-mix(in srgb, var(--text-color, currentColor) 18%, transparent) 1px, transparent 1.2px)',
                    }
                  : {
                      height: `${containerHeight}px`,
                      minHeight: '400px',
                      // Dragging the custom scrollbar locks page-height math.
                      // Don't ease the grid taller mid-drag or the thumb slips.
                      transition:
                        typeof document !== 'undefined' &&
                        document.documentElement.dataset.scrollbarDragging ===
                          'true'
                          ? 'none'
                          : 'height 0.4s ease-out',
                    }
              }
              onPointerDown={
                layoutMode === 'canvas' ? handleCanvasPointerDown : undefined
              }
              onPointerMove={
                layoutMode === 'canvas' ? handleCanvasPointerMove : undefined
              }
              onPointerUp={
                layoutMode === 'canvas' ? finishCanvasPointer : undefined
              }
              onPointerCancel={
                layoutMode === 'canvas' ? finishCanvasPointer : undefined
              }
              onClickCapture={
                layoutMode === 'canvas' ? handleCanvasClickCapture : undefined
              }
              onKeyDown={
                layoutMode === 'canvas' ? handleCanvasKeyDown : undefined
              }
              onKeyUp={layoutMode === 'canvas' ? handleCanvasKeyUp : undefined}
              onBlur={layoutMode === 'canvas' ? handleCanvasBlur : undefined}
              tabIndex={layoutMode === 'canvas' ? 0 : undefined}
              data-library-canvas-surface={
                layoutMode === 'canvas' ? 'true' : undefined
              }
              data-tour={layoutMode === 'canvas' ? 'library-grid' : undefined}
              aria-label={
                layoutMode === 'canvas' ? t.library.canvasAriaLabel : undefined
              }
            >
              {layoutMode === 'canvas' && (
                <LibraryCanvasChrome
                  ariaLabel={t.library.canvasAriaLabel}
                  atMaxZoom={canvasAtMaxZoom}
                  atMinZoom={canvasAtMinZoom}
                  dismissHintLabel={t.library.canvasDismissHint}
                  hint={t.library.canvasPanHint}
                  mobileHint={t.library.canvasPanHintMobile}
                  isDefault={canvasViewIsDefault}
                  onReset={resetCanvasView}
                  onZoom={zoomCanvas}
                  resetLabel={t.library.canvasResetView}
                  zoomInLabel={t.library.canvasZoomIn}
                  zoomOutLabel={t.library.canvasZoomOut}
                  zoomPercent={canvasZoomPercent}
                />
              )}
              <div
                ref={layoutMode === 'canvas' ? worldRef : undefined}
                className={
                  layoutMode === 'canvas'
                    ? 'library-canvas-world absolute left-1/2 top-1/2'
                    : 'contents'
                }
                style={
                  layoutMode === 'canvas'
                    ? {
                        // Transform painted via paintCanvasTransform (absolute follow).
                        transformOrigin: '0 0',
                      }
                    : undefined
                }
              >
                {renderItems.map((item, itemIndex) => {
                  const layout = layouts.get(item.id)
                  if (!layout) return null

                  const platformColor = getPlatformColor(item.platform)
                  // VIP badge is Netease-only (fee/isVip); Bangumi music has no fee model
                  const isNeteaseMusic =
                    item.item_type === 'music' &&
                    !isBangumiPlatform(item.platform) &&
                    (item.platform.toLowerCase().includes('netease') ||
                      item.platform.toLowerCase().includes('网易') ||
                      item.id.startsWith('netease_'))
                  const isVip =
                    isNeteaseMusic && isNeteaseVipFromMeta(item.metadata)
                  const currentSongId = (
                    item.metadata.id || item.id.replace('netease_song_', '')
                  ).toString()

                  // 轻量身份：切句不刷整表；换歌离场保留短窗口
                  const isCurrentSong = liveSongId === currentSongId
                  const isLeavingSong = leavingSongId === currentSongId
                  const showMusicLive = isCurrentSong || isLeavingSong
                  const isPlaying = Boolean(isCurrentSong && globalIsPlaying)
                  // 播放中 + 退场窗口：锁 hover，避免中途放大/藏词打断动画
                  const hoverLocked = isPlaying || isLeavingSong

                  const rowIndex = Math.floor(layout.top / 300)
                  const listAnimationDelay = rowIndex * 0.05
                  const surfaceDragging =
                    layoutMode === 'canvas' &&
                    containerRef.current?.dataset.dragging === 'true'
                  const canvasEnterDelay =
                    layoutMode === 'canvas'
                      ? claimCanvasCardEnter(
                          item.id,
                          itemIndex,
                          Boolean(surfaceDragging),
                        )
                      : null
                  const canvasPriority = layoutMode === 'canvas'

                  // Bangumi / MAL 用户评分（0 表示未评分），显示在卡片左上角
                  const isBangumi = isBangumiPlatform(item.platform)
                  const userRate = hasUserRatingBadge(item.platform)
                    ? Number(
                        item.metadata.rate ?? item.metadata?.list_status?.score,
                      ) || 0
                    : 0
                  // Bangumi 游戏使用竖版，渲染为封面卡片
                  const isBangumiGame = isBangumi && item.item_type === 'game'

                  const ratingBadge =
                    userRate > 0
                      ? (() => {
                          const rs = getRatingBadgeStyle(userRate)
                          const animClass = rs.gloss
                            ? userRate >= 10
                              ? 'rating-badge-anim-max'
                              : 'rating-badge-anim'
                            : ''
                          return (
                            <div
                              className={`library-card-chrome absolute top-2.5 left-2.5 z-20 flex items-center justify-center overflow-hidden rounded-lg font-extrabold leading-none shadow-lg pointer-events-none ${rs.box} ${animClass}`}
                            >
                              {rs.gloss && (
                                <>
                                  <span className="absolute inset-x-0 top-0 h-1/2 bg-linear-to-b from-white/45 to-transparent" />
                                  <span className="rating-badge-shine" />
                                </>
                              )}
                              <span className="relative">{userRate}</span>
                            </div>
                          )
                        })()
                      : null

                  const platformCorner = (
                    <div className="absolute top-3 right-3 z-10 group/platform">
                      <div className="platform-icon-bg">
                        <PlatformIcon
                          platform={item.platform}
                          className="w-3.5 h-3.5"
                        />
                      </div>
                      <div className="absolute top-full right-0 mt-2 bg-black/90 text-white text-xs px-2.5 py-1 rounded-md opacity-0 group-hover/platform:opacity-100 transition-opacity duration-200 whitespace-nowrap pointer-events-none">
                        {item.platform}
                      </div>
                    </div>
                  )

                  return (
                    <div
                      key={item.id}
                      className={`absolute group library-card-container${hoverLocked ? ' is-hover-locked' : ''}`}
                      data-tour={
                        tourCardId === item.id ? 'library-card' : undefined
                      }
                      data-canvas-card={
                        layoutMode === 'canvas' ? '' : undefined
                      }
                      data-layout-left={
                        layoutMode === 'canvas' ? layout.left : undefined
                      }
                      data-layout-top={
                        layoutMode === 'canvas' ? layout.top : undefined
                      }
                      data-layout-width={
                        layoutMode === 'canvas' ? layout.width : undefined
                      }
                      data-layout-height={
                        layoutMode === 'canvas' ? layout.height : undefined
                      }
                      style={
                        {
                          left: `${layout.left}px`,
                          top: `${layout.top}px`,
                          width: `${layout.width}px`,
                          height: `${layout.height}px`,
                          '--platform-color': platformColor,
                          // List fadeInUp stagger only; canvas enter delay lives on the shell.
                          ...(layoutMode === 'list'
                            ? { animationDelay: `${listAnimationDelay}s` }
                            : {}),
                          // Canvas focus scale is painted each frame (absolute follow).
                          // List must set transform/zIndex so React clears leftover paint.
                          transformOrigin:
                            layoutMode === 'canvas'
                              ? 'center center'
                              : undefined,
                          transform:
                            layoutMode === 'canvas' ? undefined : 'none',
                          zIndex: layoutMode === 'canvas' ? undefined : 'auto',
                        } as CSSProperties
                      }
                      // 入场动画播放一次后移除，避免卡片滚出/滚入视口时
                      // 浏览器重建绘制层导致 fadeInUp 重播（表现为瞬间透明再恢复）
                      onAnimationEnd={(e) => {
                        if (e.target === e.currentTarget) {
                          ;(e.currentTarget as HTMLElement).style.animation =
                            'none'
                        }
                      }}
                    >
                      {item.item_type === 'music' ? (
                        <LibraryCardShell
                          cover={item.cover}
                          title={item.title}
                          coverBreathing={Boolean(isPlaying)}
                          priority={canvasPriority}
                          canvasEnterDelay={canvasEnterDelay}
                          className={
                            hoverLocked
                              ? 'bg-white rounded-xl shadow-md transition-shadow duration-300'
                              : 'bg-white rounded-xl shadow-md hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 hover:scale-[1.02]'
                          }
                          placeholder={
                            <div className="w-full h-full flex items-center justify-center bg-linear-to-br from-pink-400 to-pink-500">
                              <span className="text-6xl">
                                {getTypeIcon(item.item_type)}
                              </span>
                            </div>
                          }
                        >
                          <div
                            className="absolute inset-0 z-[1] cursor-pointer"
                            data-canvas-card-action
                            onClick={(e) => {
                              e.preventDefault()
                              e.stopPropagation()
                              handlePlayMusic(item)
                            }}
                          >
                            {showMusicLive && (
                              <>
                                {/* active=当前曲（含暂停，光晕冻结保留）；playing=频谱动画 */}
                                <LibraryPlayingWaveBorder
                                  musicColor={musicColor}
                                  active={isCurrentSong}
                                  playing={isPlaying}
                                />
                                {/* active=当前曲（含暂停）；换歌时 false 走退场 */}
                                <LibraryCardLyrics
                                  active={isCurrentSong}
                                  musicColor={musicColor}
                                />
                              </>
                            )}

                            <div className="library-card-chrome library-card-hover-chrome absolute inset-0 bg-linear-to-t from-black/95 via-black/60 to-transparent opacity-0 group-hover:opacity-100 transition-all duration-300 flex flex-col justify-end p-3">
                              <div>
                                <div className="flex items-start gap-1">
                                  <h3 className="font-bold text-white text-xs leading-tight line-clamp-2 mb-1 flex-1">
                                    {item.title}
                                  </h3>
                                  {isVip && (
                                    <span className="inline-flex items-center px-1.5 py-0.5 rounded-md bg-linear-to-r from-yellow-500 to-amber-600 text-[10px] font-semibold text-white shadow-md select-none">
                                      VIP
                                    </span>
                                  )}
                                </div>
                                {renderWatchProgressPanel(item, {
                                  dark: true,
                                }) ??
                                  (getExtraInfo(item) && (
                                    <p className="text-[10px] text-white/75 line-clamp-1">
                                      {getExtraInfo(item)}
                                    </p>
                                  ))}
                              </div>
                            </div>
                          </div>

                          {platformCorner}
                          {ratingBadge}
                        </LibraryCardShell>
                      ) : item.item_type === 'anime' ||
                        item.item_type === 'tv_series' ||
                        item.item_type === 'book' ||
                        isBangumiGame ? (
                        <LibraryCardShell
                          cover={item.cover}
                          title={item.title}
                          priority={canvasPriority}
                          canvasEnterDelay={canvasEnterDelay}
                          className="bg-white rounded-xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1"
                          placeholder={
                            <div className="w-full h-full flex items-center justify-center bg-linear-to-br from-pink-400 to-purple-500">
                              <span className="text-6xl">
                                <FaVideo />
                              </span>
                            </div>
                          }
                        >
                          <button
                            type="button"
                            className="absolute inset-0 z-[1] cursor-pointer text-left bg-transparent border-0 p-0"
                            data-canvas-card-action
                            aria-label={item.title}
                            onClick={(e) => {
                              e.preventDefault()
                              e.stopPropagation()
                              openLibraryItemExternal(item)
                            }}
                          >
                            <div className="absolute bottom-3 left-3 right-3 z-[1] flex justify-start pointer-events-none">
                              <div className="library-card-caption">
                                <div className="library-card-caption__row">
                                  <h3 className="library-card-caption__title line-clamp-1">
                                    {item.title}
                                  </h3>
                                  <span
                                    className={`library-card-caption__type ${
                                      item.item_type === 'anime'
                                        ? 'library-card-caption__type--anime'
                                        : item.item_type === 'book'
                                          ? 'library-card-caption__type--book'
                                          : item.item_type === 'game'
                                            ? 'library-card-caption__type--game'
                                            : 'library-card-caption__type--tv'
                                    }`}
                                  >
                                    {item.item_type === 'anime'
                                      ? t.library.anime
                                      : item.item_type === 'book'
                                        ? t.library.book
                                        : item.item_type === 'game'
                                          ? t.library.game
                                          : t.library.tvSeries}
                                  </span>
                                </div>
                                {renderWatchProgressPanel(item)}
                              </div>
                            </div>
                          </button>
                          {platformCorner}
                          {ratingBadge}
                        </LibraryCardShell>
                      ) : item.item_type === 'video' ? (
                        <LibraryCardShell
                          cover={item.cover}
                          title={item.title}
                          priority={canvasPriority}
                          canvasEnterDelay={canvasEnterDelay}
                          className="bg-white rounded-xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1"
                          placeholder={
                            <div className="w-full h-full flex items-center justify-center bg-linear-to-br from-blue-400 to-blue-500">
                              <span className="text-6xl">
                                {getTypeIcon(item.item_type)}
                              </span>
                            </div>
                          }
                        >
                          <button
                            type="button"
                            className="absolute inset-0 z-[1] cursor-pointer text-left bg-transparent border-0 p-0"
                            data-canvas-card-action
                            aria-label={item.title}
                            onClick={(e) => {
                              e.preventDefault()
                              e.stopPropagation()
                              openLibraryItemExternal(item)
                            }}
                          >
                            <div className="absolute bottom-3 left-3 right-3 z-[1] flex justify-start pointer-events-none">
                              <div className="library-card-caption">
                                <h3 className="library-card-caption__title line-clamp-2">
                                  {item.title}
                                </h3>
                                {renderWatchProgressPanel(item) ??
                                  (getExtraInfo(item) && (
                                    <p className="library-card-caption__meta">
                                      {getExtraInfo(item)}
                                    </p>
                                  ))}
                              </div>
                            </div>
                          </button>
                          {platformCorner}
                          {ratingBadge}
                        </LibraryCardShell>
                      ) : (
                        <LibraryCardShell
                          cover={item.cover}
                          title={item.title}
                          priority={canvasPriority}
                          canvasEnterDelay={canvasEnterDelay}
                          className="bg-white rounded-xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1"
                          placeholder={
                            <div className="w-full h-full flex items-center justify-center bg-linear-to-br from-purple-400 to-pink-500">
                              <span className="text-6xl">
                                {getTypeIcon(item.item_type)}
                              </span>
                            </div>
                          }
                        >
                          <button
                            type="button"
                            className="absolute inset-0 z-[1] cursor-pointer bg-transparent border-0 p-0"
                            data-canvas-card-action
                            aria-label={item.title}
                            onClick={(e) => {
                              e.preventDefault()
                              e.stopPropagation()
                              openLibraryItemExternal(item)
                            }}
                          />
                          <div className="library-card-chrome absolute inset-0 z-[1] bg-linear-to-t from-black/90 via-black/40 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-300 flex flex-col justify-end p-4 pointer-events-none">
                            <h3 className="font-bold text-white text-base line-clamp-2 leading-snug mb-1">
                              {item.title}
                            </h3>
                            {renderWatchProgressPanel(item, { dark: true }) ??
                              (getExtraInfo(item) && (
                                <p className="text-sm text-white/80">
                                  {getExtraInfo(item)}
                                </p>
                              ))}
                          </div>
                          {platformCorner}
                          {ratingBadge}
                        </LibraryCardShell>
                      )}
                    </div>
                  )
                })}
              </div>
            </div>
          </QuickTransition>

          {/* 无限滚动哨兵：不可见，避免底部常驻 Spinner 造成「卡住/双重加载」 */}
          {hasMore && (
            <div
              ref={observerTarget}
              className="h-px w-full"
              aria-hidden="true"
            />
          )}
        </div>
      )}
    </div>
  )
}
