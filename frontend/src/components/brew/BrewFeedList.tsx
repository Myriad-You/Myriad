/**
 * Brew 文章列表组件
 * 设计参考 TappCard 风格 - 瀑布流卡片展示
 *
 * 性能优化（WebKit 重点优化）：
 * - 移除 framer-motion，使用纯 CSS 动画
 * - 减少 transition 属性数量
 * - 使用 React.memo + 自定义比较避免重渲染
 * - 图片懒加载 + decoding="async"
 */

import type { BrewItem } from '../../types/brew'
import type { TimeTranslations } from './types'
import { LuFileText as FileText } from '@lib/icons'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { ItemCard } from './cards'
import { DEFAULT_THEME_COLOR, getPlainText } from './constants'

interface BrewFeedListProps {
  items: BrewItem[]
  selectedItem: BrewItem | null
  loading: boolean
  hasMore: boolean
  total: number
  onItemSelect: (item: BrewItem) => void
  onToggleStar: (item: BrewItem) => void
  onLoadMore: () => void
  sourceColors?: Map<number, string>
  // 编辑模式相关
  editMode?: boolean
  selectedIds?: Set<number>
  onItemSelectToggle?: (id: number) => void
  // 是否已登录（游客隐藏收藏按钮）
  isAuthenticated?: boolean
}

// ==================== BrewFeedList 主组件 ====================

export default function BrewFeedList({
  items,
  selectedItem,
  loading,
  hasMore,
  total,
  onItemSelect,
  onToggleStar,
  onLoadMore,
  sourceColors,
  editMode,
  selectedIds,
  onItemSelectToggle,
  isAuthenticated = false, // 默认游客模式
}: BrewFeedListProps) {
  const { t, locale } = useI18n()
  const observerRef = useRef<IntersectionObserver | null>(null)
  const [columnCount, setColumnCount] = useState(2)

  // 缓存翻译对象
  const timeTranslations = useMemo<TimeTranslations>(() => ({
    justNow: t.brew.justNow,
    minutesAgo: t.brew.minutesAgo,
    hoursAgo: t.brew.hoursAgo,
    daysAgo: t.brew.daysAgo,
  }), [t.brew])

  const brewTranslations = useMemo(() => ({
    hasAnnotations: t.brew.hasAnnotations,
    annotationsLabel: t.brew.annotationsLabel,
    hasPodcast: t.brew.hasPodcast,
    podcastLabel: t.brew.podcastLabel,
    unstarArticle: t.brew.unstarArticle,
    starArticle: t.brew.starArticle,
    openInNewTab: t.brew.openInNewTab,
  }), [t.brew])

  // 响应式列数 - 使用 ResizeObserver 替代 resize 事件（更高效，避免防抖）
  const containerRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const updateColumnCount = (width: number) => {
      setColumnCount(width < 640 ? 1 : 2)
    }

    // 初始设置
    updateColumnCount(window.innerWidth)

    // 使用 ResizeObserver 监听容器宽度变化
    if (containerRef.current && typeof ResizeObserver !== 'undefined') {
      const observer = new ResizeObserver((entries) => {
        const entry = entries[0]
        if (entry) {
          updateColumnCount(entry.contentRect.width)
        }
      })
      observer.observe(containerRef.current)
      return () => observer.disconnect()
    }

    // 降级方案：使用 resize 事件
    const handleResize = () => updateColumnCount(window.innerWidth)
    window.addEventListener('resize', handleResize)
    return () => window.removeEventListener('resize', handleResize)
  }, [])

  // 缓存主题色获取函数
  const getItemThemeColor = useCallback((item: BrewItem): string => {
    return sourceColors?.get(item.source_id) || DEFAULT_THEME_COLOR
  }, [sourceColors])

  // 瀑布流列分配 - 最短列优先算法（根据预估高度平衡列）
  const columns = useMemo(() => {
    const cols: BrewItem[][] = Array.from({ length: columnCount }, () => [])
    const colHeights: number[] = new Array(columnCount).fill(0)

    // 预估卡片高度：基础高度 + 封面图高度 + 摘要行数
    const estimateHeight = (item: BrewItem): number => {
      let height = 140 // 基础高度（标题、元信息、padding）
      if (item.image)
        height += 80 // 封面图
      if (item.summary) {
        const textLen = getPlainText(item.summary).length
        height += Math.min(Math.ceil(textLen / 40) * 22, 66) // 每行约22px，最多3行
      }
      return height
    }

    items.forEach((item) => {
      // 找到当前最短的列
      let shortestCol = 0
      let minHeight = colHeights[0]
      for (let i = 1; i < columnCount; i++) {
        if (colHeights[i] < minHeight) {
          minHeight = colHeights[i]
          shortestCol = i
        }
      }

      cols[shortestCol].push(item)
      colHeights[shortestCol] += estimateHeight(item)
    })

    return cols
  }, [items, columnCount])

  // 缓存最后一项 ID
  const lastItemId = useMemo(() =>
    items.length > 0 ? items[items.length - 1].id : null, [items])

  // 无限滚动加载 - 使用 IntersectionObserver
  const lastItemRef = useCallback(
    (node: HTMLDivElement | null) => {
      if (loading)
        return
      if (observerRef.current)
        observerRef.current.disconnect()

      observerRef.current = new IntersectionObserver(
        (entries) => {
          if (entries[0].isIntersecting && hasMore) {
            onLoadMore()
          }
        },
        { rootMargin: '100px' }, // 提前 100px 开始加载
      )

      if (node)
        observerRef.current.observe(node)
    },
    [loading, hasMore, onLoadMore],
  )

  // 空状态
  if (items.length === 0 && !loading) {
    return (
      <div className="flex flex-col items-center justify-center py-20 text-gray-500 dark:text-gray-400">
        <div className="w-20 h-20 rounded-2xl bg-gray-500/10 flex items-center justify-center mb-4">
          <FileText className="w-10 h-10 text-gray-400/50" />
        </div>
        <p className="text-lg font-medium text-gray-700 dark:text-gray-300">{t.brew.noArticles}</p>
        <p className="text-sm mt-1 opacity-70">{t.brew.subscribeMoreSources}</p>
      </div>
    )
  }

  return (
    <div className="w-full" ref={containerRef}>
      {/* 文章列表 - 瀑布流布局 */}
      <div className="flex gap-4">
        {columns.map((columnItems, colIndex) => (
          <div key={colIndex} className="flex-1 flex flex-col gap-4">
            {columnItems.map((item, itemIndex) => (
              <ItemCard
                key={item.id}
                item={item}
                index={colIndex * columnItems.length + itemIndex}
                isSelected={selectedItem?.id === item.id}
                isLast={item.id === lastItemId}
                themeColor={getItemThemeColor(item)}
                onItemSelect={onItemSelect}
                onToggleStar={onToggleStar}
                lastItemRef={item.id === lastItemId ? lastItemRef : undefined}
                editMode={editMode}
                isChecked={selectedIds?.has(item.id)}
                onToggleCheck={onItemSelectToggle}
                isAuthenticated={isAuthenticated}
                timeTranslations={timeTranslations}
                brewTranslations={brewTranslations}
                locale={locale}
              />
            ))}
          </div>
        ))}
      </div>

      {/* 加载中 */}
      {loading && (
        <div className="flex justify-center py-8">
          <div className="w-7 h-7 border-2 border-gray-300 dark:border-gray-600 border-t-gray-500 dark:border-t-gray-300 rounded-full animate-spin" />
        </div>
      )}

      {/* 没有更多 */}
      {!loading && !hasMore && items.length > 0 && (
        <p className="text-center py-8 text-gray-400 dark:text-gray-500 text-sm">
          {t.brew.loadedAllArticles.replace('{count}', String(total))}
        </p>
      )}
    </div>
  )
}
