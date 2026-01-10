import type { Song } from '../utils/musicPlayer'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { API_URL } from '../config'
import { useI18n } from '../contexts/I18nContext'
import { useMusicPlayerControl } from '../contexts/MusicPlayerContext'
import { useNotification } from '../contexts/NotificationContext'
import { useLibraryIntersectionObserver } from '../hooks/animation/pages/library'
import { useSharedResize } from '../hooks/useSharedEventListener'
import { getLibraryDataDeduped } from '../utils/requestDedup'
import PlatformIcon from './PlatformIcon'
import { QuickTransition } from './SkeletonTransition'
import { Spinner } from './Spinner'

// 添加样式到页面
if (typeof document !== 'undefined' && !document.getElementById('library-grid-styles')) {
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
        }
        
        /* 平台图标背景 */
        .platform-icon-bg {
            width: 2.5rem;
            height: 2.5rem;
            border-radius: 9999px;
            backdrop-filter: blur(12px);
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
        
        @keyframes spin {
            to { transform: rotate(360deg); }
        }
        
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
    `
  document.head.appendChild(style)
}

interface LibraryItem {
  id: string
  item_type: 'game' | 'video' | 'music' | 'anime' | 'tv_series'
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
  filter: 'all' | 'game' | 'video' | 'music' | 'anime' | 'tv_series'
}

// 获取项目在网格中的尺寸 (w, h)
function getItemGridSize(type: string) {
  switch (type) {
    case 'video':
    case 'game':
      return { w: 2, h: 1 }
    case 'anime':
    case 'tv_series':
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
  const [prevFilter, setPrevFilter] = useState<'all' | 'game' | 'video' | 'music' | 'anime' | 'tv_series'>('all')
  const [isTransitioning, setIsTransitioning] = useState(false)

  // 布局状态
  const [layouts, setLayouts] = useState<Map<string, CardLayout>>(new Map())
  const [visibleCount, setVisibleCount] = useState(20) // 初始显示数量

  const containerRef = useRef<HTMLDivElement>(null)
  const containerWidthRef = useRef<number>(0) // 🔧 缓存容器宽度，避免重复读取
  const { showInfo } = useNotification()
  const { playSong, currentSong, isPlaying: globalIsPlaying, musicColor } = useMusicPlayerControl()
  const { t } = useI18n()

  // 使用 ref 存储回调函数，避免在依赖中频繁更新
  const showInfoRef = useRef(showInfo)
  const playSongRef = useRef(playSong)
  showInfoRef.current = showInfo
  playSongRef.current = playSong

  // 筛选后的所有项目
  const filteredAllItems = useMemo(() => {
    return filter === 'all'
      ? allItems
      : allItems.filter(item => item.item_type === filter)
  }, [filter, allItems])

  // 核心布局算法：完全避免空隙
  const computeLayout = useCallback(() => {
    if (!containerRef.current || filteredAllItems.length === 0)
      return

    // 🔧 使用缓存的容器宽度，避免强制重排
    // 只有缓存无效时才读取
    if (containerWidthRef.current === 0) {
      containerWidthRef.current = containerRef.current.offsetWidth
    }
    const containerWidth = containerWidthRef.current
    const gap = 16
    let columns = 5

    // 响应式列数
    if (containerWidth < 640)
      columns = 2
    else if (containerWidth < 768)
      columns = 3
    else if (containerWidth < 1024)
      columns = 4
    else if (containerWidth < 1536)
      columns = 5
    else columns = 6

    const baseWidth = (containerWidth - gap * (columns + 1)) / columns
    const uniformHeight = baseWidth

    // 居中逻辑修正：针对纯宽卡片（2x1）在奇数列数下的居中处理
    let startOffset = 0
    let layoutColumns = columns

    // 如果是游戏或视频分类（只有宽2的卡片），且列数是奇数
    // 那么最后一列无法被填满（因为没有宽1的卡片），导致整体偏左
    // 需要计算偏移量使内容居中
    if ((filter === 'game' || filter === 'video') && columns % 2 !== 0 && columns > 1) {
      layoutColumns = columns - 1
      // 剩余空间 = 1个列宽 + 1个间隙
      // 偏移量 = 剩余空间 / 2
      startOffset = (baseWidth + gap) / 2
    }

    // 1. 准备队列：按尺寸分类，保持原始相对顺序
    const queues: Record<string, { item: LibraryItem, originalIndex: number }[]> = {
      '1x1': [],
      '1x2': [],
      '2x1': [],
      // '2x2': [] // 暂无2x2类型
    }

    filteredAllItems.forEach((item, index) => {
      const size = getItemGridSize(item.item_type)
      const key = `${size.w}x${size.h}`
      if (queues[key]) {
        queues[key].push({ item, originalIndex: index })
      }
      else {
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
      const rowHasEmpty = false

      for (let x = 0; x < layoutColumns; x++) {
        if (isOccupied(x, y))
          continue

        // 发现空位 (x, y)
        // 尝试寻找最佳匹配项
        // 优先级：
        // 1. 检查是否能放入 2x1 (需要 x+1 空闲)
        // 2. 检查是否能放入 1x2 (需要 y+1 空闲 - 总是假设 y+1 空闲，除非有预占，但这里我们是逐行扫描，y+1通常未处理)
        //    注意：如果之前有 1x2 占据了 (x, y+1)，则 isOccupied(x, y+1) 会为 true。
        // 3. 放入 1x1

        // 为了保持"平均开始排布"，我们在所有能放入的候选中，选择 originalIndex 最小的那个

        const candidates: { type: string, index: number, item: LibraryItem, w: number, h: number }[] = []

        // 检查 1x1
        if (queues['1x1'].length > 0) {
          const qItem = queues['1x1'][0]
          candidates.push({ item: qItem.item, index: qItem.originalIndex, type: '1x1', w: 1, h: 1 })
        }

        // 检查 2x1
        const canFit2x1 = x + 1 < layoutColumns && !isOccupied(x + 1, y)
        if (canFit2x1 && queues['2x1'].length > 0) {
          const qItem = queues['2x1'][0]
          candidates.push({ item: qItem.item, index: qItem.originalIndex, type: '2x1', w: 2, h: 1 })
        }

        // 检查 1x2
        // 垂直方向通常是无限的，但要检查是否被上方的某些长条物体阻挡？
        // 我们是按 y 递增扫描，所以 (x, y+1) 只有可能被之前的操作占据（不太可能，除非有复杂形状）
        // 但为了严谨，检查一下
        const canFit1x2 = !isOccupied(x, y + 1)
        if (canFit1x2 && queues['1x2'].length > 0) {
          const qItem = queues['1x2'][0]
          candidates.push({ item: qItem.item, index: qItem.originalIndex, type: '1x2', w: 1, h: 2 })
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
        if (itemBottom > maxY)
          maxY = itemBottom
      }

      // 检查当前行是否还有未处理的空位（被跳过的）
      // 如果所有列都处理过（占用或尝试过），进入下一行
      y++

      // 安全阀：防止死循环 (如果数据异常)
      if (y > totalItems * 2)
        break
    }

    setLayouts(newLayouts)
  }, [filteredAllItems, filter])

  // 使用共享的 resize 监听器
  useSharedResize(() => {
    // 🔧 resize 时刷新容器宽度缓存
    if (containerRef.current) {
      containerWidthRef.current = containerRef.current.offsetWidth
    }
    computeLayout()
  }, { debounce: 150 })

  // 初始计算布局
  useEffect(() => {
    computeLayout()
  }, [computeLayout])

  // 滚动加载更多 - 🔧 添加节流防止过快触发
  const loadMoreRef = useRef<number | null>(null)
  const loadMore = useCallback(() => {
    if (loadMoreRef.current)
      return // 防止重复触发
    loadMoreRef.current = requestAnimationFrame(() => {
      setVisibleCount(prev => Math.min(prev + 20, filteredAllItems.length))
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
  const { observeLibraryIntersection, unobserveLibraryIntersection } = useLibraryIntersectionObserver()
  const observerTarget = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const target = observerTarget.current
    if (!target)
      return

    observeLibraryIntersection(target, (entry) => {
      if (entry.isIntersecting && hasMore) {
        loadMore()
      }
    })

    return () => unobserveLibraryIntersection(target)
  }, [hasMore, loadMore, observeLibraryIntersection, unobserveLibraryIntersection])

  // 排序后的可见项目
  const visibleItems = useMemo(() => {
    if (layouts.size === 0)
      return []

    // 获取所有已布局的项目
    const laidOutItems = filteredAllItems.filter(item => layouts.has(item.id))

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
    if (visibleItems.length === 0)
      return 400
    let maxBottom = 0
    visibleItems.forEach((item) => {
      const layout = layouts.get(item.id)
      if (layout) {
        const bottom = layout.top + layout.height
        if (bottom > maxBottom)
          maxBottom = bottom
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
      }
      else {
        throw new Error('No library data available')
      }
    }
    catch (err) {
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
    )

    for (let i = 0; i < maxLength; i++) {
      const typeOrder = ['game', 'video', 'music', 'anime', 'tv_series'].sort(() => Math.random() - 0.5)
      typeOrder.forEach((type) => {
        if (groups[type][i]) {
          result.push(groups[type][i])
        }
      })
    }

    return result
  }

  // 显示错误通知 - 使用 ref 避免依赖变化
  useEffect(() => {
    if (error) {
      showInfoRef.current(t.library.emptyLibrary)
    }
  }, [error, t])

  // 显示空状态通知 - 使用 ref 避免依赖变化
  useEffect(() => {
    if (!loading && filteredAllItems.length === 0 && !error) {
      showInfoRef.current(t.library.emptyCategory)
    }
  }, [loading, filteredAllItems.length, error, t])

  const getPlatformColor = useCallback((platform: string) => {
    switch (platform.toLowerCase()) {
      case 'bilibili': return '#00A1D6'
      case 'steam': return '#171a21'
      case 'netease music':
      case 'netease': return '#d33a31'
      case 'github': return '#24292e'
      default: return '#6b7280'
    }
  }, [])

  const getTypeIcon = useCallback((type: string) => {
    switch (type) {
      case 'game': return '🎮'
      case 'video': return '🎬'
      case 'music': return '🎵'
      default: return '📦'
    }
  }, [])

  const getExtraInfo = useCallback((item: LibraryItem) => {
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
    if ((item.item_type === 'video' || item.item_type === 'anime' || item.item_type === 'tv_series') && item.metadata.progress) {
      return item.metadata.progress
    }
    return null
  }, [t])

  const handlePlayMusic = useCallback((item: LibraryItem) => {
    const songId = (item.metadata.id || item.id.replace('netease_song_', '')).toString()
    const musicState = (window as any).__musicPlayerState
    if (musicState?.currentSong?.id === songId) {
      window.dispatchEvent(new CustomEvent('open-control-panel'))
      showInfoRef.current(t.library.alreadyPlaying)
      return
    }

    const isVip = item.metadata.isVip || item.metadata.fee === 1 || item.metadata.fee === 4
    if (isVip) {
      showInfoRef.current(t.library.vipSongWarning)
    }

    const name = item.metadata.name || item.title
    let artist = t.library.unknownArtist
    const artists = item.metadata.ar || item.metadata.artists || []
    if (Array.isArray(artists) && artists.length > 0) {
      artist = artists.map((a: any) => a.name || a).join(', ')
    }
    else if (item.metadata.artist) {
      artist = item.metadata.artist
    }

    let album = t.library.unknownAlbum
    let cover = item.cover || ''
    if (item.metadata.al) {
      album = item.metadata.al.name || album
      cover = item.metadata.al.picUrl || cover
    }
    else if (item.metadata.album) {
      album = item.metadata.album.name || item.metadata.album
      if (item.metadata.album.picUrl) {
        cover = item.metadata.album.picUrl
      }
    }

    const duration = item.metadata.dt
      ? Math.floor(item.metadata.dt / 1000)
      : item.metadata.duration ? item.metadata.duration : 0

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
    showInfoRef.current(t.library.nowPlaying.replace('{name}', name))
  }, [t])

  const needsTransition = (from: string, to: string) => {
    return from !== 'all' && to !== 'all' && from !== to
  }

  useEffect(() => {
    if (filter === prevFilter)
      return
    if (needsTransition(prevFilter, filter)) {
      setIsTransitioning(true)
      setTimeout(() => {
        setPrevFilter(filter)
        setTimeout(() => setIsTransitioning(false), 150)
      }, 200)
    }
    else {
      setPrevFilter(filter)
    }
    // 切换分类时重置显示数量
    setVisibleCount(20)
  }, [filter, prevFilter])

  if (loading && allItems.length === 0) {
    return null
  }

  return (
    <div className="animate-in fade-in slide-in-from-bottom-4 duration-700 ease-out">
      {error ? (
        <div className="flex items-center justify-center min-h-[400px]"></div>
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
                if (!layout)
                  return null

                const platformColor = getPlatformColor(item.platform)
                const isVip = (item.metadata.isVip || item.metadata.fee === 1 || item.metadata.fee === 4)
                const currentSongId = (item.metadata.id || item.id.replace('netease_song_', '')).toString()

                // Context 实时状态，无需额外检查
                const isCurrentSong = currentSong && currentSong.id === currentSongId
                const isPlaying = isCurrentSong && globalIsPlaying

                const rowIndex = Math.floor(layout.top / 300)
                const animationDelay = rowIndex * 0.05

                return (
                  <div
                    key={item.id}
                    className="absolute group library-card-container"
                    style={{
                      'left': `${layout.left}px`,
                      'top': `${layout.top}px`,
                      'width': `${layout.width}px`,
                      'height': `${layout.height}px`,
                      '--platform-color': platformColor,
                      'animationDelay': `${animationDelay}s`,
                    } as React.CSSProperties}
                  >
                    {item.item_type === 'music'
                      ? (
                          <div className="relative bg-white rounded-xl shadow-md hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 hover:scale-[1.02] overflow-hidden h-full">
                            <div
                              className="block w-full h-full relative cursor-pointer"
                              onClick={(e) => {
                                e.preventDefault()
                                e.stopPropagation()
                                handlePlayMusic(item)
                              }}
                            >
                              {item.cover
                                ? (
                                    <img
                                      src={item.cover}
                                      alt={item.title}
                                      className="w-full h-full object-cover transition-all duration-500 group-hover:scale-110"
                                      loading="lazy"
                                      decoding="async"
                                      onError={(e) => {
                                        (e.target as HTMLImageElement).src = `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=400&background=random`
                                      }}
                                    />
                                  )
                                : (
                                    <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-pink-400 to-pink-500">
                                      <span className="text-6xl">{getTypeIcon(item.item_type)}</span>
                                    </div>
                                  )}

                              {isPlaying && (
                                <div
                                  className="playing-indicator"
                                  style={{
                                    '--music-color': musicColor,
                                    '--platform-color': musicColor,
                                  } as React.CSSProperties}
                                >
                                  <PlatformIcon platform={item.platform} className="w-8 h-8" />
                                </div>
                              )}

                              <div className="absolute inset-0 bg-gradient-to-t from-black/95 via-black/60 to-transparent opacity-0 group-hover:opacity-100 transition-all duration-300 flex flex-col justify-end p-3">
                                <div>
                                  <div className="flex items-start gap-1">
                                    <h3 className="font-bold text-white text-xs leading-tight line-clamp-2 mb-1 flex-1">
                                      {item.title}
                                    </h3>
                                    {isVip && (
                                      <span className="inline-flex items-center px-1.5 py-0.5 rounded-md bg-gradient-to-r from-yellow-500 to-amber-600 text-[10px] font-semibold text-white shadow-md select-none">
                                        VIP
                                      </span>
                                    )}
                                  </div>
                                  {getExtraInfo(item) && (
                                    <p className="text-[10px] text-white/75 line-clamp-1">
                                      {getExtraInfo(item)}
                                    </p>
                                  )}
                                </div>
                              </div>
                            </div>

                            {!isPlaying && (
                              <div className="absolute top-3 right-3 flex gap-2">
                                <div className="group/platform">
                                  <div
                                    className="platform-icon-bg"
                                    style={{
                                      '--platform-color': platformColor,
                                    } as React.CSSProperties}
                                  >
                                    <PlatformIcon platform={item.platform} className="w-5 h-5" />
                                  </div>
                                  <div className="absolute top-full right-0 mt-2 bg-black/90 backdrop-blur-sm text-white text-xs px-2.5 py-1 rounded-md opacity-0 group-hover/platform:opacity-100 transition-opacity duration-200 whitespace-nowrap pointer-events-none">
                                    {item.platform}
                                  </div>
                                </div>
                              </div>
                            )}
                          </div>
                        )
                      : item.item_type === 'anime' || item.item_type === 'tv_series'
                        ? (
                            <div className="relative bg-white rounded-2xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 overflow-hidden h-full">
                              <a
                                href={item.metadata.url || '#'}
                                target={item.metadata.url ? '_blank' : undefined}
                                rel={item.metadata.url ? 'noopener noreferrer' : undefined}
                                className="block w-full h-full relative group"
                              >
                                {item.cover
                                  ? (
                                      <img
                                        src={item.cover}
                                        alt={item.title}
                                        className="w-full h-full object-cover transition-all duration-500 group-hover:scale-110"
                                        loading="lazy"
                                        decoding="async"
                                        onError={(e) => {
                                          (e.target as HTMLImageElement).src = `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=400&background=random`
                                        }}
                                      />
                                    )
                                  : (
                                      <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-pink-400 to-purple-500">
                                        <span className="text-6xl">{item.item_type === 'anime' ? '📺' : '🎬'}</span>
                                      </div>
                                    )}

                                <div className="absolute bottom-3 left-3 right-3">
                                  <div className="inline-flex items-start max-w-full">
                                    <div className="bg-white/95 backdrop-blur-sm rounded-lg px-3 py-2 shadow-lg w-full">
                                      <div className="flex items-center gap-2">
                                        <h3 className="font-bold text-gray-900 text-sm line-clamp-1 leading-snug flex-1">
                                          {item.title}
                                        </h3>
                                        <span className={`inline-block px-2 py-0.5 rounded text-xs font-medium whitespace-nowrap ${
                                          item.item_type === 'anime'
                                            ? 'bg-pink-100 text-pink-700'
                                            : 'bg-purple-100 text-purple-700'
                                        }`}
                                        >
                                          {item.item_type === 'anime' ? t.library.anime : t.library.tvSeries}
                                        </span>
                                      </div>
                                      {getExtraInfo(item) && (
                                        <p className="text-xs text-gray-600 line-clamp-1">
                                          {getExtraInfo(item)}
                                        </p>
                                      )}
                                    </div>
                                  </div>
                                </div>
                              </a>

                              <div className="absolute top-3 right-3 group/platform">
                                <div className="platform-icon-bg">
                                  <PlatformIcon platform={item.platform} className="w-5 h-5" />
                                </div>
                                <div className="absolute top-full right-0 mt-2 bg-black/90 backdrop-blur-sm text-white text-xs px-2.5 py-1 rounded-md opacity-0 group-hover/platform:opacity-100 transition-opacity duration-200 whitespace-nowrap pointer-events-none">
                                  {item.platform}
                                </div>
                              </div>
                            </div>
                          )
                        : item.item_type === 'video'
                          ? (
                              <div className="relative bg-white rounded-2xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 overflow-hidden h-full">
                                <a
                                  href={item.metadata.url || '#'}
                                  target={item.metadata.url ? '_blank' : undefined}
                                  rel={item.metadata.url ? 'noopener noreferrer' : undefined}
                                  className="block w-full h-full relative"
                                >
                                  {item.cover
                                    ? (
                                        <img
                                          src={item.cover}
                                          alt={item.title}
                                          className="w-full h-full object-cover transition-all duration-500 group-hover:scale-110"
                                          loading="lazy"
                                          decoding="async"
                                          onError={(e) => {
                                            (e.target as HTMLImageElement).src = `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=400&background=random`
                                          }}
                                        />
                                      )
                                    : (
                                        <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-blue-400 to-blue-500">
                                          <span className="text-6xl">{getTypeIcon(item.item_type)}</span>
                                        </div>
                                      )}

                                  <div className="absolute bottom-3 left-3 right-3">
                                    <div className="inline-flex items-start max-w-full">
                                      <div className="bg-white/95 backdrop-blur-sm rounded-lg px-3 py-2 shadow-lg">
                                        <h3 className="font-bold text-gray-900 text-sm line-clamp-2 leading-snug">
                                          {item.title}
                                        </h3>
                                        {getExtraInfo(item) && (
                                          <p className="text-xs text-gray-600 mt-1">
                                            {getExtraInfo(item)}
                                          </p>
                                        )}
                                      </div>
                                    </div>
                                  </div>
                                </a>

                                <div className="absolute top-3 right-3 group/platform">
                                  <div className="platform-icon-bg">
                                    <PlatformIcon platform={item.platform} className="w-5 h-5" />
                                  </div>
                                  <div className="absolute top-full right-0 mt-2 bg-black/90 backdrop-blur-sm text-white text-xs px-2.5 py-1 rounded-md opacity-0 group-hover/platform:opacity-100 transition-opacity duration-200 whitespace-nowrap pointer-events-none">
                                    {item.platform}
                                  </div>
                                </div>
                              </div>
                            )
                          : (
                              <div className="bg-white rounded-2xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 overflow-hidden h-full">
                                <div className="relative overflow-hidden h-full bg-gradient-to-br from-gray-900 to-gray-800">
                                  <a
                                    href={item.platform.toLowerCase() === 'steam' && item.metadata.appid
                                      ? `https://store.steampowered.com/app/${item.metadata.appid}`
                                      : (item.metadata.url || '#')}
                                    target="_blank"
                                    rel="noopener noreferrer"
                                    className="block w-full h-full relative"
                                  >
                                    {item.cover
                                      ? (
                                          <img
                                            src={item.cover}
                                            alt={item.title}
                                            className="w-full h-full object-cover transition-all duration-500 group-hover:scale-110"
                                            loading="lazy"
                                            decoding="async"
                                            onError={(e) => {
                                              (e.target as HTMLImageElement).src = `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=400&background=random`
                                            }}
                                          />
                                        )
                                      : (
                                          <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-purple-400 to-pink-500">
                                            <span className="text-6xl">{getTypeIcon(item.item_type)}</span>
                                          </div>
                                        )}
                                  </a>

                                  <div className="absolute inset-0 bg-gradient-to-t from-black/90 via-black/40 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-300 flex flex-col justify-end p-4 pointer-events-none">
                                    <h3 className="font-bold text-white text-base line-clamp-2 leading-snug mb-1">
                                      {item.title}
                                    </h3>
                                    {getExtraInfo(item) && (
                                      <p className="text-sm text-white/80">
                                        {getExtraInfo(item)}
                                      </p>
                                    )}
                                  </div>

                                  <div className="absolute top-3 right-3 group/platform z-10">
                                    <div className="platform-icon-bg">
                                      <PlatformIcon platform={item.platform} className="w-5 h-5" />
                                    </div>
                                    <div className="absolute top-full right-0 mt-2 bg-black/90 backdrop-blur-sm text-white text-xs px-2.5 py-1 rounded-md opacity-0 group-hover/platform:opacity-100 transition-opacity duration-200 whitespace-nowrap pointer-events-none">
                                      {item.platform}
                                    </div>
                                  </div>
                                </div>
                              </div>
                            )}
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
                <Spinner size="sm" variant="primary" />
                <span className="text-sm">{t.library.loadingMore}</span>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
