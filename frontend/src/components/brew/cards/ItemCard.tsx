/**
 * 文章条目卡片组件
 * 从 BrewFeedList 提取，独立维护
 *
 * 性能优化：
 * - React.memo + 自定义比较函数
 * - useCallback 缓存回调
 * - useMemo 缓存计算
 */

import type { ItemCardProps, TimeTranslations } from '../types'
import {
  LuExternalLink as ExternalLink,
  LuMic as Mic,
  LuSparkles as Sparkles,
  LuStar as Star,
} from '@lib/icons'
import React, { useCallback, useMemo, useState } from 'react'
import { useBrewCardStagger } from '../../../hooks/animation/pages/brew'
import {
  getFullPlainText,
  getIconUrl,
  getImageUrl,
  getPlainText,
  SHORT_CONTENT_THRESHOLD,
} from '../constants'

// 格式化时间
function formatTime(timestamp: number | null, translations: TimeTranslations, locale: string) {
  if (!timestamp)
    return ''
  const date = new Date(timestamp)
  const now = new Date()
  const diff = now.getTime() - date.getTime()

  if (diff < 60000)
    return translations.justNow
  if (diff < 3600000)
    return translations.minutesAgo.replace('{minutes}', String(Math.floor(diff / 60000)))
  if (diff < 86400000)
    return translations.hoursAgo.replace('{hours}', String(Math.floor(diff / 3600000)))
  if (diff < 604800000)
    return translations.daysAgo.replace('{days}', String(Math.floor(diff / 86400000)))

  const dateLocale = locale === 'zh-CN' ? 'zh-CN' : locale === 'ja-JP' ? 'ja-JP' : 'en-US'
  return date.toLocaleDateString(dateLocale, { month: 'short', day: 'numeric' })
}

export const ItemCard = React.memo<ItemCardProps>(({
  item,
  index,
  isSelected,
  isLast,
  themeColor,
  onItemSelect,
  onToggleStar,
  lastItemRef,
  editMode,
  isChecked,
  onToggleCheck,
  isAuthenticated = false,
  timeTranslations,
  brewTranslations,
  locale,
}) => {
  const [isHovered, setIsHovered] = useState(false)

  // 接入动画调度器
  const { canAnimate, animateStyle, animConfig } = useBrewCardStagger(index, 'item')
  const enableHover = animConfig.level !== 'none'

  // 缓存摘要文本和完整文本
  const summaryText = useMemo(() => getPlainText(item.summary), [item.summary])
  const fullText = useMemo(() => getFullPlainText(item.content || item.summary), [item.content, item.summary])

  // 判断是否为短文
  const isShortContent = useMemo(() => {
    return fullText.length > 0 && fullText.length < SHORT_CONTENT_THRESHOLD && !item.image
  }, [fullText, item.image])

  // 点击处理
  const handleClick = useCallback(() => {
    if (editMode && onToggleCheck) {
      onToggleCheck(item.id)
    }
    else if (!isShortContent) {
      onItemSelect(item)
    }
  }, [editMode, onToggleCheck, onItemSelect, item, isShortContent])

  const handleStarClick = useCallback((e: React.MouseEvent) => {
    e.stopPropagation()
    onToggleStar(item)
  }, [onToggleStar, item])

  // 图片加载错误处理
  const handleImageError = useCallback((e: React.SyntheticEvent<HTMLImageElement>) => {
    const parent = e.currentTarget.parentElement?.parentElement
    if (parent)
      parent.style.display = 'none'
  }, [])

  const handleIconError = useCallback((e: React.SyntheticEvent<HTMLImageElement>) => {
    e.currentTarget.style.display = 'none'
  }, [])

  // 样式类名
  const hoverClass = enableHover && !isShortContent ? 'hover:-translate-y-px' : ''
  const cursorClass = isShortContent && !editMode ? 'cursor-default' : 'cursor-pointer'

  return (
    <div
      ref={isLast ? lastItemRef : undefined}
      onClick={handleClick}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      className={`group relative rounded-2xl overflow-hidden bg-white/90 dark:bg-neutral-900/90 ${cursorClass} ${
        isSelected ? 'ring-2 ring-blue-500' : ''
      } ${editMode && isChecked ? 'ring-2 ring-amber-500' : ''} ${item.is_read ? 'opacity-60' : ''} ${hoverClass}`}
      style={animateStyle}
    >
      {/* 编辑模式复选框 */}
      {editMode && (
        <div className="absolute top-3 left-3 z-20">
          <div className={`w-5 h-5 rounded-md border-2 flex items-center justify-center ${
            isChecked
              ? 'bg-amber-500 border-amber-500'
              : 'bg-white/80 dark:bg-neutral-800/80 border-gray-300 dark:border-neutral-600'
          }`}
          >
            {isChecked && (
              <svg className="w-3.5 h-3.5 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={3} d="M5 13l4 4L19 7" />
              </svg>
            )}
          </div>
        </div>
      )}

      {/* 装饰光效 */}
      <div
        className={`absolute -right-8 -top-8 w-32 h-32 rounded-full blur-3xl ${isHovered ? 'opacity-20' : 'opacity-10'}`}
        style={{ background: `linear-gradient(135deg, ${themeColor}, transparent 70%)` }}
      />

      {/* 边框高光效果 */}
      {isHovered && (
        <div
          className="absolute inset-0 rounded-2xl pointer-events-none"
          style={{ boxShadow: `inset 0 0 0 1px ${themeColor}30` }}
        />
      )}

      {/* 封面图 */}
      {item.image && (
        <div className="px-6 pt-6 relative">
          <div className="aspect-[7/2] overflow-hidden rounded-xl bg-gray-100 dark:bg-neutral-800">
            <img
              src={getImageUrl(item.image) || ''}
              alt=""
              className="w-full h-full object-cover"
              loading="lazy"
              decoding="async"
              onError={handleImageError}
            />
          </div>
          {!item.is_read && (
            <div
              className="absolute top-7 right-7 w-2.5 h-2.5 rounded-full"
              style={{ backgroundColor: themeColor }}
            />
          )}
        </div>
      )}

      {/* 未读标记 - 无封面 */}
      {!item.is_read && !item.image && (
        <div
          className="absolute top-5 right-5 w-2.5 h-2.5 rounded-full"
          style={{ backgroundColor: themeColor }}
        />
      )}

      {/* 内容区域 */}
      <div className="relative z-10 p-6">
        {/* 来源栏 */}
        <div className="flex items-center gap-2 mb-4">
          <div
            className="flex items-center gap-2 px-2.5 py-1 rounded-full"
            style={{ backgroundColor: `${themeColor}15` }}
          >
            {item.source_icon && (
              <img
                src={getIconUrl(item.source_icon) || ''}
                alt=""
                className="w-4 h-4 rounded-full flex-shrink-0"
                loading="lazy"
                onError={handleIconError}
              />
            )}
            <span
              className="text-xs font-medium"
              style={{ color: themeColor }}
            >
              {item.source_name}
            </span>
          </div>

          <span className="text-xs text-gray-400 dark:text-gray-500">
            {formatTime(item.published_at, timeTranslations, locale)}
          </span>

          {item.reading_time && (
            <span className="text-xs text-gray-400 dark:text-gray-500 flex items-center gap-1">
              <svg className="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              {item.reading_time}
              min
            </span>
          )}

          {/* AI 功能标记 */}
          {(item.has_ai_annotations || item.has_ai_podcast) && (
            <div className="flex items-center gap-1.5 ml-auto">
              {item.has_ai_annotations && (
                <span
                  className="text-xs px-2 py-1 rounded-full bg-purple-500/15 text-purple-600 dark:text-purple-400 inline-flex items-center gap-1 font-medium"
                  title={brewTranslations.hasAnnotations}
                >
                  <Sparkles className="w-3.5 h-3.5 shrink-0" />
                  {brewTranslations.annotationsLabel}
                </span>
              )}
              {item.has_ai_podcast && (
                <span
                  className="text-xs px-2 py-1 rounded-full bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 inline-flex items-center gap-1 font-medium"
                  title={brewTranslations.hasPodcast}
                >
                  <Mic className="w-3.5 h-3.5 shrink-0" />
                  {brewTranslations.podcastLabel}
                </span>
              )}
            </div>
          )}
        </div>

        {/* 标题区 */}
        <div className="flex items-start gap-3">
          <h4 className={`flex-1 font-bold line-clamp-2 leading-snug text-xl tracking-tight ${
            item.is_read ? 'text-gray-500 dark:text-gray-400' : 'text-gray-900 dark:text-gray-50'
          }`}
          >
            {item.title}
          </h4>

          {/* 操作按钮 */}
          <div className={`flex items-center gap-0.5 flex-shrink-0 transition-opacity duration-300 ease-out ${isHovered ? 'opacity-100' : 'opacity-0'}`}>
            {isAuthenticated && (
              <button
                onClick={handleStarClick}
                className={`p-1.5 rounded-lg transition-colors ${
                  item.is_starred ? 'text-amber-500' : 'text-gray-400 hover:text-amber-500 hover:bg-gray-100 dark:hover:bg-gray-800'
                }`}
                title={item.is_starred ? brewTranslations.unstarArticle : brewTranslations.starArticle}
                aria-label={item.is_starred ? brewTranslations.unstarArticle : brewTranslations.starArticle}
              >
                <Star className={`w-4 h-4 ${item.is_starred ? 'fill-current' : ''}`} />
              </button>
            )}
            <a
              href={item.link}
              target="_blank"
              rel="noopener noreferrer"
              onClick={e => e.stopPropagation()}
              className="p-1.5 rounded-lg text-gray-400 hover:text-blue-500 hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors"
              title={brewTranslations.openInNewTab}
              aria-label={brewTranslations.openInNewTab}
            >
              <ExternalLink className="w-4 h-4" />
            </a>
          </div>
        </div>

        {/* 内容区 */}
        {isShortContent
          ? (
              <p className="mt-3 text-[15px] text-gray-600 dark:text-gray-300 leading-[1.75] whitespace-pre-wrap">
                {fullText}
              </p>
            )
          : summaryText && (
            <p className="mt-3 text-[15px] text-gray-500 dark:text-gray-400 line-clamp-3 leading-[1.7]">
              {summaryText}
            </p>
          )}
      </div>

      {/* 边框 */}
      <div className="absolute inset-0 rounded-2xl ring-1 ring-inset ring-black/[0.04] dark:ring-white/[0.06] pointer-events-none" />

      {/* 悬浮高光 */}
      {isHovered && (
        <div
          className="absolute inset-0 rounded-2xl pointer-events-none"
          style={{ boxShadow: `inset 0 0 0 1.5px ${themeColor}45` }}
        />
      )}
    </div>
  )
}, (prevProps, nextProps) => {
  return (
    prevProps.item.id === nextProps.item.id
    && prevProps.item.is_read === nextProps.item.is_read
    && prevProps.item.is_starred === nextProps.item.is_starred
    && prevProps.item.has_ai_annotations === nextProps.item.has_ai_annotations
    && prevProps.item.has_ai_podcast === nextProps.item.has_ai_podcast
    && prevProps.isSelected === nextProps.isSelected
    && prevProps.isLast === nextProps.isLast
    && prevProps.themeColor === nextProps.themeColor
    && prevProps.index === nextProps.index
    && prevProps.editMode === nextProps.editMode
    && prevProps.isChecked === nextProps.isChecked
    && prevProps.isAuthenticated === nextProps.isAuthenticated
    && prevProps.locale === nextProps.locale
  )
})

ItemCard.displayName = 'ItemCard'
