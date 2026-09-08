/**
 * 订阅源卡片组件
 * 从 BrewSourceGrid 提取，独立维护
 *
 * 性能优化：
 * - React.memo + 自定义比较函数
 * - useCallback 缓存回调
 * - useMemo 缓存样式计算
 */

import type { TranslationKeys } from '../../../i18n'
import type { BrewSource, CardSize, SourceType } from '../../../types/brew'
import type { SortMode } from '../types'
import {
  LuEdit3 as Edit3,
  LuExternalLink as ExternalLink,
  LuRefreshCw as RefreshCw,
  LuRss as Rss,
  LuSparkles as Sparkles,
} from '@lib/icons'

import React, {
  forwardRef,
  useCallback,
  useEffect,
  useMemo,
  useState,
} from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { useBrewCardStagger } from '../../../hooks/animation'
import { isExlight } from '../../../hooks/useAnimationLevel'
import { extractColorsFromLoadedImage } from '../../../utils/colorExtractor'
import {
  DEFAULT_THEME_COLOR,
  getIconUrl,
  getImageUrl,
  getSourceColor,
  SIZE_TO_ROWS,
  stripHtml,
} from '../constants'

export interface SourceCardProps {
  source: BrewSource
  index: number
  onSourceClick: (source: BrewSource) => void
  onRefreshSource: (sourceId: number) => void
  onThemeColorExtracted?: (sourceId: number, color: string) => void
  // 编辑模式
  isEditMode?: boolean
  isSelected?: boolean
  isDeleting?: boolean
  onEdit?: () => void
  onDelete?: () => void
  // 尺寸调整
  onResizeStart?: (
    e: React.MouseEvent | React.TouchEvent,
    sourceId: number,
  ) => void
  previewSize?: CardSize
  // 拖拽排序
  isDragging?: boolean
  isDragOver?: boolean
  onDragStart?: (
    e: React.MouseEvent | React.TouchEvent,
    sourceId: number,
  ) => void
  sortMode?: SortMode
  /**
   * 已登录才有真实已读态。游客侧 `list_sources` 用 `user_id = -1` 做 LEFT JOIN，
   * `recent_items[].is_read` 恒为 false —— 未读圆点必须按角色隐藏，不是改后端 SQL。
   */
  isAuthenticated?: boolean
}

// 获取 Feed 类型标签
function getFeedTypeLabel(type: string, t: TranslationKeys): string {
  switch (type) {
    case 'atom':
      return t.brew.feedTypeAtom
    case 'json_feed':
      return t.brew.feedTypeJson
    case 'rss':
      return t.brew.feedTypeRss
    default:
      return type.toUpperCase()
  }
}

// 获取来源类型标签和提示
function getSourceTypeInfo(
  sourceType: SourceType,
  t: TranslationKeys,
): { label: string; tooltip: string; isBrewlia: boolean } {
  switch (sourceType) {
    case 'brewlia':
      return {
        label: 'Brewlia AI',
        tooltip: t.brew.feedTypeBrewliaDesc,
        isBrewlia: true,
      }
    case 'link':
      return {
        label: t.brew.feedTypeLink,
        tooltip: t.brew.feedTypeLinkDesc,
        isBrewlia: false,
      }
    case 'rss':
    default:
      return {
        label: '',
        tooltip: t.brew.feedTypeRssDesc,
        isBrewlia: false,
      }
  }
}

// 格式化时间
function formatTime(timestamp: number | null, t: TranslationKeys): string {
  if (!timestamp) return ''
  const date = new Date(timestamp)
  const now = new Date()
  const diff = now.getTime() - date.getTime()

  if (diff < 60000) return t.brew.justNow
  if (diff < 3600000) {
    return t.brew.minutesAgo.replace(
      '{minutes}',
      String(Math.floor(diff / 60000)),
    )
  }
  if (diff < 86400000) {
    return t.brew.hoursAgo.replace(
      '{hours}',
      String(Math.floor(diff / 3600000)),
    )
  }
  if (diff < 604800000) {
    return t.brew.daysAgo.replace('{days}', String(Math.floor(diff / 86400000)))
  }
  return date.toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' })
}

export const SourceCard = React.memo(
  forwardRef<HTMLDivElement, SourceCardProps>(
    (
      {
        source,
        index,
        onSourceClick,
        onRefreshSource,
        onThemeColorExtracted,
        isEditMode = false,
        isSelected = false,
        isDeleting = false,
        onEdit,
        onDelete: _onDelete,
        onResizeStart,
        previewSize,
        isDragging = false,
        isDragOver = false,
        onDragStart,
        sortMode,
        isAuthenticated = false,
      },
      ref,
    ) => {
      const [isHovered, setIsHovered] = useState(false)
      // Soft-fail proxy returns 1×1 PNG (HTTP 200) for dead favicons — treat as missing
      const [iconBroken, setIconBroken] = useState(false)
      const { t } = useI18n()

      // 接入动画调度器
      const {
        canAnimate,
        delay: _delay,
        onComplete,
        animConfig,
        initialStyle: _initialStyle,
        animateStyle,
      } = useBrewCardStagger(index, 'source')

      const enableHover = !isExlight(animConfig)

      const color = getSourceColor(source)
      const hasUnread = source.unread_count > 0
      const recentItems = source.recent_items || []
      const size: CardSize =
        source.source_type === 'link'
          ? 'tiny'
          : previewSize || source.card_size || 'mini'
      const needsColorExtract = !source.theme_color && Boolean(source.icon)
      const showSourceIcon = Boolean(source.icon) && !iconBroken

      // Reset broken state when the source icon URL changes (no re-request loops)
      useEffect(() => {
        setIconBroken(false)
      }, [source.icon])

      // 图标加载后提取颜色；1×1 soft-fail placeholder → Rss fallback
      const handleIconLoad = useCallback(
        (e: React.SyntheticEvent<HTMLImageElement>) => {
          const img = e.currentTarget
          // Backend soft-fail placeholder is a 1×1 transparent PNG
          if (img.naturalWidth <= 1 && img.naturalHeight <= 1) {
            setIconBroken(true)
            return
          }
          if (source.theme_color || !source.icon) return

          try {
            const palette = extractColorsFromLoadedImage(img)
            if (
              palette.primary &&
              palette.primary !== DEFAULT_THEME_COLOR &&
              palette.primary !== '#6b7280'
            ) {
              onThemeColorExtracted?.(source.id, palette.primary)
            }
          } catch {
            // 图标颜色提取失败，忽略
          }
        },
        [source.id, source.theme_color, source.icon, onThemeColorExtracted],
      )

      // 刷新处理（fire-and-forget：结果由未读计数/信息流更新体现，
      // 此处不设 loading 态——历史上的 refreshing 状态因未 await 从未渲染过）
      const handleRefresh = useCallback(
        (e: React.MouseEvent) => {
          e.stopPropagation()
          onRefreshSource(source.id)
        },
        [source.id, onRefreshSource],
      )

      // 点击卡片处理
      const handleCardClick = useCallback(() => {
        if (source.source_type === 'link' && !isEditMode) {
          const targetUrl = source.site_url || source.url
          if (targetUrl) {
            window.open(targetUrl, '_blank', 'noopener,noreferrer')
          }
          return
        }
        onSourceClick(source)
      }, [source, onSourceClick, isEditMode])

      // 鼠标事件
      const handleMouseEnter = useCallback(() => setIsHovered(true), [])
      const handleMouseLeave = useCallback(() => setIsHovered(false), [])

      // 获取 row-span
      const rowSpan = useMemo(() => SIZE_TO_ROWS[size], [size])

      // 合并 ref
      const setRef = useCallback(
        (el: HTMLDivElement | null) => {
          if (typeof ref === 'function') {
            ref(el)
          } else if (ref) {
            ref.current = el
          }
        },
        [ref],
      )

      // 通知调度器动画完成
      useEffect(() => {
        if (canAnimate) {
          onComplete?.()
        }
      }, [canAnimate, onComplete])

      // 处理编辑点击
      const handleEdit = useCallback(
        (e: React.MouseEvent) => {
          e.stopPropagation()
          onEdit?.()
        },
        [onEdit],
      )

      // 处理拉伸开始
      const handleResizeStart = useCallback(
        (e: React.MouseEvent | React.TouchEvent) => {
          e.stopPropagation()
          e.preventDefault()
          onResizeStart?.(e, source.id)
        },
        [source.id, onResizeStart],
      )

      // 处理拖拽开始
      const handleDragStart = useCallback(
        (e: React.MouseEvent | React.TouchEvent) => {
          e.stopPropagation()
          e.preventDefault()
          onDragStart?.(e, source.id)
        },
        [source.id, onDragStart],
      )

      // 是否显示拖拽手柄
      const showDragHandle = isEditMode && sortMode === 'custom'

      // 计算卡片样式
      const cardStyle = useMemo<React.CSSProperties>(() => {
        const base: React.CSSProperties = {
          gridRow: `span ${rowSpan}`,
        }

        if (isDragging) {
          return {
            ...base,
            transform: 'scale(1.02)',
            opacity: 0.9,
            zIndex: 100,
          }
        }

        if (isDragOver) {
          return {
            ...base,
            transform: 'scale(0.98)',
            opacity: 0.85,
            zIndex: 50,
          }
        }

        if (isSelected) {
          return {
            ...base,
            ...animateStyle,
            boxShadow: `inset 0 0 0 2px ${color}`,
          }
        }

        return {
          ...base,
          ...animateStyle,
        }
      }, [rowSpan, isDragging, isDragOver, isSelected, color, animateStyle])

      // 悬停过渡类名
      const hoverClasses = useMemo(() => {
        if (!enableHover || isDragging || isDragOver) return ''
        return 'hover:-translate-y-px'
      }, [enableHover, isDragging, isDragOver])

      // 尺寸相关的样式类。入口型来源的 bar 在老网格里没有独立形态
      // （列宽由 CSS Grid 定，锁不住宽度），按高度最接近的一档取样式
      const sizeClasses = useMemo(() => {
        const paddingMap: Record<CardSize, string> = {
          bar: 'p-2.5 px-3.5',
          tiny: 'p-2.5 px-3.5',
          mini: 'p-4',
          full: 'p-5',
        }
        const roundedMap: Record<CardSize, string> = {
          bar: 'rounded-xl',
          tiny: 'rounded-xl',
          mini: 'rounded-xl',
          full: 'rounded-2xl',
        }
        return {
          padding: paddingMap[size],
          rounded: roundedMap[size],
        }
      }, [size])

      const contentTransition = ''

      return (
        <div
          ref={setRef}
          onClick={handleCardClick}
          onMouseEnter={handleMouseEnter}
          onMouseLeave={handleMouseLeave}
          className={`group relative ${sizeClasses.rounded} overflow-hidden bg-white/90 dark:bg-neutral-900/90 cursor-pointer ${isDeleting ? 'opacity-50 pointer-events-none' : ''} ${hoverClasses}`}
          style={cardStyle}
        >
          {/* 拖拽手柄 */}
          {showDragHandle && (
            <div
              className={`absolute left-0 right-0 top-0 ${size === 'tiny' ? 'h-6' : size === 'mini' ? 'h-8' : 'h-10'} flex items-center justify-center cursor-grab active:cursor-grabbing z-10 opacity-0 group-hover:opacity-100 touch-none`}
              onMouseDown={handleDragStart}
              onTouchStart={handleDragStart}
            >
              <div className="flex items-center gap-0.75 px-2.5 py-1 rounded-full bg-black/5 dark:bg-white/10">
                <div
                  className={`${size === 'full' ? 'w-1.25 h-1.25' : 'w-0.75 h-0.75'} rounded-full bg-gray-400/80`}
                />
                <div
                  className={`${size === 'full' ? 'w-1.25 h-1.25' : 'w-0.75 h-0.75'} rounded-full bg-gray-400/80`}
                />
                <div
                  className={`${size === 'full' ? 'w-1.25 h-1.25' : 'w-0.75 h-0.75'} rounded-full bg-gray-400/80`}
                />
              </div>
            </div>
          )}

          {/* 背景光效 */}
          <div
            className={`absolute inset-0 ${isHovered ? 'opacity-[0.12]' : 'opacity-[0.06]'}`}
            style={{
              background: `linear-gradient(135deg, ${color}, transparent 65%)`,
            }}
          />

          {/* 装饰光效 - 右上 */}
          {size !== 'tiny' && (
            <div
              className={`absolute -right-4 -top-4 w-16 h-16 rounded-full blur-2xl ${
                hasUnread ? 'opacity-20' : 'opacity-10'
              }`}
              style={{
                background: `linear-gradient(135deg, ${color}, transparent 70%)`,
              }}
            />
          )}

          {/* 装饰光效 - 左下 */}
          {size === 'full' && (
            <div
              className="absolute -left-6 -bottom-6 w-16 h-16 rounded-full blur-xl opacity-10"
              style={{
                background: `radial-gradient(circle, ${color}, transparent 60%)`,
              }}
            />
          )}

          {/* 主内容区域 */}
          <div
            className={`absolute inset-0 ${sizeClasses.padding} flex flex-col ${contentTransition}`}
          >
            {/* 顶部：图标 + 名称 + 元信息 */}
            <div
              className={`flex items-center gap-3 shrink-0 ${contentTransition}`}
            >
              {/* 图标 */}
              <div
                className={`${size === 'tiny' ? 'w-10 h-10' : size === 'mini' ? 'w-9 h-9' : 'w-11 h-11'} rounded-xl flex items-center justify-center relative overflow-hidden shrink-0 ${contentTransition}`}
              >
                {showSourceIcon ? (
                  <img
                    src={getIconUrl(source.icon) || ''}
                    alt=""
                    // Only set CORS when canvas color extraction is needed;
                    // otherwise avoid extra taint/CORS failures on some CDNs.
                    {...(needsColorExtract
                      ? { crossOrigin: 'anonymous' as const }
                      : {})}
                    className={`${size === 'tiny' ? 'w-9 h-9' : size === 'mini' ? 'w-8 h-8' : 'w-10 h-10'} rounded-lg object-cover ${contentTransition}`}
                    loading="lazy"
                    decoding="async"
                    onLoad={handleIconLoad}
                    onError={() => setIconBroken(true)}
                  />
                ) : (
                  <div
                    className="w-full h-full rounded-xl flex items-center justify-center"
                    style={{
                      background: `linear-gradient(135deg, ${color}, ${color}dd)`,
                    }}
                  >
                    <Rss
                      className={`${size === 'full' ? 'w-5 h-5' : 'w-4 h-4'} text-white`}
                    />
                  </div>
                )}
              </div>

              {/* 名称 + 元信息 */}
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <h3
                    className={`font-bold text-gray-800 dark:text-gray-100 truncate leading-none ${size === 'full' ? 'text-[17px]' : 'text-[15px]'} ${contentTransition}`}
                  >
                    {source.name}
                  </h3>
                  {hasUnread && (
                    <span
                      className={`${size === 'full' ? 'w-2.5 h-2.5' : 'w-2 h-2'} rounded-full shrink-0 ${size === 'tiny' ? 'animate-pulse' : ''}`}
                      style={{ backgroundColor: color }}
                    />
                  )}
                </div>
                <div
                  className={`flex items-center gap-2 mt-1 ${contentTransition}`}
                >
                  {(() => {
                    const sourceInfo = getSourceTypeInfo(source.source_type, t)
                    const feedLabel =
                      source.source_type === 'link'
                        ? t.brew.feedTypeLink
                        : getFeedTypeLabel(source.feed_type, t)
                    const tooltip = sourceInfo.isBrewlia
                      ? `${getFeedTypeLabel(source.feed_type, t)} · ${sourceInfo.tooltip}`
                      : sourceInfo.tooltip
                    return (
                      <span
                        className={`${size === 'full' ? 'text-[12px] px-2.5 py-1 rounded-md' : 'text-[10px] px-1.5 py-0.5 rounded'} font-medium leading-none cursor-help flex items-center gap-1`}
                        style={{ background: `${color}15`, color }}
                        title={tooltip}
                      >
                        {sourceInfo.isBrewlia && (
                          <Sparkles
                            className={`${size === 'full' ? 'w-3 h-3' : 'w-2.5 h-2.5'}`}
                          />
                        )}
                        {sourceInfo.isBrewlia ? 'Brewlia' : feedLabel}
                      </span>
                    )
                  })()}
                  {source.source_type !== 'link' && (
                    <span
                      className={`${size === 'full' ? 'text-[13px]' : 'text-[11px]'} leading-none`}
                    >
                      {hasUnread ? (
                        <>
                          <span style={{ color }} className="font-medium">
                            {source.unread_count}
                          </span>
                          <span className="text-gray-400 dark:text-gray-500">
                            /
                            {t.brew.articlesCount.replace(
                              '{count}',
                              String(source.item_count),
                            )}
                          </span>
                        </>
                      ) : (
                        <span className="text-gray-400 dark:text-gray-500">
                          {t.brew.articlesCount.replace(
                            '{count}',
                            String(source.item_count),
                          )}
                        </span>
                      )}
                    </span>
                  )}
                </div>
              </div>

              {/* AI 风格标签 */}
              {!isEditMode &&
                source.ai_style_tags &&
                source.ai_style_tags.length > 0 && (
                  <div className="flex items-center gap-1 shrink-0 ml-auto">
                    {source.ai_style_tags.slice(0, 2).map((tag, idx) => (
                      <span
                        key={idx}
                        className={`${size === 'full' ? 'text-[12px] px-2.5 py-1 rounded-md' : 'text-[10px] px-1.5 py-0.5 rounded'} font-medium leading-none`}
                        style={{ background: `${color}15`, color }}
                      >
                        {tag}
                      </span>
                    ))}
                  </div>
                )}

              {/* 操作按钮 - 编辑模式 */}
              {isEditMode && (
                <div className="flex items-center gap-0.5 shrink-0">
                  <button
                    onClick={handleEdit}
                    className={`${size === 'tiny' ? 'p-1' : 'p-1.5'} rounded-lg transition-all duration-200 ease-out text-gray-400 hover:text-blue-500 hover:bg-blue-500/10`}
                    title={t.brew.editSubscription}
                    aria-label={t.brew.editSubscription}
                  >
                    <Edit3
                      className={`${size === 'tiny' ? 'w-3.5 h-3.5' : 'w-4 h-4'}`}
                    />
                  </button>
                  {source.source_type !== 'link' && (
                    <button
                      onClick={handleRefresh}
                      className={`${size === 'tiny' ? 'p-1' : 'p-1.5'} rounded-lg transition-all duration-200 ease-out text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-500/10`}
                      title={t.brew.refreshSubscription}
                      aria-label={t.brew.refreshSubscription}
                    >
                      <RefreshCw
                        className={size === 'tiny' ? 'w-3.5 h-3.5' : 'w-4 h-4'}
                      />
                    </button>
                  )}
                  {size === 'full' && source.site_url && (
                    <a
                      href={source.site_url}
                      target="_blank"
                      rel="noopener noreferrer"
                      onClick={(e) => e.stopPropagation()}
                      className="p-1.5 rounded-lg transition-all duration-200 ease-out text-gray-400 hover:text-blue-500 hover:bg-blue-500/10"
                      title={t.brew.visitWebsite}
                    >
                      <ExternalLink className="w-4 h-4" />
                    </a>
                  )}
                </div>
              )}
            </div>

            {/* 文章预览区域 */}
            <div
              className={`flex-1 mt-2 flex flex-col overflow-hidden min-h-0 ${contentTransition} ${
                size === 'tiny'
                  ? 'opacity-0 max-h-0 mt-0 pointer-events-none'
                  : 'opacity-100 max-h-125'
              }`}
            >
              {recentItems.length > 0 ? (
                <>
                  {/* 最新文章预览 */}
                  <div
                    className={`flex-1 ${size === 'mini' ? 'px-3 py-2.5 min-h-11' : 'p-4'} rounded-xl bg-black/3 dark:bg-white/4 flex overflow-hidden ${contentTransition}`}
                  >
                    {size === 'full' && recentItems[0].image && (
                      <div className="w-24 h-full shrink-0 mr-4 rounded-lg overflow-hidden bg-gray-100 dark:bg-neutral-800">
                        <img
                          src={
                            getImageUrl(recentItems[0].image) ||
                            recentItems[0].image
                          }
                          alt=""
                          className="w-full h-full object-cover"
                          loading="lazy"
                          decoding="async"
                          onError={(e) => {
                            ;(
                              e.target as HTMLImageElement
                            ).parentElement!.style.display = 'none'
                          }}
                        />
                      </div>
                    )}
                    <div className="flex items-start gap-2 flex-1 min-w-0 min-h-0">
                      {isAuthenticated && !recentItems[0].is_read && (
                        <span
                          className={`${size === 'full' ? 'w-2.5 h-2.5 mt-1' : 'w-1.5 h-1.5 mt-1.25'} rounded-full shrink-0`}
                          style={{ backgroundColor: color }}
                        />
                      )}
                      <div className="flex-1 min-w-0 flex flex-col h-full overflow-hidden">
                        {size === 'mini' ? (
                          <>
                            <p className="text-[13px] font-semibold text-gray-700 dark:text-gray-300 truncate leading-snug">
                              {recentItems[0].title}
                            </p>
                            {recentItems[0].summary && (
                              <p className="text-[11px] text-gray-500 dark:text-gray-400 line-clamp-2 mt-0.5 leading-snug">
                                {stripHtml(recentItems[0].summary)}
                              </p>
                            )}
                          </>
                        ) : (
                          <>
                            <div className="flex items-center gap-2 shrink-0">
                              <p className="text-[15px] font-semibold text-gray-700 dark:text-gray-300 truncate leading-tight flex-1 min-w-0">
                                {recentItems[0].title}
                              </p>
                              <span className="text-[11px] text-gray-400 dark:text-gray-500 shrink-0">
                                {formatTime(recentItems[0].published_at, t)}
                              </span>
                            </div>
                            {recentItems[0].summary && (
                              <p
                                className={`text-[13px] text-gray-500 dark:text-gray-400 ${recentItems[0].image ? 'line-clamp-2' : 'line-clamp-3'} mt-1 leading-relaxed overflow-hidden`}
                              >
                                {stripHtml(recentItems[0].summary)}
                              </p>
                            )}
                          </>
                        )}
                      </div>
                    </div>
                  </div>

                  {/* 次新文章列表 */}
                  <div
                    className={`flex flex-col justify-evenly overflow-hidden ${contentTransition} ${
                      size === 'full'
                        ? 'flex-1 min-h-0 mt-2.5 opacity-100 max-h-50'
                        : 'flex-none h-0 mt-0 opacity-0 pointer-events-none'
                    }`}
                  >
                    {recentItems.slice(1, 4).map((item, idx) => (
                      <div
                        key={item.id || idx}
                        className={`flex items-center gap-2.5 px-3 py-2 rounded-lg transition-colors ${
                          !isEditMode
                            ? 'hover:bg-black/2 dark:hover:bg-white/3'
                            : ''
                        }`}
                      >
                        {isAuthenticated && !item.is_read && (
                          <span
                            className="w-2 h-2 rounded-full shrink-0"
                            style={{ backgroundColor: color, opacity: 0.7 }}
                          />
                        )}
                        <span className="text-[14px] text-gray-600 dark:text-gray-400 truncate flex-1">
                          {item.title}
                        </span>
                        <span className="text-[12px] text-gray-400 dark:text-gray-500 shrink-0">
                          {formatTime(item.published_at, t)}
                        </span>
                      </div>
                    ))}
                  </div>
                </>
              ) : (
                <div
                  className={`flex-1 ${size === 'mini' ? 'p-3' : 'p-4'} rounded-xl bg-black/3 dark:bg-white/4 flex items-start overflow-hidden`}
                >
                  {source.description && size === 'full' ? (
                    <p className="text-[14px] text-gray-500 dark:text-gray-400 line-clamp-6 leading-relaxed">
                      {source.description}
                    </p>
                  ) : (
                    <div className="flex items-center gap-2 text-[13px] text-gray-400 dark:text-gray-500">
                      <Rss className="w-4 h-4" />
                      <span>{t.brew.noArticles}</span>
                    </div>
                  )}
                </div>
              )}
            </div>
          </div>

          {/* 编辑模式 - 右下角拉伸条 */}
          {isEditMode && source.source_type !== 'link' && (
            <div
              className={`absolute bottom-0 right-0 ${size === 'tiny' ? 'w-10 h-10 p-1.5' : size === 'mini' ? 'w-12 h-12 p-2' : 'w-14 h-14 p-2.5'} cursor-se-resize z-50 flex items-end justify-end touch-none group/resize`}
              onMouseDown={handleResizeStart}
              onTouchStart={handleResizeStart}
            >
              <div
                className={`${size === 'tiny' ? 'w-4 h-4 border-b-4 border-r-4 rounded-br-lg' : size === 'mini' ? 'w-5 h-5 border-b-5 border-r-5 rounded-br-lg' : 'w-6 h-6 border-b-6 border-r-6 rounded-br-xl'} opacity-50 group-hover/resize:opacity-100 transition-opacity duration-200`}
                style={{ borderColor: color }}
              />
            </div>
          )}

          {/* 边框 */}
          <div
            className={`absolute inset-0 ${sizeClasses.rounded} ring-1 ring-inset ring-black/5 dark:ring-white/10 pointer-events-none`}
          />

          {/* 悬浮高光边框 */}
          {isHovered && (
            <div
              className={`absolute inset-0 ${sizeClasses.rounded} pointer-events-none`}
              style={{ boxShadow: `inset 0 0 0 1px ${color}40` }}
            />
          )}
        </div>
      )
    },
  ),
  (prevProps, nextProps) => {
    const prevItems = prevProps.source.recent_items
    const nextItems = nextProps.source.recent_items
    const recentItemsEqual =
      (prevItems?.length ?? 0) === (nextItems?.length ?? 0) &&
      (prevItems?.every(
        (item, i) =>
          item.id === nextItems?.[i]?.id &&
          item.is_read === nextItems?.[i]?.is_read,
      ) ??
        true)

    return (
      prevProps.source.id === nextProps.source.id &&
      prevProps.source.card_size === nextProps.source.card_size &&
      prevProps.source.theme_color === nextProps.source.theme_color &&
      prevProps.source.unread_count === nextProps.source.unread_count &&
      prevProps.source.item_count === nextProps.source.item_count &&
      prevProps.source.name === nextProps.source.name &&
      prevProps.index === nextProps.index &&
      prevProps.isEditMode === nextProps.isEditMode &&
      prevProps.isSelected === nextProps.isSelected &&
      prevProps.isDeleting === nextProps.isDeleting &&
      prevProps.isDragging === nextProps.isDragging &&
      prevProps.isDragOver === nextProps.isDragOver &&
      prevProps.sortMode === nextProps.sortMode &&
      prevProps.isAuthenticated === nextProps.isAuthenticated &&
      prevProps.previewSize === nextProps.previewSize &&
      prevProps.onSourceClick === nextProps.onSourceClick &&
      recentItemsEqual
    )
  },
)

SourceCard.displayName = 'SourceCard'
