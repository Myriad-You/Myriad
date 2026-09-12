import type { BrewItem } from '../../../types/brew'
import type { ReadingQueue } from '../logic/readingQueue'
import type { FontOption, LayoutOption, ReaderCopy, ThemeConfig } from './types'
import {
  LuCalendar as Calendar,
  LuClock as Clock,
  LuUser as User,
} from '@lib/icons'
import { useMemo } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { neighborsInQueue } from '../logic/readingQueue'
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
  t: ReaderCopy
  contentRef: React.RefObject<HTMLDivElement | null>
  contentInnerRef: React.RefObject<HTMLDivElement | null>
  /** 换文章淡入淡出只作用这一层，两侧面板不跟着动。 */
  columnRef?: React.RefObject<HTMLDivElement | null>
  contentReady: boolean
  onNavigateToArticle?: (articleId: number) => void
  readingQueue?: ReadingQueue | null
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
  readingQueue,
}: ReaderArticleBodyProps) {
  const { locale, format } = useI18n()
  const queueNav = neighborsInQueue(readingQueue, item.id)

  const formattedDate = useMemo(() => {
    if (!item.published_at) return ''
    return new Date(item.published_at).toLocaleDateString(locale, {
      year: 'numeric',
      month: 'long',
      day: 'numeric',
    })
  }, [item.published_at, locale])

  return (
          <div
            ref={columnRef}
            className={`w-full ${currentLayout.width} px-6 pt-32 pb-16 transition-all duration-300`}
          >
            <h1
              className={`font-bold ${currentTheme.text} leading-tight mb-6`}
              style={{ fontFamily: currentFont.family, fontSize: 40 }}
            >
              {item.title}
            </h1>

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
                  {format(t.brew.readingTime, { time: item.reading_time })}
                </span>
              )}
              {item.word_count && (
                <span>
                  {item.word_count.toLocaleString(locale)} {t.brew.wordCount}
                </span>
              )}
            </div>

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
              {/* WebKit：动画期间占位，避免同时灌大量 DOM。 */}
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

            {queueNav && onNavigateToArticle ? (
                  <div
                    className={`mt-12 pt-8 border-t ${currentTheme.border} max-w-2xl mx-auto`}
                    onClick={(e) => e.stopPropagation()}
                  >
                    {queueNav.name ? (
                    <div
                      className={`text-center mb-6 ${currentTheme.secondary}`}
                    >
                      <span className="text-sm">
                        {queueNav.name} ·
                        {queueNav.index + 1} /{queueNav.total}
                      </span>
                    </div>
                    ) : null}
                    <div className="flex gap-4">
                      <button
                        onClick={() => {
                          if (queueNav.prev) onNavigateToArticle(queueNav.prev.id)
                        }}
                        disabled={!queueNav.prev}
                        className={`flex-1 min-w-0 p-4 rounded-2xl text-left transition-all ${queueNav.prev ? `${currentTheme.surface} hover:opacity-80 cursor-pointer` : 'opacity-30 cursor-not-allowed'} border ${currentTheme.border}`}
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
                          {queueNav.prev?.title || t.brew.noMore}
                        </div>
                      </button>
                      <button
                        onClick={() => {
                          if (queueNav.next) onNavigateToArticle(queueNav.next.id)
                        }}
                        disabled={!queueNav.next}
                        className={`flex-1 min-w-0 p-4 rounded-2xl text-right transition-all ${queueNav.next ? `${currentTheme.surface} hover:opacity-80 cursor-pointer` : 'opacity-30 cursor-not-allowed'} border ${currentTheme.border}`}
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
                          {queueNav.next?.title || t.brew.noMore}
                        </div>
                      </button>
                    </div>
                  </div>
            ) : null}

            <div className="h-20" />
          </div>
  )
}
