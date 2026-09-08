/**
 * 阅读器正文列：标题、元信息、封面、prose、音频、上下篇
 */

import type { BrewItem } from '../../../types/brew'
import type { FontOption, LayoutOption, ThemeConfig } from './types'
import {
  LuCalendar as Calendar,
  LuClock as Clock,
  LuUser as User,
} from '@lib/icons'
import { useMemo } from 'react'
import { useReadingListOptional } from '../../../contexts/ReadingListContext'
import { getArticleProseClass } from './articleProseClass'
import { getImageUrl } from './contentRender'

export interface ReaderArticleBodyProps {
  item: BrewItem
  currentTheme: ThemeConfig
  currentFont: FontOption
  currentLayout: LayoutOption
  isDark: boolean
  fontSize: number
  lineHeight: number
  t: Record<string, any>
  contentRef: React.RefObject<HTMLDivElement | null>
  contentInnerRef: React.RefObject<HTMLDivElement | null>
  /**
   * 中栏根节点。换文章的淡入淡出作用在这一层，两侧面板不跟着动。
   * 由 BrewReader 持有 —— 动画的时序和 `item` 的延迟切换是同一件事。
   */
  columnRef?: React.RefObject<HTMLDivElement | null>
  contentReady: boolean
  onNavigateToArticle?: (articleId: number) => void
  articleList?: BrewItem[]
  currentArticleIndex?: number
}

export function ReaderArticleBody({
  item,
  currentTheme,
  currentFont,
  currentLayout,
  isDark,
  fontSize,
  lineHeight,
  t,
  contentRef,
  contentInnerRef,
  columnRef,
  contentReady,
  onNavigateToArticle,
  articleList,
  currentArticleIndex,
}: ReaderArticleBodyProps) {
  const readingList = useReadingListOptional()
  const positionInfo = readingList?.getPositionInfo(item.id)

  const formattedDate = useMemo(() => {
    if (!item.published_at) return ''
    return new Date(item.published_at).toLocaleDateString('zh-CN', {
      year: 'numeric',
      month: 'long',
      day: 'numeric',
    })
  }, [item.published_at])

  return (
          <div
            ref={columnRef}
            className={`w-full ${currentLayout.width} px-6 pt-32 pb-16 transition-all duration-300`}
          >
            {/* 标题 */}
            <h1
              className={`font-bold ${currentTheme.text} leading-tight mb-6`}
              style={{ fontFamily: currentFont.family, fontSize: 40 }}
            >
              {item.title}
            </h1>

            {/* 元信息 */}
            <div
              className={`flex flex-wrap items-center gap-4 text-sm ${currentTheme.secondary} mb-8 pb-8 border-b ${currentTheme.border}`}
            >
              <span className="flex items-center gap-1.5">
                {item.source_icon && (
                  <img
                    src={item.source_icon}
                    alt=""
                    className="w-4 h-4 rounded"
                  />
                )}
                {item.source_name}
              </span>
              {item.author && (
                <span className="flex items-center gap-1.5">
                  <User className="w-4 h-4" />
                  {item.author}
                </span>
              )}
              {item.published_at && (
                <span className="flex items-center gap-1.5">
                  <Calendar className="w-4 h-4" />
                  {formattedDate}
                </span>
              )}
              {item.reading_time && (
                <span className="flex items-center gap-1.5">
                  <Clock className="w-4 h-4" />
                  {t.brew.readingTime.replace(
                    '{time}',
                    String(item.reading_time),
                  )}
                </span>
              )}
              {item.word_count && (
                <span>
                  {item.word_count.toLocaleString()} {t.brew.wordCount}
                </span>
              )}
            </div>

            {/* 封面图 */}
            {item.image && (
              <div className="mb-8">
                <img
                  src={getImageUrl(item.image) || ''}
                  alt=""
                  className="w-full rounded-2xl"
                  loading="lazy"
                />
              </div>
            )}

            {/* 正文内容 */}
            <div
              ref={contentRef}
              className={getArticleProseClass(isDark, currentTheme.text)}
              style={{
                fontSize: `${fontSize}px`,
                lineHeight,
                fontFamily: currentFont.family,
              }}
              onClick={(e) => e.stopPropagation()}
            >
              {/* WebKit 优化：动画期间显示简单占位，避免同时渲染大量 DOM */}
              {contentReady ? (
                <div ref={contentInnerRef} />
              ) : (
                <div className="space-y-4 animate-pulse">
                  <div
                    className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`}
                    style={{ width: '90%' }}
                  />
                  <div
                    className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`}
                    style={{ width: '100%' }}
                  />
                  <div
                    className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`}
                    style={{ width: '85%' }}
                  />
                  <div
                    className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`}
                    style={{ width: '95%' }}
                  />
                  <div
                    className={`h-4 rounded ${isDark ? 'bg-white/10' : 'bg-black/5'}`}
                    style={{ width: '70%' }}
                  />
                </div>
              )}
            </div>

            {/* 音频播放器 */}
            {item.audio_url && (
              <div
                className={`mt-8 p-4 rounded-2xl ${currentTheme.surfaceSolid} border ${currentTheme.border}`}
                onClick={(e) => e.stopPropagation()}
              >
                <p className={`text-sm ${currentTheme.secondary} mb-3`}>
                  {t.brew.audioLabel}
                </p>
                <audio src={item.audio_url} controls className="w-full" />
              </div>
            )}

            {/* 文章导航 - 上一篇/下一篇 */}
            {(() => {
              // 阅读列表导航
              if (positionInfo && onNavigateToArticle) {
                const prevItem = readingList?.getPrevious()
                const nextItem = readingList?.getNext()
                return (
                  <div
                    className={`mt-12 pt-8 border-t ${currentTheme.border} max-w-2xl mx-auto`}
                    onClick={(e) => e.stopPropagation()}
                  >
                    <div
                      className={`text-center mb-6 ${currentTheme.secondary}`}
                    >
                      <span className="text-sm">
                        {readingList?.currentList?.name} ·
                        {positionInfo.index + 1} /{positionInfo.total}
                      </span>
                    </div>
                    <div className="flex gap-4">
                      <button
                        onClick={() => {
                          if (prevItem) {
                            readingList?.goToArticle(positionInfo.index - 1)
                            onNavigateToArticle(prevItem.id)
                          }
                        }}
                        disabled={!positionInfo.hasPrev}
                        className={`flex-1 min-w-0 p-4 rounded-2xl text-left transition-all ${positionInfo.hasPrev ? `${currentTheme.surface} hover:opacity-80 cursor-pointer` : 'opacity-30 cursor-not-allowed'} border ${currentTheme.border}`}
                      >
                        <div
                          className={`text-xs ${currentTheme.secondary} mb-1 flex items-center gap-1`}
                        >
                          <svg
                            className="w-3 h-3 shrink-0"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                          >
                            <path
                              strokeLinecap="round"
                              strokeLinejoin="round"
                              strokeWidth={2}
                              d="M15 19l-7-7 7-7"
                            />
                          </svg>
                          {t.brew.prevArticle}
                        </div>
                        <div
                          className={`${currentTheme.text} font-medium truncate`}
                        >
                          {prevItem?.title || t.brew.noMore}
                        </div>
                      </button>
                      <button
                        onClick={() => {
                          if (nextItem) {
                            readingList?.goToArticle(positionInfo.index + 1)
                            onNavigateToArticle(nextItem.id)
                          }
                        }}
                        disabled={!positionInfo.hasNext}
                        className={`flex-1 min-w-0 p-4 rounded-2xl text-right transition-all ${positionInfo.hasNext ? `${currentTheme.surface} hover:opacity-80 cursor-pointer` : 'opacity-30 cursor-not-allowed'} border ${currentTheme.border}`}
                      >
                        <div
                          className={`text-xs ${currentTheme.secondary} mb-1 flex items-center justify-end gap-1`}
                        >
                          {t.brew.nextArticle}
                          <svg
                            className="w-3 h-3 shrink-0"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                          >
                            <path
                              strokeLinecap="round"
                              strokeLinejoin="round"
                              strokeWidth={2}
                              d="M9 5l7 7-7 7"
                            />
                          </svg>
                        </div>
                        <div
                          className={`${currentTheme.text} font-medium truncate`}
                        >
                          {nextItem?.title || t.brew.noMore}
                        </div>
                      </button>
                    </div>
                  </div>
                )
              }
              // 全局文章列表导航
              if (
                articleList &&
                currentArticleIndex != null &&
                onNavigateToArticle
              ) {
                const hasPrev = currentArticleIndex > 0
                const hasNext = currentArticleIndex < articleList.length - 1
                const prevArticle = hasPrev
                  ? articleList[currentArticleIndex - 1]
                  : null
                const nextArticle = hasNext
                  ? articleList[currentArticleIndex + 1]
                  : null
                return (
                  <div
                    className={`mt-12 pt-8 border-t ${currentTheme.border} max-w-2xl mx-auto`}
                    onClick={(e) => e.stopPropagation()}
                  >
                    <div className="flex gap-4">
                      <button
                        onClick={() => {
                          if (prevArticle) onNavigateToArticle(prevArticle.id)
                        }}
                        disabled={!hasPrev}
                        className={`flex-1 min-w-0 p-4 rounded-2xl text-left transition-all ${hasPrev ? `${currentTheme.surface} hover:opacity-80 cursor-pointer` : 'opacity-30 cursor-not-allowed'} border ${currentTheme.border}`}
                      >
                        <div
                          className={`text-xs ${currentTheme.secondary} mb-1 flex items-center gap-1`}
                        >
                          <svg
                            className="w-3 h-3 shrink-0"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                          >
                            <path
                              strokeLinecap="round"
                              strokeLinejoin="round"
                              strokeWidth={2}
                              d="M15 19l-7-7 7-7"
                            />
                          </svg>
                          {t.brew.prevArticle}
                        </div>
                        <div
                          className={`${currentTheme.text} font-medium truncate`}
                        >
                          {prevArticle?.title || t.brew.noMore}
                        </div>
                      </button>
                      <button
                        onClick={() => {
                          if (nextArticle) onNavigateToArticle(nextArticle.id)
                        }}
                        disabled={!hasNext}
                        className={`flex-1 min-w-0 p-4 rounded-2xl text-right transition-all ${hasNext ? `${currentTheme.surface} hover:opacity-80 cursor-pointer` : 'opacity-30 cursor-not-allowed'} border ${currentTheme.border}`}
                      >
                        <div
                          className={`text-xs ${currentTheme.secondary} mb-1 flex items-center justify-end gap-1`}
                        >
                          {t.brew.nextArticle}
                          <svg
                            className="w-3 h-3 shrink-0"
                            fill="none"
                            stroke="currentColor"
                            viewBox="0 0 24 24"
                          >
                            <path
                              strokeLinecap="round"
                              strokeLinejoin="round"
                              strokeWidth={2}
                              d="M9 5l7 7-7 7"
                            />
                          </svg>
                        </div>
                        <div
                          className={`${currentTheme.text} font-medium truncate`}
                        >
                          {nextArticle?.title || t.brew.noMore}
                        </div>
                      </button>
                    </div>
                  </div>
                )
              }
              return null
            })()}

            {/* 底部留白 */}
            <div className="h-20" />
          </div>
  )
}
