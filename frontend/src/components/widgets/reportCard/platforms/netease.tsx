import type { CSSProperties } from 'react'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import {
  pairPageCount,
  REPORT_PAIR_DWELL_MS,
  useReportDetailPage,
} from '../reportPaging'
import '../ambient.css'

export const MusicStatsWidget = memo(
  ({
    data,
    allowLoop = true,
  }: {
    data?: any
    allowLoop?: boolean
  }) => {
    const { t } = useI18n()

    const moodKeywords = useMemo(
      () => data?.mood_keywords || [],
      [data?.mood_keywords],
    )
    const followerCount = useMemo(
      () => data?.follower_count || 0,
      [data?.follower_count],
    )
    const playlistCount = useMemo(
      () => data?.playlist_count || 0,
      [data?.playlist_count],
    )
    const level = useMemo(() => data?.level || 0, [data?.level])

    const formatNumber = (num: number) => {
      if (num >= 10000)
        return `${(num / 10000).toFixed(1)}${t.reportsPage.tenThousandSuffix}`
      if (num >= 1000) return `${(num / 1000).toFixed(1)}k`
      return num.toString()
    }

    const bubbles = useMemo(() => {
      const items: Array<{
        tag: string
        color: string
        x: number
        y: number
        size: number
        floatDuration: number
        floatDelay: number
      }> = []
      const hash = (str: string, seed: number) => {
        let h = seed
        for (let j = 0; j < str.length; j++) {
          h = Math.imul(h ^ str.charCodeAt(j), 2654435761)
        }
        return ((h ^ (h >>> 16)) >>> 0) / 4294967296
      }
      const isOverlappingStats = (x: number, y: number) => x > 60 && y > 60

      moodKeywords.forEach((keyword: any, i: number) => {
        const size = 35 + Math.floor(hash(keyword.tag, 1) * 60)
        let bestX = 50
        let bestY = 50
        let maxMinDist = -1

        for (let attempt = 0; attempt < 30; attempt++) {
          const r1 = hash(keyword.tag, 100 + attempt + i * 50)
          const r2 = hash(keyword.tag, 200 + attempt + i * 50)
          const x = 10 + r1 * 80
          const y = 10 + r2 * 80
          if (isOverlappingStats(x, y)) continue

          let minDist = 1000
          if (items.length > 0) {
            for (const item of items) {
              const dx = x - item.x
              const dy = (y - item.y) * 2
              const d = Math.sqrt(dx * dx + dy * dy)
              if (d < minDist) minDist = d
            }
          }
          if (minDist > maxMinDist) {
            maxMinDist = minDist
            bestX = x
            bestY = y
          }
        }

        items.push({
          tag: keyword.tag,
          color: keyword.color,
          x: bestX,
          y: bestY,
          size,
          floatDuration: 3 + hash(keyword.tag, 4) * 4,
          floatDelay: hash(keyword.tag, 5) * 2,
        })
      })
      return items
    }, [moodKeywords])

    return (
      <div className="relative h-full w-full overflow-hidden">
        <div className="absolute inset-0 bg-linear-to-br from-red-50/50 to-transparent dark:from-red-900/20 dark:to-transparent" />
        <div className="relative h-full w-full p-3">
          <div className="absolute inset-0 pointer-events-none">
            {bubbles.map((bubble, i) => (
              // 外层只管入场和悬停放大；外观与上下浮动在内层，用 CSS 一直浮（见 ambient.css）。
              <motion.div
                key={bubble.tag}
                className="report-mood-bubble absolute font-bold pointer-events-auto cursor-default"
                style={
                  {
                    'left': `${bubble.x}%`,
                    'top': `${bubble.y}%`,
                    'width': `${bubble.size}px`,
                    'height': `${bubble.size}px`,
                    'marginLeft': `-${bubble.size / 2}px`,
                    'marginTop': `-${bubble.size / 2}px`,
                    'fontSize': `${Math.min(Math.max(10, bubble.size / 4), 16)}px`,
                    'zIndex': 10,
                    '--bubble-color': bubble.color,
                  } as CSSProperties
                }
                initial={{ scale: 0, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                transition={{
                  scale: {
                    type: 'spring',
                    stiffness: 260,
                    damping: 20,
                    delay: i * 0.1,
                  },
                  opacity: { duration: 0.6, delay: i * 0.1 },
                }}
                whileHover={{
                  scale: 1.15,
                  zIndex: 50,
                  transition: { duration: 0.3, ease: 'easeOut' },
                }}
              >
                <div
                  className={`report-mood-bubble__body${allowLoop ? ' is-floating' : ''}`}
                  style={
                    {
                      '--float-duration': `${bubble.floatDuration}s`,
                      '--float-delay': `${bubble.floatDelay}s`,
                    } as CSSProperties
                  }
                >
                  <div className="absolute top-[15%] left-[15%] w-[20%] h-[10%] bg-white/30 dark:bg-white/15 rounded-full transform -rotate-45" />
                  <span className="relative z-10 mix-blend-multiply dark:mix-blend-normal">
                    {bubble.tag}
                  </span>
                </div>
              </motion.div>
            ))}
          </div>
          <div className="absolute bottom-3 right-3 flex flex-col items-end gap-2 z-20">
            <motion.div
              className="px-2.5 py-0.5 rounded-full text-[9px] font-bold flex items-center gap-1 shadow-lg bg-linear-to-br from-red-50 to-red-100 dark:from-red-950/80 dark:to-red-900/60 text-red-600 dark:text-red-300"
              style={{ boxShadow: '0 2px 12px rgba(239, 68, 68, 0.25)' }}
              initial={{ scale: 0.8, opacity: 0, x: 20 }}
              animate={{ scale: 1, opacity: 1, x: 0 }}
              transition={{ duration: 0.4, delay: 0.1 }}
            >
              <span className="text-[7px]">●</span>
              <span>
                Lv.
                {level}
              </span>
            </motion.div>
            <motion.div
              className="flex items-center gap-2 px-3 py-1.5 rounded-lg glass-surface glass-90 shadow-lg border border-white/30 dark:border-white/10"
              initial={{ scale: 0.8, opacity: 0, x: 20 }}
              animate={{ scale: 1, opacity: 1, x: 0 }}
              transition={{ duration: 0.4, delay: 0.2 }}
            >
              <div className="flex flex-col items-end">
                <span className="text-lg font-black leading-none text-gray-900 dark:text-gray-100">
                  {formatNumber(followerCount)}
                </span>
                <span className="text-[9px] tracking-wide mt-0.5 italic font-semibold text-gray-600 dark:text-gray-400 font-georgia">
                  {t.reportsPage.fans}
                </span>
              </div>
              <div className="w-px h-5 bg-gray-300 dark:bg-white/20" />
              <div className="flex flex-col items-end">
                <span className="text-lg font-black leading-none text-gray-900 dark:text-gray-100">
                  {formatNumber(playlistCount)}
                </span>
                <span className="text-[9px] tracking-wide mt-0.5 italic font-semibold text-gray-600 dark:text-gray-400 font-georgia">
                  {t.reportsPage.lists}
                </span>
              </div>
            </motion.div>
          </div>
        </div>
      </div>
    )
  },
)

export const NeteaseWidget = memo(
  ({ data, showOverview, onContentChange, allowLoop = true }: any) => {
    const processedData = useMemo(() => {
      if (!data) return undefined
      let moodKeywords: Array<{ tag: string; color: string }> = []
      if (data.mood_keywords && Array.isArray(data.mood_keywords)) {
        if (data.mood_keywords.length > 0) {
          if (typeof data.mood_keywords[0] === 'string') {
            const defaultColors = [
              '#7B68EE',
              '#FF6B9D',
              '#4ECDC4',
              '#FFB347',
              '#95E1D3',
            ]
            moodKeywords = data.mood_keywords.map((tag: string, i: number) => ({
              tag,
              color: defaultColors[i % defaultColors.length],
            }))
          } else if (typeof data.mood_keywords[0] === 'object') {
            moodKeywords = data.mood_keywords
          }
        }
      }
      return {
        soul_color: data.soul_color,
        mood_keywords: moodKeywords,
        library_items: data.library_items,
        follower_count: data.follower_count,
        playlist_count: data.playlist_count,
        level: data.level,
      }
    }, [data])

    const libraryItems = useMemo(
      () => processedData?.library_items || [],
      [processedData?.library_items],
    )
    // 一页两首。首页由报告卡按页码给；嵌在别处时自己轮换。
    const paged = useReportDetailPage(
      pairPageCount(libraryItems.length),
      REPORT_PAIR_DWELL_MS,
    )
    const [ownItemIndex, setCurrentItemIndex] = useState(0)
    const prevShowOverviewRef = useRef(showOverview)

    useEffect(() => {
      if (
        paged === null &&
        prevShowOverviewRef.current &&
        !showOverview &&
        libraryItems.length > 0
      ) {
        setCurrentItemIndex((prev) => (prev + 2) % libraryItems.length)
      }
      prevShowOverviewRef.current = showOverview
    }, [paged, showOverview, libraryItems.length])

    useEffect(() => {
      if (paged === null && !showOverview && libraryItems.length > 0) {
        let cancelled = false
        let timeoutId: number | null = null
        const tick = () => {
          if (cancelled || document.hidden) return
          setCurrentItemIndex((prev) => (prev + 2) % libraryItems.length)
          timeoutId = window.setTimeout(tick, 5000)
        }
        timeoutId = window.setTimeout(tick, 5000)

        const onVisibility = () => {
          if (document.hidden && timeoutId) {
            clearTimeout(timeoutId)
            timeoutId = null
          } else if (!document.hidden && !cancelled && !timeoutId) {
            tick()
          }
        }
        document.addEventListener('visibilitychange', onVisibility)

        return () => {
          cancelled = true
          if (timeoutId) clearTimeout(timeoutId)
          document.removeEventListener('visibilitychange', onVisibility)
        }
      }
    }, [paged, showOverview, libraryItems.length])
    const currentItemIndex = paged === null ? ownItemIndex : paged * 2

    const currentItems = useMemo(
      () =>
        [
          libraryItems[currentItemIndex],
          libraryItems[(currentItemIndex + 1) % libraryItems.length],
        ].filter(Boolean),
      [libraryItems, currentItemIndex],
    )

    useEffect(() => {
      if (!showOverview && currentItems.length > 0) {
        onContentChange?.({
          titles: currentItems.map((item: any) => item.title),
          type: 'music',
        })
      } else {
        onContentChange?.(null)
      }
    }, [showOverview, currentItems, onContentChange])

    return (
      <AnimatePresence mode="wait">
        {showOverview ||
        currentItems.length === 0 ||
        libraryItems.length === 0 ? (
          <motion.div
            key="stats"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.5 }}
            className="h-full w-full"
          >
            <MusicStatsWidget
              data={processedData}
              allowLoop={allowLoop}
            />
          </motion.div>
        ) : (
          <motion.div
            key={`music-${currentItemIndex}`}
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -10 }}
            transition={{ duration: 0.5 }}
            className="h-full w-full p-1.5"
          >
            <div className="h-full w-full flex gap-1.5">
              {currentItems.map((item: any, idx: number) => (
                <div key={idx} className="flex-1 h-full">
                  <div className="relative h-full w-full rounded-lg overflow-hidden shadow-lg bg-white dark:bg-black/90">
                    <div className="absolute inset-0">
                      <img
                        src={
                          item.cover ||
                          `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=200&background=e60026&color=fff`
                        }
                        alt={item.title}
                        className="w-full h-full object-cover"
                        loading="lazy"
                        referrerPolicy="no-referrer"
                      />
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    )
  },
)
