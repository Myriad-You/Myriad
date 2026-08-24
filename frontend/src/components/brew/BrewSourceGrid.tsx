/**
 * Brew 订阅源网格组件
 * 设计参考 TappCard / UnifiedAppCard 风格
 *
 * 功能整合：
 * - 订阅源卡片展示
 * - 内置管理工具（搜索、批量选择、编辑、删除）
 * - 浮动操作栏
 *
 * 性能优化：
 * - 接入统一动画调度器 (useBrewCardStagger)
 * - React.memo + 自定义比较函数避免不必要的重渲染
 * - useMemo/useCallback 缓存计算结果和回调
 * - 根据动画级别自动降级（禁用/简化动画）
 * - 图片懒加载
 */

import type { AddSourceInput, BrewSource, CardSize } from '../../types/brew'

import type { SortMode } from './manager/ControlIsland'
import { LuRss as Rss, LuSearch as Search } from '@lib/icons'
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { useI18n } from '../../contexts/I18nContext'
import * as brewApi from '../../services/brewApi'
// 磁贴墙（localStorage flag 后面走这条路；旧网格在 PR 9 清退）
import BrewTileWall from './BrewTileWall'
// 卡片组件
import { SourceCard } from './cards'
// 共享常量
import { brewMainCategory, PRESET_CATEGORY_DB_VALUES } from './constants'
import { TOPIC_LARGE_COUNT_SMART } from './logic/layout'
import { compareByScore, roleFromAuth } from './logic/score'
import { clusterTopics, previewsToTopicItems } from './logic/topics'
import ControlIsland from './manager/ControlIsland'
// 管理组件
import EditModal from './manager/EditModal'
import { isBrewTileGridEnabled } from './tileGridFlag'

interface BrewSourceGridProps {
  sources: BrewSource[]
  category?: string // 分类筛选
  onSourceClick: (source: BrewSource) => void
  onRefreshSource: (sourceId: number) => void
  onSourceUpdate?: (source: BrewSource) => void
  onSourcesChange?: () => void
  onAddSource?: (input: AddSourceInput) => Promise<void>
  /** 点主题卡：进入跨源列表（viewMode: 'topic-feed'） */
  onTopicClick?: (topicKey: string, topicNameKey: string) => void
  isAuthenticated?: boolean // 是否已登录（用于已读状态等普通用户功能）
  isAdmin?: boolean // 是否是管理员（用于添加、编辑、删除等管理功能）
}

export default function BrewSourceGrid({
  sources,
  category,
  onSourceClick,
  onRefreshSource,
  onSourceUpdate,
  onSourcesChange,
  onAddSource,
  onTopicClick,
  isAuthenticated = false, // 默认游客模式（用于已读状态）
  isAdmin = false, // 默认非管理员（用于管理功能）
}: BrewSourceGridProps) {
  const { t } = useI18n()
  // 磁贴墙开关。一次读定：中途切 flag 需要刷新，避免两套布局在同一会话里混用。
  const [tileGrid] = useState(isBrewTileGridEnabled)
  // 智能模式插几张主题卡 —— 与「前 N 张走 4x4」是同一个 N
  const SMART_TOPIC_CARDS = TOPIC_LARGE_COUNT_SMART
  const viewerRole = roleFromAuth(isAuthenticated, isAdmin)
  // 管理功能状态（仅登录用户可用）
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set())
  const [isEditMode, setIsEditMode] = useState(false)
  const [editingSource, setEditingSource] = useState<BrewSource | null>(null)
  const [deletingIds, setDeletingIds] = useState<number[]>([])
  const [isDeleting, setIsDeleting] = useState(false)
  const [isRefreshing, setIsRefreshing] = useState(false)

  // 排序状态
  // 默认智能排序：打开 /brew 第一眼要能判断「现在有什么可看的」，
  // 而不是先扫一遍站名。旧的 custom / update 等模式依据一行未改。
  const [sortMode, setSortMode] = useState<SortMode>(
    tileGrid ? 'smart' : 'custom',
  )
  const [customOrder, setCustomOrder] = useState<number[]>([]) // 自由排序的顺序
  const [randomSeed, setRandomSeed] = useState(Date.now()) // 随机排序种子

  // 拖拽排序状态（仅登录用户可用）
  const [draggingSourceId, setDraggingSourceId] = useState<number | null>(null)
  const [dragOverSourceId, setDragOverSourceId] = useState<number | null>(null)
  const cardRefsRef = useRef<Map<number, HTMLDivElement>>(new Map())
  const dragStartPosRef = useRef<{ x: number; y: number } | null>(null)

  // FLIP 动画：存储卡片位置快照
  const cardRectsRef = useRef<Map<number, DOMRect>>(new Map())
  const flipAnimationsRef = useRef<Map<number, Animation>>(new Map())

  // 记录所有卡片的当前位置（在排序变化前调用）
  const recordCardPositions = useCallback(() => {
    cardRectsRef.current.clear()
    cardRefsRef.current.forEach((el, id) => {
      cardRectsRef.current.set(id, el.getBoundingClientRect())
    })
  }, [])

  // 播放 FLIP 动画（在排序变化后调用）- 支持位置和尺寸变化
  const playFlipAnimations = useCallback(() => {
    // 取消所有正在进行的动画
    flipAnimationsRef.current.forEach((anim) => anim.cancel())
    flipAnimationsRef.current.clear()

    // 用 requestAnimationFrame 确保 DOM 已更新
    requestAnimationFrame(() => {
      cardRefsRef.current.forEach((el, id) => {
        const firstRect = cardRectsRef.current.get(id)
        if (!firstRect) return

        const lastRect = el.getBoundingClientRect()

        // 计算位移差
        const deltaX = firstRect.left - lastRect.left
        const deltaY = firstRect.top - lastRect.top

        // 计算尺寸比例差（FLIP 的核心：用 scale 模拟尺寸变化）
        const scaleX = firstRect.width / lastRect.width
        const scaleY = firstRect.height / lastRect.height

        // 检查是否有位置或尺寸变化
        const hasPositionChange = Math.abs(deltaX) > 1 || Math.abs(deltaY) > 1
        const hasSizeChange =
          Math.abs(scaleX - 1) > 0.01 || Math.abs(scaleY - 1) > 0.01

        if (!hasPositionChange && !hasSizeChange) {
          return
        }

        // 使用 Web Animations API 执行流畅动画
        // transformOrigin 设为 top left 确保缩放从正确的位置开始
        const animation = el.animate(
          [
            {
              transform: `translate(${deltaX}px, ${deltaY}px) scale(${scaleX}, ${scaleY})`,
              transformOrigin: 'top left',
            },
            {
              transform: 'translate(0, 0) scale(1, 1)',
              transformOrigin: 'top left',
            },
          ],
          {
            duration: 650,
            easing: 'cubic-bezier(0.16, 1, 0.3, 1)', // 更优雅的缓动曲线
            fill: 'none',
          },
        )

        flipAnimationsRef.current.set(id, animation)

        animation.onfinish = () => {
          flipAnimationsRef.current.delete(id)
        }
      })
    })
  }, [])

  // 获取所有分类（用于编辑弹窗）- 支持多分类
  const categories = useMemo(() => {
    const cats = new Set<string>()
    sources.forEach((s) => {
      if (s.category) {
        // 解析逗号分隔的多分类
        s.category.split(',').forEach((c) => {
          const trimmed = c.trim()
          if (trimmed) cats.add(trimmed)
        })
      }
    })
    return Array.from(cats)
  }, [sources])

  // 根据分类和搜索筛选源
  const filteredSources = useMemo(() => {
    let result = sources
    if (category) {
      // 支持多分类：检查 category 字段是否包含目标分类（用逗号分隔）
      result = result.filter((s) => {
        if (!s.category) return false
        const cats = s.category.split(',').map((c) => c.trim())
        return cats.includes(category)
      })
    }
    if (searchQuery.trim()) {
      const query = searchQuery.toLowerCase()
      result = result.filter(
        (s) =>
          s.name.toLowerCase().includes(query) ||
          s.url.toLowerCase().includes(query) ||
          s.description?.toLowerCase().includes(query),
      )
    }
    return result
  }, [sources, category, searchQuery])

  // 初始化自定义排序顺序（从数据库加载或默认）
  useEffect(() => {
    if (customOrder.length === 0 && filteredSources.length > 0) {
      // 检查是否有已保存的 sort_order
      const sourcesWithOrder = filteredSources.filter(
        (s) => s.sort_order !== null && s.sort_order !== undefined,
      )

      if (sourcesWithOrder.length > 0) {
        // 按 sort_order 排序，然后提取 id 作为 customOrder
        const sortedByOrder = [...filteredSources].sort((a, b) => {
          const orderA = a.sort_order ?? Number.MAX_SAFE_INTEGER
          const orderB = b.sort_order ?? Number.MAX_SAFE_INTEGER
          return orderA - orderB
        })
        setCustomOrder(sortedByOrder.map((s) => s.id))
      } else {
        // 没有保存的排序，使用默认顺序
        setCustomOrder(filteredSources.map((s) => s.id))
      }
    }
  }, [filteredSources, customOrder.length])

  /**
   * 会话内冻结的时钟。切排序 / 手动刷新 / 重新进入才重算 —— 未读数字可以实时
   * 变，位置不能跟着动，否则每读一篇整墙都在挪。
   */
  const [scoreNow, setScoreNow] = useState(() => Date.now())

  /**
   * 跨源主题。只看 `item.topic`（离线打标写入），窗口近 30 天、不足 3 篇不成卡。
   * 数据来自源预览，不额外拉接口。
   */
  const topics = useMemo(
    () =>
      tileGrid
        ? clusterTopics(previewsToTopicItems(filteredSources), scoreNow)
        : [],
    [tileGrid, filteredSources, scoreNow],
  )

  // 排序后的源列表
  const sortedSources = useMemo(() => {
    const result = [...filteredSources]

    switch (sortMode) {
      case 'smart':
      case 'topic':
        // 两种新模式的源排序依据相同（分档分数），差别只在主题卡放几张、放哪 ——
        // 那是磁贴墙的事，见 BrewTileWall 的 topicMode。
        return result.sort((a, b) =>
          compareByScore(a, b, viewerRole, scoreNow),
        )

      case 'update':
        // 按最新成功更新 / 文章时间排序（最新的在前）。
        // Never fall back to last_fetched_at — failed fetches update it and
        // would surface broken sources as "recently updated".
        return result.sort((a, b) => {
          const latestA =
            a.recent_items?.[0]?.published_at || a.last_success_at || 0
          const latestB =
            b.recent_items?.[0]?.published_at || b.last_success_at || 0
          return latestB - latestA
        })

      case 'custom':
        // 按自定义顺序排序
        if (customOrder.length === 0) {
          return result // 如果没有自定义顺序，保持原样
        }
        return result.sort((a, b) => {
          const indexA = customOrder.indexOf(a.id)
          const indexB = customOrder.indexOf(b.id)
          // 如果不在自定义顺序中，放到最后
          if (indexA === -1) return 1
          if (indexB === -1) return -1
          return indexA - indexB
        })

      case 'category': {
        // 按分类排序（同分类内按名称排序）
        // 预置分类（友情链接、我）在排序时被忽略，取主分类进行排序
        const getMainCategory = (cat: string | null): string => {
          if (!cat) return ''
          const cats = cat
            .split(',')
            .map((c) => c.trim())
            .filter(Boolean)
          // 过滤掉预置分类，取第一个非预置分类
          const mainCat = cats.find(
            (c) => !PRESET_CATEGORY_DB_VALUES.includes(c),
          )
          return mainCat || cats[0] || ''
        }
        return result.sort((a, b) => {
          const catA = getMainCategory(a.category)
          const catB = getMainCategory(b.category)
          if (catA !== catB) {
            return catA.localeCompare(catB, 'zh-CN')
          }
          return a.name.localeCompare(b.name, 'zh-CN')
        })
      }

      case 'random':
        // 随机排序（使用种子确保同一会话内稳定）
        return result.sort((a, b) => {
          const hashA = (a.id * randomSeed) % 1000
          const hashB = (b.id * randomSeed) % 1000
          return hashA - hashB
        })

      case 'pinyin':
        // 按拼音排序（使用 localeCompare）
        return result.sort((a, b) => a.name.localeCompare(b.name, 'zh-CN'))

      default:
        return result
    }
  }, [filteredSources, sortMode, customOrder, randomSeed, viewerRole, scoreNow])

  // 分类排序时的分类标题生成（与磁贴墙的分类换页共用同一口径）
  const getMainCategoryForRender = (cat: string | null): string =>
    brewMainCategory(cat, t.brew.uncategorized)

  // 切换排序模式时的处理 - 带 FLIP 动画
  const handleSortModeChange = useCallback(
    (mode: SortMode) => {
      // 1. 记录当前所有卡片的位置
      recordCardPositions()

      // 2. 更新排序模式（触发重排序）
      setSortMode(mode)
      // 切排序是「重算时机」之一：分数与尺寸都按新的 now 重算
      setScoreNow(Date.now())

      // 如果切换到随机排序，更新种子
      if (mode === 'random') {
        setRandomSeed(Date.now())
      }
      // 如果切换到自由排序，初始化顺序
      if (mode === 'custom' && customOrder.length === 0) {
        // 优先按已保存的 sort_order 排序
        const sourcesWithOrder = filteredSources.filter(
          (s) => s.sort_order !== null && s.sort_order !== undefined,
        )
        if (sourcesWithOrder.length > 0) {
          const sortedByOrder = [...filteredSources].sort((a, b) => {
            const orderA = a.sort_order ?? Number.MAX_SAFE_INTEGER
            const orderB = b.sort_order ?? Number.MAX_SAFE_INTEGER
            return orderA - orderB
          })
          setCustomOrder(sortedByOrder.map((s) => s.id))
        } else {
          setCustomOrder(filteredSources.map((s) => s.id))
        }
      }

      // 3. 下一帧播放 FLIP 动画
      // 使用 setTimeout 确保 React 已经完成 DOM 更新
      setTimeout(() => {
        playFlipAnimations()
      }, 0)
    },
    [
      filteredSources,
      customOrder.length,
      recordCardPositions,
      playFlipAnimations,
    ],
  )

  // 拖拽排序 - 开始拖拽（仅编辑模式 + 自由排序模式）
  const handleCardDragStart = useCallback(
    (e: React.MouseEvent | React.TouchEvent, sourceId: number) => {
      if (!isEditMode || sortMode !== 'custom') return

      const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
      const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY

      dragStartPosRef.current = { x: clientX, y: clientY }
      setDraggingSourceId(sourceId)
    },
    [isEditMode, sortMode],
  )

  // 拖拽排序 - 移动和结束
  useEffect(() => {
    if (draggingSourceId === null) return

    let rafId: number | null = null

    const handleMove = (e: MouseEvent | TouchEvent) => {
      if (rafId !== null) return

      rafId = requestAnimationFrame(() => {
        rafId = null

        const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
        const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY

        // 遍历卡片，找到鼠标下的卡片
        let targetId: number | null = null
        cardRefsRef.current.forEach((element, id) => {
          if (id === draggingSourceId) return
          const rect = element.getBoundingClientRect()
          if (
            clientX >= rect.left &&
            clientX <= rect.right &&
            clientY >= rect.top &&
            clientY <= rect.bottom
          ) {
            targetId = id
          }
        })

        setDragOverSourceId(targetId)
      })
    }

    const handleEnd = async () => {
      if (rafId !== null) {
        cancelAnimationFrame(rafId)
      }

      // 如果有目标卡片，交换两者位置（1对1交换）
      if (dragOverSourceId !== null && draggingSourceId !== null) {
        let newFromIndex = -1
        let newToIndex = -1

        // 记录当前位置用于 FLIP 动画
        recordCardPositions()

        setCustomOrder((prev) => {
          const newOrder = [...prev]
          const fromIndex = newOrder.indexOf(draggingSourceId)
          const toIndex = newOrder.indexOf(dragOverSourceId)

          if (fromIndex !== -1 && toIndex !== -1) {
            // 1对1位置交换
            newOrder[fromIndex] = dragOverSourceId
            newOrder[toIndex] = draggingSourceId
            newFromIndex = fromIndex
            newToIndex = toIndex
          }

          return newOrder
        })

        // 播放 FLIP 动画
        setTimeout(playFlipAnimations, 0)

        // 保存交换后的排序到数据库；失败则回滚 order + toast
        if (newFromIndex !== -1 && newToIndex !== -1) {
          const fromId = draggingSourceId
          const toId = dragOverSourceId
          try {
            await Promise.all([
              brewApi.updateSource(fromId, {
                sort_order: newToIndex,
              }),
              brewApi.updateSource(toId, {
                sort_order: newFromIndex,
              }),
            ])
          } catch (err) {
            console.error('Failed to save sort order:', err)
            // Roll back local order swap
            setCustomOrder((prev) => {
              const rolled = [...prev]
              const a = rolled.indexOf(fromId)
              const b = rolled.indexOf(toId)
              if (a !== -1 && b !== -1) {
                rolled[a] = toId
                rolled[b] = fromId
              }
              return rolled
            })
            setTimeout(playFlipAnimations, 0)
            try {
              const { showToast } = await import('../../utils/toastManager')
              showToast({
                message: t.common.error || 'Failed to save sort order',
                type: 'error',
              })
            } catch {
              /* toast optional */
            }
          }
        }
      }

      setDraggingSourceId(null)
      setDragOverSourceId(null)
      dragStartPosRef.current = null
    }

    window.addEventListener('mousemove', handleMove)
    window.addEventListener('mouseup', handleEnd)
    window.addEventListener('touchmove', handleMove, { passive: false })
    window.addEventListener('touchend', handleEnd)

    return () => {
      if (rafId !== null) {
        cancelAnimationFrame(rafId)
      }
      window.removeEventListener('mousemove', handleMove)
      window.removeEventListener('mouseup', handleEnd)
      window.removeEventListener('touchmove', handleMove)
      window.removeEventListener('touchend', handleEnd)
    }
  }, [
    draggingSourceId,
    dragOverSourceId,
    recordCardPositions,
    playFlipAnimations,
  ])

  // 保存卡片ref
  const setCardRef = useCallback(
    (id: number, element: HTMLDivElement | null) => {
      if (element) {
        cardRefsRef.current.set(id, element)
      } else {
        cardRefsRef.current.delete(id)
      }
    },
    [],
  )

  // 图标颜色提取后保存到数据库
  const handleThemeColorExtracted = useCallback(
    async (sourceId: number, color: string) => {
      try {
        const updatedSource = await brewApi.updateSource(sourceId, {
          theme_color: color,
        })
        // 通知父组件更新
        if (onSourceUpdate) {
          onSourceUpdate(updatedSource)
        }
      } catch {
        // 保存主题色失败，静默处理
      }
    },
    [onSourceUpdate],
  )

  // 处理选择切换
  const handleToggleSelect = useCallback((sourceId: number) => {
    setSelectedIds((prev) => {
      const next = new Set(prev)
      if (next.has(sourceId)) {
        next.delete(sourceId)
      } else {
        next.add(sourceId)
      }
      return next
    })
  }, [])

  // 全选/取消全选
  const handleSelectAll = useCallback(() => {
    if (selectedIds.size === filteredSources.length) {
      setSelectedIds(new Set())
    } else {
      setSelectedIds(new Set(filteredSources.map((s) => s.id)))
    }
  }, [filteredSources, selectedIds.size])

  // 进入编辑模式
  const handleEnterEditMode = useCallback(() => {
    setIsEditMode(true)
  }, [])

  // 退出编辑模式
  const handleExitEditMode = useCallback(() => {
    setIsEditMode(false)
    setSelectedIds(new Set())
  }, [])

  // 批量删除
  const handleBatchDelete = useCallback(async () => {
    if (selectedIds.size === 0) return

    const ids = Array.from(selectedIds)
    setDeletingIds(ids)
    setIsDeleting(true)

    try {
      await Promise.all(ids.map((id) => brewApi.deleteSource(id)))
      setSelectedIds(new Set())
      onSourcesChange?.()
    } catch (err) {
      console.error('Failed to delete sources:', err)
    } finally {
      setDeletingIds([])
      setIsDeleting(false)
    }
  }, [selectedIds, onSourcesChange])

  // 批量刷新全部订阅
  const handleBatchRefresh = useCallback(async () => {
    const refreshableSources = filteredSources.filter(
      (source) => source.source_type !== 'link',
    )
    if (refreshableSources.length === 0) return

    setIsRefreshing(true)

    try {
      // 并行刷新所有订阅源
      await Promise.all(
        refreshableSources.map((source) => onRefreshSource(source.id)),
      )
    } catch (err) {
      console.error('Failed to refresh sources:', err)
    } finally {
      setIsRefreshing(false)
    }
  }, [filteredSources, onRefreshSource])

  // 全部订阅标记已读
  const [_isMarkingAllRead, setIsMarkingAllRead] = useState(false)
  const handleMarkAllSourcesRead = useCallback(async () => {
    if (!isAuthenticated) return

    setIsMarkingAllRead(true)
    try {
      // 按当前分类过滤
      await brewApi.markAllRead({ category: category || undefined })
      // 触发刷新
      onSourcesChange?.()
    } catch (err) {
      console.error('Failed to mark all sources read:', err)
    } finally {
      setIsMarkingAllRead(false)
    }
  }, [isAuthenticated, category, onSourcesChange])

  // Resize 状态
  const [resizingSource, setResizingSource] = useState<{
    sourceId: number
    startY: number
    startSize: CardSize
    previewSize: CardSize // 预览尺寸（拖拽时显示）
  } | null>(null)

  // 尺寸阈值配置（基于拖动距离）
  const SIZE_ORDER: CardSize[] = ['tiny', 'mini', 'full']
  const RESIZE_THRESHOLD = 100 // 每个尺寸变化需要的像素距离

  // 开始拖拽调整尺寸
  const handleResizeStart = useCallback(
    (e: React.MouseEvent | React.TouchEvent, sourceId: number) => {
      if (!isEditMode) return

      const source = sources.find((s) => s.id === sourceId)
      if (!source) return

      const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY
      const currentSize = source.card_size || 'mini'

      setResizingSource({
        sourceId,
        startY: clientY,
        startSize: currentSize,
        previewSize: currentSize,
      })
    },
    [isEditMode, sources],
  )

  // 拖拽移动处理 - 更新预览尺寸并触发 FLIP 动画
  useEffect(() => {
    if (!resizingSource) return

    const handleMove = (e: MouseEvent | TouchEvent) => {
      const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY
      const deltaY = clientY - resizingSource.startY

      // 计算目标尺寸
      const startIndex = SIZE_ORDER.indexOf(resizingSource.startSize)
      const sizeChange = Math.round(deltaY / RESIZE_THRESHOLD)
      const targetIndex = Math.max(
        0,
        Math.min(SIZE_ORDER.length - 1, startIndex + sizeChange),
      )
      const targetSize = SIZE_ORDER[targetIndex]

      // 尺寸变化时触发 FLIP 动画
      if (targetSize !== resizingSource.previewSize) {
        // 1. 记录当前位置（在 DOM 更新前）
        recordCardPositions()
        // 2. 更新预览尺寸（触发重渲染）
        setResizingSource((prev) =>
          prev ? { ...prev, previewSize: targetSize } : null,
        )
        // 3. 双层 RAF 确保 React 渲染完成后再播放动画
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            playFlipAnimations()
          })
        })
      }
    }

    const handleEnd = async () => {
      if (!resizingSource) return

      const source = sources.find((s) => s.id === resizingSource.sourceId)
      const finalSize = resizingSource.previewSize

      // 尺寸有变化时才更新
      if (source && finalSize !== resizingSource.startSize) {
        // 记录所有卡片位置用于 FLIP 动画
        recordCardPositions()

        // 更新数据
        const optimisticSource: BrewSource = {
          ...source,
          card_size: finalSize,
        }
        onSourceUpdate?.(optimisticSource)

        // 播放 FLIP 动画
        setTimeout(playFlipAnimations, 0)

        // 保存到数据库
        try {
          await brewApi.updateSource(source.id, { card_size: finalSize })
        } catch (err) {
          console.error('Failed to save card size:', err)
          // 失败时回滚
          const rollbackSource: BrewSource = {
            ...source,
            card_size: resizingSource.startSize,
          }
          onSourceUpdate?.(rollbackSource)
        }
      }

      setResizingSource(null)
    }

    window.addEventListener('mousemove', handleMove)
    window.addEventListener('mouseup', handleEnd)
    window.addEventListener('touchmove', handleMove)
    window.addEventListener('touchend', handleEnd)

    return () => {
      window.removeEventListener('mousemove', handleMove)
      window.removeEventListener('mouseup', handleEnd)
      window.removeEventListener('touchmove', handleMove)
      window.removeEventListener('touchend', handleEnd)
    }
  }, [
    resizingSource,
    sources,
    onSourceUpdate,
    recordCardPositions,
    playFlipAnimations,
  ])

  /**
   * 「锁定当前尺寸」：把当前派生出来的尺寸写回 `card_size`，或清空解锁。
   *
   * 取代了右下角拖拉 resize —— 尺寸本来由分数派生，手拉一个再被下次装箱
   * 推走只会让人以为坏了。零 migration：`card_size` 的语义从「手工尺寸」
   * 变成「用户锁定」，映射见 logic/layout。
   */
  const handleToggleSizeLock = useCallback(
    async (source: BrewSource, next: '2x2' | '4x2' | '4x4' | null) => {
      const CARD_SIZE_BY_TILE = {
        '2x2': 'tiny',
        '4x2': 'mini',
        '4x4': 'full',
      } as const
      const nextCardSize: CardSize | null = next
        ? CARD_SIZE_BY_TILE[next]
        : null

      // 乐观更新：磁贴尺寸立刻生效，失败再回滚
      onSourceUpdate?.({ ...source, card_size: nextCardSize })
      try {
        await brewApi.updateSource(source.id, {
          // 空字符串 = 解锁（后端与 theme_color / icon 同一约定）
          card_size: nextCardSize ?? '',
        })
      } catch (err) {
        console.error('Failed to save size lock:', err)
        onSourceUpdate?.(source)
      }
    },
    [onSourceUpdate],
  )

  /** 主题卡点击：把主题 key 与 i18n key 一起交出去，文案由上层查 t.brew */
  const handleTopicClick = useCallback(
    (topic: { key: string; nameKey: string }) => {
      onTopicClick?.(topic.key, topic.nameKey)
    },
    [onTopicClick],
  )

  // 处理编辑保存
  const handleEditSave = useCallback(
    async (
      id: number,
      data: {
        name?: string
        category?: string
        update_interval?: number
        enabled?: boolean
        icon?: string
        ai_style_tags?: string[]
      },
    ) => {
      // 如果更新了图标，同时清除主题色，让图标加载时重新提取
      const updateData =
        data.icon !== undefined
          ? { ...data, theme_color: '' } // 清除主题色
          : data
      const updatedSource = await brewApi.updateSource(id, updateData)
      onSourceUpdate?.(updatedSource)
      onSourcesChange?.()
    },
    [onSourceUpdate, onSourcesChange],
  )

  // 处理单个删除
  const handleDeleteSource = useCallback(
    async (sourceId: number) => {
      setDeletingIds([sourceId])
      try {
        await brewApi.deleteSource(sourceId)
        onSourcesChange?.()
      } catch (err) {
        console.error('Failed to delete source:', err)
      } finally {
        setDeletingIds([])
      }
    },
    [onSourcesChange],
  )

  if (filteredSources.length === 0 && !searchQuery) {
    return (
      <div className="relative min-h-[60vh]">
        {/* 控制岛 - 放在内容顶部 */}
        <ControlIsland
          sources={sources}
          filteredSources={sortedSources}
          categories={categories}
          searchQuery={searchQuery}
          setSearchQuery={setSearchQuery}
          selectedIds={selectedIds}
          onSelectAll={handleSelectAll}
          onBatchDelete={handleBatchDelete}
          onBatchRefresh={handleBatchRefresh}
          onMarkAllSourcesRead={handleMarkAllSourcesRead}
          onEnterEditMode={handleEnterEditMode}
          onExitEditMode={handleExitEditMode}
          isEditMode={isEditMode}
          isDeleting={isDeleting}
          isRefreshing={isRefreshing}
          onAddSource={onAddSource}
          onSourcesChange={onSourcesChange}
          sortMode={sortMode}
          onSortModeChange={handleSortModeChange}
          isSubCategory={!!category}
          isAdmin={isAdmin}
          isAuthenticated={isAuthenticated}
          topicCount={topics.length}
        />

        <div className="flex flex-col items-start py-8">
          <div className="rounded-2xl glass-surface glass-90 border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10 flex items-center gap-3 px-5 py-3">
            <div className="w-9 h-9 rounded-xl bg-gray-100/80 dark:bg-white/5 flex items-center justify-center text-gray-400 dark:text-gray-500 shrink-0">
              <Rss className="w-5.5 h-5.5" />
            </div>
            <div className="min-w-0">
              <p className="text-sm font-medium text-gray-600 dark:text-gray-300">
                {category
                  ? t.brew.emptyCategoryNoSources.replace(
                      '{category}',
                      category,
                    )
                  : t.brew.emptyNoSources}
              </p>
              <p className="text-xs text-gray-400 dark:text-gray-500 mt-0.5 leading-snug">
                {t.brew.addSourceHint}
              </p>
            </div>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="relative pb-24 sm:pb-16">
      {/* 移动端控制岛 - 放在内容顶部 */}
      <ControlIsland
        sources={sources}
        filteredSources={sortedSources}
        categories={categories}
        searchQuery={searchQuery}
        setSearchQuery={setSearchQuery}
        selectedIds={selectedIds}
        onSelectAll={handleSelectAll}
        onBatchDelete={handleBatchDelete}
        onBatchRefresh={handleBatchRefresh}
        onMarkAllSourcesRead={handleMarkAllSourcesRead}
        onEnterEditMode={handleEnterEditMode}
        onExitEditMode={handleExitEditMode}
        isEditMode={isEditMode}
        isDeleting={isDeleting}
        isRefreshing={isRefreshing}
        onAddSource={onAddSource}
        onSourcesChange={onSourcesChange}
        sortMode={sortMode}
        onSortModeChange={handleSortModeChange}
        isSubCategory={!!category}
        isAdmin={isAdmin}
        isAuthenticated={isAuthenticated}
        topicCount={topics.length}
      />

      {/* 空搜索结果 */}
      {filteredSources.length === 0 && searchQuery && (
        <div className="flex flex-col items-center justify-center py-16 text-gray-500 dark:text-gray-400">
          <Search className="w-12 h-12 mb-3 opacity-30" />
          <p className="text-lg font-medium text-gray-700 dark:text-gray-300">
            {t.brew.noMatchingSources}
          </p>
          <p className="text-sm mt-1 opacity-70">{t.brew.tryOtherKeywords}</p>
        </div>
      )}

      {/* 磁贴墙（flag 开）：虚拟坐标 + 横向分页。
          flag 关时下面的旧 CSS Grid 保持像素级不变，观察两周后在 PR 9 一起清退。 */}
      {tileGrid && sortedSources.length > 0 && (
        <BrewTileWall
          sources={sortedSources}
          // smart 只把**前 2 张**主题卡插到最前（§8）；插全部会把第一屏的源
          // 全挤到后面去，打开 /brew 第一眼看不到任何订阅源。topic 模式才全上。
          topics={
            sortMode === 'topic'
              ? topics
              : sortMode === 'smart'
                ? topics.slice(0, SMART_TOPIC_CARDS)
                : []
          }
          topicMode={sortMode === 'topic' ? 'topic' : 'smart'}
          onTopicClick={handleTopicClick}
          role={viewerRole}
          isSearching={Boolean(searchQuery.trim())}
          scope={`${category ?? 'all'}:${sortMode}`}
          breakOnCategory={sortMode === 'category'}
          uncategorizedLabel={t.brew.uncategorized}
          prevPageLabel={t.brew.tilePagePrev}
          nextPageLabel={t.brew.tilePageNext}
          pageLabel={t.brew.tilePageNth}
          onSourceClick={onSourceClick}
          registerCardRef={setCardRef}
          isEditMode={isEditMode}
          onDragStart={handleCardDragStart}
          draggingSourceId={draggingSourceId}
          dragOverSourceId={dragOverSourceId}
          onToggleSizeLock={handleToggleSizeLock}
        />
      )}

      {/* 卡片网格（旧） */}
      {!tileGrid && sortedSources.length > 0 && (
        <div
          className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3 md:gap-4"
          style={{ gridAutoRows: '1.5rem' }}
        >
          {sortedSources.map((source, index) => {
            // 分类排序模式下，检查是否需要显示分类标题
            const showCategoryHeader =
              sortMode === 'category' &&
              (index === 0 ||
                getMainCategoryForRender(sortedSources[index - 1].category) !==
                  getMainCategoryForRender(source.category))
            const currentCategory = getMainCategoryForRender(source.category)

            return (
              <React.Fragment key={source.id}>
                {/* 分类标题 */}
                {showCategoryHeader && (
                  <div
                    className="col-span-1 md:col-span-2 xl:col-span-3 flex items-center gap-2 pt-6 pb-1 first:pt-0"
                    style={{ gridRow: 'span 2' }}
                  >
                    <span className="px-3 py-1.5 rounded-lg glass-surface glass-60 text-[13px] font-medium text-gray-600 dark:text-gray-300 shadow-sm ring-1 ring-black/5 dark:ring-white/10">
                      {currentCategory}
                      <span className="ml-1.5 text-[11px] text-gray-400 dark:text-gray-500">
                        {
                          sortedSources.filter(
                            (s) =>
                              getMainCategoryForRender(s.category) ===
                              currentCategory,
                          ).length
                        }
                      </span>
                    </span>
                  </div>
                )}
                <SourceCard
                  ref={(el) => setCardRef(source.id, el)}
                  source={source}
                  index={index}
                  onSourceClick={
                    isEditMode
                      ? () => handleToggleSelect(source.id)
                      : onSourceClick
                  }
                  onRefreshSource={onRefreshSource}
                  onThemeColorExtracted={handleThemeColorExtracted}
                  isEditMode={isEditMode}
                  isSelected={selectedIds.has(source.id)}
                  isDeleting={deletingIds.includes(source.id)}
                  onEdit={() => setEditingSource(source)}
                  onDelete={() => handleDeleteSource(source.id)}
                  onResizeStart={handleResizeStart}
                  previewSize={
                    resizingSource?.sourceId === source.id
                      ? resizingSource.previewSize
                      : undefined
                  }
                  isDragging={draggingSourceId === source.id}
                  isDragOver={dragOverSourceId === source.id}
                  onDragStart={handleCardDragStart}
                  sortMode={sortMode}
                  isAuthenticated={isAuthenticated}
                />
              </React.Fragment>
            )
          })}
        </div>
      )}

      {/* 编辑弹窗 */}
      {editingSource && (
        <EditModal
          source={editingSource}
          categories={categories}
          onClose={() => setEditingSource(null)}
          onSave={handleEditSave}
        />
      )}
    </div>
  )
}
