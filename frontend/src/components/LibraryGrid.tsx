import type {
  WatchProgress,
  WatchProgressLabels,
} from '../utils/libraryWatchProgress'
import type { Song } from '../utils/musicPlayer'

import { FaBook, FaGamepad, FaMusic, FaVideo } from '@lib/icons'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
import { useMusicPlayerControl } from '../contexts/MusicPlayerContext'
import { useLibraryIntersectionObserver } from '../hooks/animation'
import { useSharedResize } from '../hooks/useSharedEventListener'
import {
  formatWatchProgressText,
  formatWatchStatusLabel,
  getWatchProgress,
} from '../utils/libraryWatchProgress'
import { getLibraryDataDeduped } from '../utils/requestDedup'
import { showInfo } from '../utils/toastManager'
import PlatformIcon from './PlatformIcon'
import { QuickTransition } from './SkeletonTransition'
import { Spinner } from './Spinner'

// 添加样式到页面
if (
  typeof document !== 'undefined' &&
  !document.getElementById('library-grid-styles')
) {
  const style = document.createElement('style')
  style.id = 'library-grid-styles'
  style.textContent = `
        /* 逐行显示动画 */
        @keyframes fadeInUp {
            from {
                opacity: 0;
                transform: translateY(20px);
            }
            to {
                opacity: 1;
                transform: translateY(0);
            }
        }

        .library-card-container {
            animation: fadeInUp 0.5s ease-out backwards;
        }

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

        /* 卡片容器样式 */
        .library-card-container {
            transition: left 0.4s ease-out, top 0.4s ease-out, width 0.4s ease-out, height 0.4s ease-out;
            /* 固定 GPU 合成层，避免卡片滚出/滚入视口时
               backdrop-filter 触发浏览器丢弃并重建绘制层（表现为瞬间透明再恢复） */
            transform: translateZ(0);
            -webkit-backface-visibility: hidden;
            backface-visibility: hidden;
        }

        /* 平台图标背景 */
        .platform-icon-bg {
            width: 2.5rem;
            height: 2.5rem;
            border-radius: 9999px;
            backdrop-filter: blur(12px);
            /* 让模糊层拥有独立稳定的合成层，消除滚动闪烁 */
            transform: translateZ(0);
            display: flex;
            align-items: center;
            justify-content: center;
            box-shadow: 0 10px 15px -3px rgba(0, 0, 0, 0.1);
            transition: all 0.3s;
            background-color: color-mix(in srgb, var(--platform-color, #6b7280) 8%, transparent);
        }

        .platform-icon-bg svg,
        .platform-icon-bg img {
            color: var(--platform-color, #6b7280);
            transition: color 0.3s ease;
        }

        /* 播放中的平台图标颜色变化 */
        .playing-breath svg,
        .playing-breath img {
            color: var(--music-color, var(--platform-color, #ef4444)) !important;
            filter: drop-shadow(0 0 4px color-mix(in srgb, var(--music-color, #ef4444) 40%, transparent));
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

        /* 播放中指示器 - 中央显示，保留原呼吸动画和旋转边框效果 */
        @keyframes breath {
            0%, 100% {
                box-shadow: 0 0 0 0 color-mix(in srgb, var(--music-color, var(--platform-color, #ef4444)) 60%, transparent),
                            0 0 15px 0 color-mix(in srgb, var(--music-color, var(--platform-color, #ef4444)) 30%, transparent);
                transform: translate(-50%, -50%) scale(1);
            }
            50% {
                box-shadow: 0 0 0 8px transparent,
                            0 0 25px 5px color-mix(in srgb, var(--music-color, var(--platform-color, #ef4444)) 20%, transparent);
                transform: translate(-50%, -50%) scale(1.05);
            }
        }

        /* 旋转复用全局 @keyframes spin（animations.css / App 全局导入） */

        .playing-indicator {
            position: absolute;
            top: 50%;
            left: 50%;
            transform: translate(-50%, -50%);
            width: 5rem;
            height: 5rem;
            border-radius: 9999px;
            backdrop-filter: blur(12px);
            display: flex;
            align-items: center;
            justify-content: center;
            animation: breath 2s ease-in-out infinite;
            z-index: 10;
            pointer-events: none;
            border: 2px solid color-mix(in srgb, var(--music-color, var(--platform-color, #ef4444)) 40%, transparent);
        }

        /* 统一使用大幅透明背景，不区分亮色/暗色模式 */
        .playing-indicator {
            background-color: color-mix(in srgb, var(--music-color, #ef4444) 8%, transparent);
        }

        .playing-indicator::before {
            content: '';
            position: absolute;
            inset: -3px;
            border-radius: 9999px;
            background: conic-gradient(from 0deg,
                transparent 0deg,
                color-mix(in srgb, var(--music-color, var(--platform-color, #ef4444)) 30%, transparent) 90deg,
                color-mix(in srgb, var(--music-color, var(--platform-color, #ef4444)) 50%, transparent) 180deg,
                color-mix(in srgb, var(--music-color, var(--platform-color, #ef4444)) 30%, transparent) 270deg,
                transparent 360deg);
            animation: spin 3s linear infinite;
            pointer-events: none;
            z-index: -1;
        }

        .playing-indicator svg,
        .playing-indicator img {
            width: 2rem;
            height: 2rem;
            color: var(--music-color, var(--platform-color, #ef4444)) !important;
            filter: drop-shadow(0 0 4px color-mix(in srgb, var(--music-color, #ef4444) 40%, transparent));
        }

        /* 高分评分徽章 - 呼吸光晕 */
        @keyframes ratingGlow {
            0%, 100% {
                box-shadow: 0 2px 8px 0 color-mix(in srgb, #f59e0b 40%, transparent);
            }
            50% {
                box-shadow: 0 2px 18px 2px color-mix(in srgb, #f59e0b 75%, transparent);
            }
        }

        .rating-badge-anim {
            animation: ratingGlow 2.4s ease-in-out infinite;
        }

        /* 满分（10）更强更快 */
        .rating-badge-anim-max {
            animation: ratingGlow 1.8s ease-in-out infinite;
        }

        /* 高分评分徽章 - 流光扫过 */
        @keyframes ratingShine {
            0% { transform: translateX(-180%) skewX(-20deg); }
            16%, 100% { transform: translateX(320%) skewX(-20deg); }
        }

        .rating-badge-shine {
            position: absolute;
            top: 0;
            bottom: 0;
            width: 45%;
            background: linear-gradient(90deg, transparent, rgba(255,255,255,0.75), transparent);
            animation: ratingShine 8s ease-in-out infinite;
            pointer-events: none;
        }
    `
  document.head.appendChild(style)
}

interface LibraryItem {
  id: string
  item_type: 'game' | 'video' | 'music' | 'anime' | 'tv_series' | 'book'
  title: string
  cover: string | null
  platform: string
  metadata: any
}

interface LibraryResponse {
  success: boolean
  items: LibraryItem[]
  total: number
}

interface CardLayout {
  left: number
  top: number
  width: number
  height: number
  gridX: number
  gridY: number
  gridW: number
  gridH: number
}

interface LibraryGridProps {
  filter: 'all' | 'game' | 'video' | 'music' | 'anime' | 'tv_series' | 'book'
}

// 判断是否为 Bangumi 平台
function isBangumiPlatform(platform: string) {
  return platform.toLowerCase() === 'bangumi'
}

// 判断是否为 MyAnimeList 平台
function isMalPlatform(platform: string) {
  const key = platform.toLowerCase().replace(/[\s_-]/g, '')
  return key === 'myanimelist' || key === 'mal'
}

// 是否展示用户评分徽章（Bangumi / MyAnimeList 等 1–10 分制）
function hasUserRatingBadge(platform: string) {
  return isBangumiPlatform(platform) || isMalPlatform(platform)
}

// Bangumi 用户评分徽章样式（仿 Metacritic 分色标记）
// 分数越高越推荐 —— 色彩越暖、尺寸越大、越醒目
function getRatingBadgeStyle(rate: number) {
  // 满分（10）：金色渐变，最大最亮，双环 + 光晕，独享的稀有感
  if (rate >= 10) {
    return {
      box: 'w-10 h-10 text-xl bg-linear-to-br from-amber-300 via-yellow-400 to-orange-500 text-white ring-2 ring-amber-200/80 ring-offset-1 ring-offset-amber-500/30 shadow-amber-400/60',
      gloss: true,
    }
}
  // 神作（9）：金色渐变 + 光晕
  if (rate >= 9) {
    return {
      box: 'w-9 h-9 text-lg bg-linear-to-br from-amber-300 to-orange-500 text-white ring-2 ring-amber-200/70 shadow-amber-500/50',
      gloss: true,
    }
}
  // 力荐（8）
  if (rate >= 8) {
    return {
      box: 'w-8 h-8 text-base bg-emerald-500 text-white ring-1 ring-emerald-300/50 shadow-emerald-500/40',
      gloss: false,
    }
}
  // 推荐（7）
  if (rate >= 7) {
    return {
      box: 'w-8 h-8 text-base bg-green-500 text-white shadow-green-500/30',
      gloss: false,
    }
}
  // 还行（6）
  if (rate >= 6) {
    return {
      box: 'w-7 h-7 text-sm bg-lime-500 text-white',
      gloss: false,
    }
}
  // 不过不失（5）
  if (rate >= 5) {
    return {
      box: 'w-7 h-7 text-sm bg-amber-500 text-white',
      gloss: false,
    }
}
  // 较差（3-4）
  if (rate >= 3) {
    return {
      box: 'w-7 h-7 text-sm bg-orange-500 text-white',
      gloss: false,
    }
}
  // 差评（1-2）
  return {
    box: 'w-7 h-7 text-sm bg-rose-500 text-white',
    gloss: false,
  }
}

// 获取项目在网格中的尺寸 (w, h)
function getItemGridSize(type: string, platform: string) {
  switch (type) {
    case 'game':
      // Bangumi 游戏使用竖版封面，其余（如 Steam）保持横版
      return isBangumiPlatform(platform) ? { w: 1, h: 2 } : { w: 2, h: 1 }
    case 'video':
      return { w: 2, h: 1 }
    case 'anime':
    case 'tv_series':
    case 'book':
      return { w: 1, h: 2 }
    case 'music':
    default:
      return { w: 1, h: 1 }
  }
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

  // 布局状态
  const [layouts, setLayouts] = useState<Map<string, CardLayout>>(new Map())
  const [visibleCount, setVisibleCount] = useState(20) // 初始显示数量

  const containerRef = useRef<HTMLDivElement>(null)
  const containerWidthRef = useRef<number>(0) // 🔧 缓存容器宽度，避免重复读取
  const {
    playSong,
    currentSong,
    isPlaying: globalIsPlaying,
    musicColor,
  } = useMusicPlayerControl()
  const { t } = useI18n()

  // 使用 ref 存储回调函数，避免在依赖中频繁更新
  const playSongRef = useRef(playSong)
  playSongRef.current = playSong

  // 筛选后的所有项目
  const filteredAllItems = useMemo(() => {
    return filter === 'all'
      ? allItems
      : allItems.filter((item) => item.item_type === filter)
  }, [filter, allItems])

  // 核心布局算法：完全避免空隙
  const computeLayout = useCallback(() => {
    if (!containerRef.current || filteredAllItems.length === 0) return

    // 🔧 使用缓存的容器宽度，避免强制重排
    // 只有缓存无效时才读取
    if (containerWidthRef.current === 0) {
      containerWidthRef.current = containerRef.current.offsetWidth
    }
    const containerWidth = containerWidthRef.current
    const gap = 16
    let columns = 5

    // 响应式列数
    if (containerWidth < 640) columns = 2
    else if (containerWidth < 768) columns = 3
    else if (containerWidth < 1024) columns = 4
    else if (containerWidth < 1536) columns = 5
    else columns = 6

    const baseWidth = (containerWidth - gap * (columns + 1)) / columns
    const uniformHeight = baseWidth

    // 居中逻辑修正：针对纯宽卡片（2x1）在奇数列数下的居中处理
    let startOffset = 0
    let layoutColumns = columns

    // 如果当前筛选下只有宽2的卡片（如纯 Steam 游戏或视频分类），且列数是奇数
    // 那么最后一列无法被填满（因为没有宽1的卡片），导致整体偏左
    // 需要计算偏移量使内容居中
    // 注意：Bangumi 游戏为竖版（宽1），与 Steam 游戏混排时不应触发此居中
    const allWideCards =
      filteredAllItems.length > 0 &&
      filteredAllItems.every(
        (item) => getItemGridSize(item.item_type, item.platform).w === 2,
      )
    if (allWideCards && columns % 2 !== 0 && columns > 1) {
      layoutColumns = columns - 1
      // 剩余空间 = 1个列宽 + 1个间隙
      // 偏移量 = 剩余空间 / 2
      startOffset = (baseWidth + gap) / 2
    }

    // 1. 准备队列：按尺寸分类，保持原始相对顺序
    const queues: Record<
      string,
      { item: LibraryItem; originalIndex: number }[]
    > = {
      '1x1': [],
      '1x2': [],
      '2x1': [],
      // '2x2': [] // 暂无2x2类型
    }

    filteredAllItems.forEach((item, index) => {
      const size = getItemGridSize(item.item_type, item.platform)
      const key = `${size.w}x${size.h}`
      if (queues[key]) {
        queues[key].push({ item, originalIndex: index })
      } else {
        // 默认归为 1x1
        queues['1x1'].push({ item, originalIndex: index })
      }
    })

    // 2. 网格状态追踪
    // 使用 Map 记录被占用的格子 "x,y" -> true
    const occupied = new Set<string>()
    const isOccupied = (x: number, y: number) => occupied.has(`${x},${y}`)
    const markOccupied = (x: number, y: number, w: number, h: number) => {
      for (let i = 0; i < w; i++) {
        for (let j = 0; j < h; j++) {
          occupied.add(`${x + i},${y + j}`)
        }
      }
    }

    const newLayouts = new Map<string, CardLayout>()
    let maxY = 0
    let placedCount = 0
    const totalItems = filteredAllItems.length

    // 3. 遍历网格填充
    // y 从 0 开始无限增长，x 从 0 到 columns-1
    let y = 0
    while (placedCount < totalItems) {
      for (let x = 0; x < layoutColumns; x++) {
        if (isOccupied(x, y)) continue

        // 发现空位 (x, y)
        // 尝试寻找最佳匹配项
        // 优先级：
        // 1. 检查是否能放入 2x1 (需要 x+1 空闲)
        // 2. 检查是否能放入 1x2 (需要 y+1 空闲 - 总是假设 y+1 空闲，除非有预占，但这里我们是逐行扫描，y+1通常未处理)
        //    注意：如果之前有 1x2 占据了 (x, y+1)，则 isOccupied(x, y+1) 会为 true。
        // 3. 放入 1x1

        // 为了保持"平均开始排布"，我们在所有能放入的候选中，选择 originalIndex 最小的那个

        const candidates: {
          type: string
          index: number
          item: LibraryItem
          w: number
          h: number
        }[] = []

        // 检查 1x1
        if (queues['1x1'].length > 0) {
          const qItem = queues['1x1'][0]
          candidates.push({
            item: qItem.item,
            index: qItem.originalIndex,
            type: '1x1',
            w: 1,
            h: 1,
          })
        }

        // 检查 2x1
        const canFit2x1 = x + 1 < layoutColumns && !isOccupied(x + 1, y)
        if (canFit2x1 && queues['2x1'].length > 0) {
          const qItem = queues['2x1'][0]
          candidates.push({
            item: qItem.item,
            index: qItem.originalIndex,
            type: '2x1',
            w: 2,
            h: 1,
          })
        }

        // 检查 1x2
        // 垂直方向通常是无限的，但要检查是否被上方的某些长条物体阻挡？
        // 我们是按 y 递增扫描，所以 (x, y+1) 只有可能被之前的操作占据（不太可能，除非有复杂形状）
        // 但为了严谨，检查一下
        const canFit1x2 = !isOccupied(x, y + 1)
        if (canFit1x2 && queues['1x2'].length > 0) {
          const qItem = queues['1x2'][0]
          candidates.push({
            item: qItem.item,
            index: qItem.originalIndex,
            type: '1x2',
            w: 1,
            h: 2,
          })
        }

        if (candidates.length === 0) {
          // 没有剩余物品能放入此格
          // 只能留空 (虽然用户说避免空白，但如果没有物品了就没办法)
          // 或者：如果只有 2x1 且当前只有 1格宽，那必须留空
          // 标记此格为"跳过/虚拟占用"以继续循环?
          // 不，直接 continue，外层循环会处理下一个 x
          // 但如果不标记，下次循环回来还是空的。
          // 所以必须标记为"废弃"
          // 但如果后续还有物品，只是当前放不下（比如只有2x1但这里只有1格），那这个格子就真的废了
          // 除非我们能从后面拉一个 1x1 过来。但如果 1x1 队列空了，那就真没办法。
          // 标记为占用，但不放置物品
          // occupied.add(`${x},${y}`); // 实际上不需要显式add，只要不处理就行，但为了算法推进，视为已处理
          continue
        }

        // 选择 originalIndex 最小的候选者
        candidates.sort((a, b) => a.index - b.index)
        const best = candidates[0]

        // 放置物品
        const queue = queues[best.type as keyof typeof queues]
        queue.shift() // 移除已使用的

        // 计算像素位置
        const left = gap + x * (baseWidth + gap) + startOffset
        const top = gap + y * (uniformHeight + gap)
        const width = best.w * baseWidth + (best.w - 1) * gap
        const height = best.h * uniformHeight + (best.h - 1) * gap

        newLayouts.set(best.item.id, {
          left,
          top,
          width,
          height,
          gridX: x,
          gridY: y,
          gridW: best.w,
          gridH: best.h,
        })

        markOccupied(x, y, best.w, best.h)
        placedCount++

        // 更新最大高度
        const itemBottom = top + height
        if (itemBottom > maxY) maxY = itemBottom
      }

      // 检查当前行是否还有未处理的空位（被跳过的）
      // 如果所有列都处理过（占用或尝试过），进入下一行
      y++

      // 安全阀：防止死循环 (如果数据异常)
      if (y > totalItems * 2) break
    }

    setLayouts(newLayouts)
  }, [filteredAllItems, filter])

  // 使用共享的 resize 监听器
  useSharedResize(
    () => {
      // 🔧 resize 时刷新容器宽度缓存
      if (containerRef.current) {
        containerWidthRef.current = containerRef.current.offsetWidth
      }
      computeLayout()
    },
    { debounce: 150 },
  )

  // 初始计算布局
  useEffect(() => {
    computeLayout()
  }, [computeLayout])

  // 滚动加载更多 - 🔧 添加节流防止过快触发
  const loadMoreRef = useRef<number | null>(null)
  const loadMore = useCallback(() => {
    if (loadMoreRef.current) return // 防止重复触发
    loadMoreRef.current = requestAnimationFrame(() => {
      setVisibleCount((prev) => Math.min(prev + 20, filteredAllItems.length))
      loadMoreRef.current = null
    })
  }, [filteredAllItems.length])

  // 清理 RAF
  useEffect(() => {
    return () => {
      if (loadMoreRef.current) {
        cancelAnimationFrame(loadMoreRef.current)
      }
    }
  }, [])

  const hasMore = visibleCount < filteredAllItems.length

  // 🆕 使用资料库原子化 IntersectionObserver
  const { observeLibraryIntersection, unobserveLibraryIntersection } =
    useLibraryIntersectionObserver()
  const observerTarget = useRef<HTMLDivElement>(null)

  useEffect(() => {
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
    loadMore,
    observeLibraryIntersection,
    unobserveLibraryIntersection,
  ])

  // 排序后的可见项目
  const visibleItems = useMemo(() => {
    if (layouts.size === 0) return []

    // 获取所有已布局的项目
    const laidOutItems = filteredAllItems.filter((item) => layouts.has(item.id))

    // 按布局位置排序 (top, then left) - 实际上布局算法已经大致按顺序了，但为了确保渲染顺序
    laidOutItems.sort((a, b) => {
      const layoutA = layouts.get(a.id)!
      const layoutB = layouts.get(b.id)!
      if (Math.abs(layoutA.top - layoutB.top) > 10)
        return layoutA.top - layoutB.top
      return layoutA.left - layoutB.left
    })

    return laidOutItems.slice(0, visibleCount)
  }, [filteredAllItems, layouts, visibleCount])

  // 动态计算容器高度
  const containerHeight = useMemo(() => {
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
  }, [visibleItems, layouts])

  useEffect(() => {
    fetchLibraryData()
  }, [])

  const fetchLibraryData = async () => {
    try {
      setLoading(true)
      // 使用去重版本，避免多组件同时请求
      const data: LibraryResponse = await getLibraryDataDeduped()

      if (data.success) {
        const balanced = balancedShuffle(data.items)
        setAllItems(balanced)
        setLoading(false)
      } else {
        throw new Error('No library data available')
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Unknown error'
      setError(message)
      setLoading(false)
    }
  }

  const balancedShuffle = (items: LibraryItem[]): LibraryItem[] => {
    const groups: Record<string, LibraryItem[]> = {
      game: [],
      video: [],
      music: [],
      anime: [],
      tv_series: [],
      book: [],
    }

    items.forEach((item) => {
      const type = item.item_type
      if (groups[type]) {
        groups[type].push(item)
      }
    })

    Object.keys(groups).forEach((key) => {
      groups[key].sort(() => Math.random() - 0.5)
    })

    const result: LibraryItem[] = []
    const maxLength = Math.max(
      groups.game.length,
      groups.video.length,
      groups.music.length,
      groups.anime.length,
      groups.tv_series.length,
      groups.book.length,
    )

    for (let i = 0; i < maxLength; i++) {
      const typeOrder = [
        'game',
        'video',
        'music',
        'anime',
        'tv_series',
        'book',
      ].sort(() => Math.random() - 0.5)
      typeOrder.forEach((type) => {
        if (groups[type][i]) {
          result.push(groups[type][i])
        }
      })
    }

    return result
  }

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

  const getPlatformColor = useCallback((platform: string) => {
    switch (platform.toLowerCase()) {
      case 'bilibili':
        return '#00A1D6'
      case 'steam':
        return '#171a21'
      case 'netease music':
      case 'netease':
        return '#d33a31'
      case 'github':
        return '#24292e'
      case 'bangumi':
        return '#f09199'
      case 'mal':
      case 'myanimelist':
        return '#2E51A2'
      case 'x':
      case 'twitter':
        return '#000000'
      case 'discord':
        return '#5865F2'
      default:
        return '#6b7280'
    }
  }, [])

  const getTypeIcon = useCallback((type: string) => {
    switch (type) {
      case 'game':
        return <FaGamepad />
      case 'video':
      case 'anime':
      case 'tv_series':
        return <FaVideo />
      case 'music':
        return <FaMusic />
      case 'book':
        return <FaBook />
      default:
        return <FaBook />
    }
  }, [])

  const watchProgressLabels = useMemo<WatchProgressLabels>(
    () => ({
      progressEp: t.library.progressEp,
      progressEpOnly: t.library.progressEpOnly,
      progressCh: t.library.progressCh,
      progressChOnly: t.library.progressChOnly,
      progressVol: t.library.progressVol,
      progressVolOnly: t.library.progressVolOnly,
      progressJoin: t.library.progressJoin,
      statusDoing: t.library.statusDoing,
      statusDone: t.library.statusDone,
      statusWish: t.library.statusWish,
      statusOnHold: t.library.statusOnHold,
      statusDropped: t.library.statusDropped,
    }),
    [t],
  )

  const resolveWatchProgress = useCallback(
    (item: LibraryItem): WatchProgress | null => {
      return getWatchProgress(item.item_type, item.metadata)
    },
    [],
  )

  const formatItemWatchProgress = useCallback(
    (item: LibraryItem): string | null => {
      const progress = resolveWatchProgress(item)
      if (!progress) return null
      return formatWatchProgressText(progress, watchProgressLabels)
    },
    [resolveWatchProgress, watchProgressLabels],
  )

  const getExtraInfo = useCallback(
    (item: LibraryItem) => {
      if (item.item_type === 'game' && item.metadata.playtime_forever) {
        const hours = Math.round(item.metadata.playtime_forever / 60)
        return t.library.playedHours.replace('{hours}', hours.toString())
      }
      if (item.item_type === 'music') {
        const artists = item.metadata.ar || item.metadata.artists || []
        if (Array.isArray(artists) && artists.length > 0) {
          return artists.map((a: any) => a.name || a).join(', ')
        }
        if (item.metadata.artist) {
          return item.metadata.artist
        }
      }
      if (
        item.item_type === 'video' ||
        item.item_type === 'anime' ||
        item.item_type === 'tv_series' ||
        item.item_type === 'book'
      ) {
        return formatItemWatchProgress(item)
      }
      return null
    },
    [t, formatItemWatchProgress],
  )

  /** Progress row + thin bar for anime/book vertical title plates. */
  const renderWatchProgressPanel = useCallback(
    (
      item: LibraryItem,
      opts?: { dark?: boolean },
    ): React.ReactNode => {
      const progress = resolveWatchProgress(item)
      if (!progress) return null
      const text = formatWatchProgressText(progress, watchProgressLabels)
      const statusLabel = formatWatchStatusLabel(
        progress.status,
        watchProgressLabels,
        { onlyDoing: true },
      )
      const dark = opts?.dark === true
      const barColor =
        item.item_type === 'book'
          ? 'bg-amber-400'
          : item.item_type === 'tv_series'
            ? 'bg-purple-400'
            : item.item_type === 'game'
              ? 'bg-emerald-400'
              : 'bg-pink-400'
      const trackColor = dark ? 'bg-white/25' : 'bg-gray-200/90'
      const textColor = dark ? 'text-white/80' : 'text-gray-600'
      const chipClass = dark
        ? 'bg-white/15 text-white/90'
        : item.item_type === 'book'
          ? 'bg-amber-50 text-amber-700'
          : 'bg-pink-50 text-pink-700'

      return (
        <div className="mt-1.5 min-w-0 space-y-1">
          <div className="flex items-center gap-1.5 min-w-0">
            <span
              className={`text-[10px] leading-tight line-clamp-1 min-w-0 ${textColor}`}
            >
              {text}
            </span>
            {statusLabel && (
              <span
                className={`shrink-0 text-[9px] font-medium leading-none px-1 py-0.5 rounded ${chipClass}`}
              >
                {statusLabel}
              </span>
            )}
          </div>
          {progress.percent != null && (
            <div
              className={`h-[3px] w-full rounded-full overflow-hidden ${trackColor}`}
              aria-hidden
            >
              <div
                className={`h-full rounded-full ${barColor} transition-[width] duration-300`}
                style={{
                  width: `${Math.max(0, Math.min(100, progress.percent))}%`,
                }}
              />
            </div>
          )}
        </div>
      )
    },
    [resolveWatchProgress, watchProgressLabels],
  )

  const handlePlayMusic = useCallback(
    (item: LibraryItem) => {
      const songId = (
        item.metadata.id || item.id.replace('netease_song_', '')
      ).toString()
      const musicState = (window as any).__musicPlayerState
      if (musicState?.currentSong?.id === songId) {
        window.dispatchEvent(new CustomEvent('open-control-panel'))
        showInfo(t.library.alreadyPlaying)
        return
      }

      const isVip =
        item.metadata.isVip ||
        item.metadata.fee === 1 ||
        item.metadata.fee === 4
      if (isVip) {
        showInfo(t.library.vipSongWarning)
      }

      const name = item.metadata.name || item.title
      let artist = t.library.unknownArtist
      const artists = item.metadata.ar || item.metadata.artists || []
      if (Array.isArray(artists) && artists.length > 0) {
        artist = artists.map((a: any) => a.name || a).join(', ')
      } else if (item.metadata.artist) {
        artist = item.metadata.artist
      }

      let album = t.library.unknownAlbum
      let cover = item.cover || ''
      if (item.metadata.al) {
        album = item.metadata.al.name || album
        cover = item.metadata.al.picUrl || cover
      } else if (item.metadata.album) {
        album = item.metadata.album.name || item.metadata.album
        if (item.metadata.album.picUrl) {
          cover = item.metadata.album.picUrl
        }
      }

      const duration = item.metadata.dt
        ? Math.floor(item.metadata.dt / 1000)
        : item.metadata.duration
          ? item.metadata.duration
          : 0

      const song: Song = {
        id: songId.toString(),
        name,
        artist,
        album,
        cover,
        url: `${API_URL}/api/proxy/music/netease/audio/${songId}`,
        duration,
        source: 'netease',
        isVip: false,
      }

      playSongRef.current(song)
      window.dispatchEvent(new CustomEvent('open-control-panel'))
      showInfo(t.library.nowPlaying.replace('{name}', name))
    },
    [t],
  )

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

  if (loading && allItems.length === 0) {
    return null
  }

  return (
    <div className="animate-in fade-in slide-in-from-bottom-4 duration-700 ease-out">
      {error || showEmpty ? (
        <div className="flex flex-col items-start py-8">
          <div className="rounded-2xl bg-white/90 dark:bg-neutral-900/90 backdrop-blur-xl border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10 flex items-center gap-3 px-5 py-3">
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
        <div className="space-y-8">
          <QuickTransition transitioning={isTransitioning}>
            <div
              ref={containerRef}
              className="relative w-full"
              style={{
                height: `${containerHeight}px`,
                minHeight: '400px',
                transition: 'height 0.4s ease-out',
              }}
            >
              {visibleItems.map((item) => {
                const layout = layouts.get(item.id)
                if (!layout) return null

                const platformColor = getPlatformColor(item.platform)
                const isVip =
                  item.metadata.isVip ||
                  item.metadata.fee === 1 ||
                  item.metadata.fee === 4
                const currentSongId = (
                  item.metadata.id || item.id.replace('netease_song_', '')
                ).toString()

                // Context 实时状态，无需额外检查
                const isCurrentSong =
                  currentSong && currentSong.id === currentSongId
                const isPlaying = isCurrentSong && globalIsPlaying

                const rowIndex = Math.floor(layout.top / 300)
                const animationDelay = rowIndex * 0.05

                // Bangumi / MAL 用户评分（0 表示未评分），显示在卡片左上角
                const isBangumi = isBangumiPlatform(item.platform)
                const userRate = hasUserRatingBadge(item.platform)
                  ? Number(
                      item.metadata.rate ??
                        item.metadata?.list_status?.score,
                    ) || 0
                  : 0
                // Bangumi 游戏使用竖版，渲染为封面卡片
                const isBangumiGame =
                  isBangumi && item.item_type === 'game'

                return (
                  <div
                    key={item.id}
                    className="absolute group library-card-container"
                    style={
                      {
                        left: `${layout.left}px`,
                        top: `${layout.top}px`,
                        width: `${layout.width}px`,
                        height: `${layout.height}px`,
                        '--platform-color': platformColor,
                        animationDelay: `${animationDelay}s`,
                      } as React.CSSProperties
                    }
                    // 入场动画播放一次后移除，避免卡片滚出视口再回来时
                    // 浏览器重建绘制层导致 fadeInUp 重播（表现为瞬间透明再恢复）
                    onAnimationEnd={(e) => {
                      if (e.target === e.currentTarget) {
                        ;(e.currentTarget as HTMLElement).style.animation =
                          'none'
                      }
                    }}
                  >
                    {item.item_type === 'music' ? (
                      <div className="relative bg-white rounded-xl shadow-md hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 hover:scale-[1.02] overflow-hidden h-full">
                        <div
                          className="block w-full h-full relative cursor-pointer"
                          onClick={(e) => {
                            e.preventDefault()
                            e.stopPropagation()
                            handlePlayMusic(item)
                          }}
                        >
                          {item.cover ? (
                            <img
                              src={item.cover}
                              alt={item.title}
                              className="w-full h-full object-cover transition-all duration-500 group-hover:scale-110"
                              loading="lazy"
                              decoding="async"
                              onError={(e) => {
                                ;(e.target as HTMLImageElement).src =
                                  `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=400&background=random`
                              }}
                            />
                          ) : (
                            <div className="w-full h-full flex items-center justify-center bg-linear-to-br from-pink-400 to-pink-500">
                              <span className="text-6xl">
                                {getTypeIcon(item.item_type)}
                              </span>
                            </div>
                          )}

                          {isPlaying && (
                            <div
                              className="playing-indicator"
                              style={
                                {
                                  '--music-color': musicColor,
                                  '--platform-color': musicColor,
                                } as React.CSSProperties
                              }
                            >
                              <PlatformIcon
                                platform={item.platform}
                                className="w-8 h-8"
                              />
                            </div>
                          )}

                          <div className="absolute inset-0 bg-linear-to-t from-black/95 via-black/60 to-transparent opacity-0 group-hover:opacity-100 transition-all duration-300 flex flex-col justify-end p-3">
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

                        {!isPlaying && (
                          <div className="absolute top-3 right-3 flex gap-2">
                            <div className="group/platform">
                              <div
                                className="platform-icon-bg"
                                style={
                                  {
                                    '--platform-color': platformColor,
                                  } as React.CSSProperties
                                }
                              >
                                <PlatformIcon
                                  platform={item.platform}
                                  className="w-5 h-5"
                                />
                              </div>
                              <div className="absolute top-full right-0 mt-2 bg-black/90 backdrop-blur-sm text-white text-xs px-2.5 py-1 rounded-md opacity-0 group-hover/platform:opacity-100 transition-opacity duration-200 whitespace-nowrap pointer-events-none">
                                {item.platform}
                              </div>
                            </div>
                          </div>
                        )}
                      </div>
                    ) : item.item_type === 'anime' ||
                      item.item_type === 'tv_series' ||
                      item.item_type === 'book' ||
                      isBangumiGame ? (
                      <div className="relative bg-white rounded-2xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 overflow-hidden h-full">
                        <a
                          href={item.metadata.url || '#'}
                          target={item.metadata.url ? '_blank' : undefined}
                          rel={
                            item.metadata.url
                              ? 'noopener noreferrer'
                              : undefined
                          }
                          className="block w-full h-full relative group"
                        >
                          {item.cover ? (
                            <img
                              src={item.cover}
                              alt={item.title}
                              className="w-full h-full object-cover transition-all duration-500 group-hover:scale-110"
                              loading="lazy"
                              decoding="async"
                              onError={(e) => {
                                ;(e.target as HTMLImageElement).src =
                                  `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=400&background=random`
                              }}
                            />
                          ) : (
                            <div className="w-full h-full flex items-center justify-center bg-linear-to-br from-pink-400 to-purple-500">
                              <span className="text-6xl">
                                <FaVideo />
                              </span>
                            </div>
                          )}

                          <div className="absolute bottom-3 left-3 right-3">
                            <div className="inline-flex items-start max-w-full">
                              <div className="bg-white/95 backdrop-blur-sm rounded-lg px-3 py-2 shadow-lg w-full">
                                <div className="flex items-center gap-2">
                                  <h3 className="font-bold text-gray-900 text-sm line-clamp-1 leading-snug flex-1">
                                    {item.title}
                                  </h3>
                                  <span
                                    className={`inline-block px-2 py-0.5 rounded text-xs font-medium whitespace-nowrap ${
                                      item.item_type === 'anime'
                                        ? 'bg-pink-100 text-pink-700'
                                        : item.item_type === 'book'
                                          ? 'bg-amber-100 text-amber-700'
                                          : item.item_type === 'game'
                                            ? 'bg-emerald-100 text-emerald-700'
                                            : 'bg-purple-100 text-purple-700'
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
                          </div>
                        </a>

                        <div className="absolute top-3 right-3 group/platform">
                          <div className="platform-icon-bg">
                            <PlatformIcon
                              platform={item.platform}
                              className="w-5 h-5"
                            />
                          </div>
                          <div className="absolute top-full right-0 mt-2 bg-black/90 backdrop-blur-sm text-white text-xs px-2.5 py-1 rounded-md opacity-0 group-hover/platform:opacity-100 transition-opacity duration-200 whitespace-nowrap pointer-events-none">
                            {item.platform}
                          </div>
                        </div>
                      </div>
                    ) : item.item_type === 'video' ? (
                      <div className="relative bg-white rounded-2xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 overflow-hidden h-full">
                        <a
                          href={item.metadata.url || '#'}
                          target={item.metadata.url ? '_blank' : undefined}
                          rel={
                            item.metadata.url
                              ? 'noopener noreferrer'
                              : undefined
                          }
                          className="block w-full h-full relative"
                        >
                          {item.cover ? (
                            <img
                              src={item.cover}
                              alt={item.title}
                              className="w-full h-full object-cover transition-all duration-500 group-hover:scale-110"
                              loading="lazy"
                              decoding="async"
                              onError={(e) => {
                                ;(e.target as HTMLImageElement).src =
                                  `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=400&background=random`
                              }}
                            />
                          ) : (
                            <div className="w-full h-full flex items-center justify-center bg-linear-to-br from-blue-400 to-blue-500">
                              <span className="text-6xl">
                                {getTypeIcon(item.item_type)}
                              </span>
                            </div>
                          )}

                          <div className="absolute bottom-3 left-3 right-3">
                            <div className="inline-flex items-start max-w-full">
                              <div className="bg-white/95 backdrop-blur-sm rounded-lg px-3 py-2 shadow-lg">
                                <h3 className="font-bold text-gray-900 text-sm line-clamp-2 leading-snug">
                                  {item.title}
                                </h3>
                                {renderWatchProgressPanel(item) ??
                                  (getExtraInfo(item) && (
                                    <p className="text-xs text-gray-600 mt-1">
                                      {getExtraInfo(item)}
                                    </p>
                                  ))}
                              </div>
                            </div>
                          </div>
                        </a>

                        <div className="absolute top-3 right-3 group/platform">
                          <div className="platform-icon-bg">
                            <PlatformIcon
                              platform={item.platform}
                              className="w-5 h-5"
                            />
                          </div>
                          <div className="absolute top-full right-0 mt-2 bg-black/90 backdrop-blur-sm text-white text-xs px-2.5 py-1 rounded-md opacity-0 group-hover/platform:opacity-100 transition-opacity duration-200 whitespace-nowrap pointer-events-none">
                            {item.platform}
                          </div>
                        </div>
                      </div>
                    ) : (
                      <div className="bg-white rounded-2xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 overflow-hidden h-full">
                        <div className="relative overflow-hidden h-full bg-linear-to-br from-gray-900 to-gray-800">
                          <a
                            href={
                              item.platform.toLowerCase() === 'steam' &&
                              item.metadata.appid
                                ? `https://store.steampowered.com/app/${item.metadata.appid}`
                                : item.metadata.url || '#'
                            }
                            target="_blank"
                            rel="noopener noreferrer"
                            className="block w-full h-full relative"
                          >
                            {item.cover ? (
                              <img
                                src={item.cover}
                                alt={item.title}
                                className="w-full h-full object-cover transition-all duration-500 group-hover:scale-110"
                                loading="lazy"
                                decoding="async"
                                onError={(e) => {
                                  ;(e.target as HTMLImageElement).src =
                                    `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=400&background=random`
                                }}
                              />
                            ) : (
                              <div className="w-full h-full flex items-center justify-center bg-linear-to-br from-purple-400 to-pink-500">
                                <span className="text-6xl">
                                  {getTypeIcon(item.item_type)}
                                </span>
                              </div>
                            )}
                          </a>

                          <div className="absolute inset-0 bg-linear-to-t from-black/90 via-black/40 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-300 flex flex-col justify-end p-4 pointer-events-none">
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

                          <div className="absolute top-3 right-3 group/platform z-10">
                            <div className="platform-icon-bg">
                              <PlatformIcon
                                platform={item.platform}
                                className="w-5 h-5"
                              />
                            </div>
                            <div className="absolute top-full right-0 mt-2 bg-black/90 backdrop-blur-sm text-white text-xs px-2.5 py-1 rounded-md opacity-0 group-hover/platform:opacity-100 transition-opacity duration-200 whitespace-nowrap pointer-events-none">
                              {item.platform}
                            </div>
                          </div>
                        </div>
                      </div>
                    )}

                    {/* Bangumi / MAL 用户评分徽章 - 左上角，分色分级（分数越高越醒目） */}
                    {userRate > 0 &&
                      (() => {
                        const rs = getRatingBadgeStyle(userRate)
                        const animClass = rs.gloss
                          ? userRate >= 10
                            ? 'rating-badge-anim-max'
                            : 'rating-badge-anim'
                          : ''
                        return (
                          <div
                            className={`absolute top-2.5 left-2.5 z-20 flex items-center justify-center overflow-hidden rounded-lg font-extrabold leading-none shadow-lg pointer-events-none ${rs.box} ${animClass}`}
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
                      })()}
                  </div>
                )
              })}
            </div>
          </QuickTransition>

          {hasMore && (
            <div
              ref={observerTarget}
              className="flex justify-center mt-8 mb-4 py-4 w-full"
            >
              <div className="flex items-center gap-2 text-gray-600 dark:text-gray-400">
                <Spinner size="xs" color="primary" />
                <span className="text-sm">{t.library.loadingMore}</span>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
