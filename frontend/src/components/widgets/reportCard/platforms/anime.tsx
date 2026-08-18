import type { ReactNode } from 'react'
import { SiBangumi, SiMyanimelist } from '@lib/icons'
/**
 * Anime-list report cards: Bangumi + MyAnimeList.
 *
 * Both platforms render the exact same face (overview stats + type-composition
 * bar + two-column cover carousel); only brand color, stat labels and the
 * subject-type vocabulary differ. They share one component so the two cards
 * cannot drift apart — a change to the Bangumi face is a change to MAL's.
 */
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { RatingBadge } from '../../../RatingBadge'
import {
  CONTENT_FADE_ANIMATE,
  CONTENT_FADE_EXIT,
  CONTENT_FADE_INITIAL,
  CONTENT_FADE_TRANSITION,
  CONTENT_SLIDE_ANIMATE,
  CONTENT_SLIDE_EXIT,
  CONTENT_SLIDE_INITIAL,
  CONTENT_SLIDE_TRANSITION,
} from '../animations'

type I18nT = ReturnType<typeof useI18n>['t']

/** Per-platform strings pulled from i18n at render time. */
interface AnimeListLabels {
  /** Taste badge fallback when the report carries no `taste_profile`. */
  fallbackTaste: string
  done: string
  doing: string
  wish: string
  /** subject_type key → localized name for the composition bar legend. */
  typeLabels: Record<string, string>
}

interface AnimeListTheme {
  /** Overview background wash. */
  surfaceClass: string
  /** Taste badge chip. */
  badgeClass: string
  /** Detail-face placeholder when an item has no cover. */
  emptyCoverClass: string
  /** Icon for that placeholder. */
  fallbackIcon: ReactNode
  /** subject_type key → segment color. */
  typeColors: Record<string, string>
  /** Segment color for keys outside `typeColors`. */
  fallbackTypeColor: string
  labels: (t: I18nT) => AnimeListLabels
}

/** Exported so tests can assert both platforms cover their own subject types. */
export const ANIME_THEMES: Record<'bangumi' | 'mal', AnimeListTheme> = {
  // Bangumi 覆盖动画/书/游戏/音乐/剧集五类
  bangumi: {
    surfaceClass:
      'bg-linear-to-br from-rose-50/50 to-transparent dark:from-rose-900/20 dark:to-transparent',
    badgeClass:
      'bg-rose-400/15 text-rose-500 border border-rose-400/25',
    emptyCoverClass: 'text-rose-400 bg-rose-50 dark:bg-rose-950/30',
    fallbackIcon: <SiBangumi />,
    typeColors: {
      anime: '#fb7185',
      book: '#a78bfa',
      game: '#60a5fa',
      music: '#34d399',
      real: '#fbbf24',
    },
    fallbackTypeColor: '#f09199',
    labels: (t) => ({
      fallbackTaste: t.widgets.reportBangumi,
      done: t.reportsPage.bangumiDone,
      doing: t.reportsPage.bangumiDoing,
      wish: t.reportsPage.bangumiWish,
      typeLabels: {
        book: t.library.book,
        anime: t.library.anime,
        game: t.library.game,
        music: t.library.music,
        real: t.library.tvSeries,
      },
    }),
  },
  // MAL 只有动画 / 漫画两类
  mal: {
    surfaceClass:
      'bg-linear-to-br from-blue-50/50 to-transparent dark:from-blue-900/20 dark:to-transparent',
    badgeClass:
      'bg-blue-500/15 text-blue-600 dark:text-blue-400 border border-blue-500/25',
    emptyCoverClass: 'text-blue-400 bg-blue-50 dark:bg-blue-950/30',
    fallbackIcon: <SiMyanimelist />,
    typeColors: {
      anime: '#2e51a2',
      manga: '#60a5fa',
    },
    fallbackTypeColor: '#2e51a2',
    labels: (t) => ({
      fallbackTaste: t.widgets.reportMal,
      done: t.reportsPage.malDone,
      doing: t.reportsPage.malDoing,
      wish: t.reportsPage.malWish,
      typeLabels: {
        anime: t.library.anime,
        manga: t.library.book,
      },
    }),
  },
}

const AnimeListFace = memo(
  ({
    theme,
    data,
    showOverview,
    onContentChange,
  }: {
    theme: AnimeListTheme
    data: any
    showOverview: boolean
    onContentChange?: (content: any) => void
  }) => {
    const { t } = useI18n()
    const labels = useMemo(() => theme.labels(t), [theme, t])
    const libraryItems = useMemo(
      () => data?.library_items || [],
      [data?.library_items],
    )
    const statusCounts =
      data?.status_counts || data?.collection_type_distribution || {}
    const done = statusCounts.done || 0
    const doing = statusCounts.doing || 0
    const wish = statusCounts.wish || 0
    const typeDist = useMemo(
      () =>
        Object.entries(data?.subject_type_distribution || {})
          .filter(([, n]) => (n as number) > 0)
          .sort((a, b) => (b[1] as number) - (a[1] as number)),
      [data?.subject_type_distribution],
    )
    const totalSubjects = useMemo(
      () => typeDist.reduce((sum, [, n]) => sum + (n as number), 0),
      [typeDist],
    )
    // 构成条：按占比换算时长，各色段首尾相接连续填充
    const barSegments = useMemo(() => {
      if (totalSubjects === 0) return []
      const fillDuration = 0.9
      const baseDelay = 0.55
      let acc = 0
      return typeDist.map(([type, count]) => {
        const n = count as number
        const segment = {
          type,
          count: n,
          pct: (n / totalSubjects) * 100,
          delay: baseDelay + (acc / totalSubjects) * fillDuration,
          duration: (n / totalSubjects) * fillDuration,
        }
        acc += n
        return segment
      })
    }, [typeDist, totalSubjects])
    // 概览态海报墙素材：有封面的收藏，最多 5 张
    const wallCovers = useMemo(
      () => libraryItems.filter((item: any) => item.cover).slice(0, 5),
      [libraryItems],
    )
    const [currentIndex, setCurrentIndex] = useState(0)
    const prevShowOverviewRef = useRef(showOverview)

    // 与网易云卡片一致：从概览切到详情时推进两位
    useEffect(() => {
      if (
        prevShowOverviewRef.current &&
        !showOverview &&
        libraryItems.length > 0
      ) {
        setCurrentIndex((prev) => (prev + 2) % libraryItems.length)
      }
      prevShowOverviewRef.current = showOverview
    }, [showOverview, libraryItems.length])

    useEffect(() => {
      if (showOverview || libraryItems.length === 0) return
      const timer = window.setInterval(() => {
        setCurrentIndex((prev) => (prev + 2) % libraryItems.length)
      }, 5000)
      return () => window.clearInterval(timer)
    }, [showOverview, libraryItems.length])

    // 一次展示两列封面（学网易云卡片）
    const currentItems = useMemo(() => {
      if (libraryItems.length === 0) return []
      if (libraryItems.length === 1) return [libraryItems[0]]
      return [
        libraryItems[currentIndex],
        libraryItems[(currentIndex + 1) % libraryItems.length],
      ]
    }, [libraryItems, currentIndex])

    useEffect(() => {
      if (!showOverview && currentItems.length > 0) {
        onContentChange?.({
          titles: currentItems.map((item: any) => item.title),
        })
      } else {
        onContentChange?.(null)
      }
    }, [showOverview, currentItems, onContentChange])

    return (
      <AnimatePresence mode="wait">
        {showOverview || currentItems.length === 0 ? (
          <motion.div
            key="stats"
            initial={CONTENT_FADE_INITIAL}
            animate={CONTENT_FADE_ANIMATE}
            exit={CONTENT_FADE_EXIT}
            transition={CONTENT_FADE_TRANSITION}
            className="h-full w-full"
          >
            <div className="relative h-full w-full overflow-hidden">
              <div className={`absolute inset-0 ${theme.surfaceClass}`} />
              {/* 右侧背景：斜切海报墙，向左渐隐 */}
              {wallCovers.length > 0 && (
                <div
                  className="absolute inset-y-0 right-0 w-[58%] opacity-70 dark:opacity-50"
                  style={{
                    maskImage:
                      'linear-gradient(to left, rgba(0,0,0,1) 45%, transparent 100%)',
                    WebkitMaskImage:
                      'linear-gradient(to left, rgba(0,0,0,1) 45%, transparent 100%)',
                  }}
                >
                  <div className="absolute -inset-y-4 left-0 right-0 flex items-center justify-end gap-2 pr-4 rotate-6">
                    {wallCovers.map((item: any, i: number) => (
                      <motion.div
                        key={`${item.title}-${i}`}
                        className="w-14 shrink-0 aspect-[3/4] rounded-lg overflow-hidden shadow-md ring-1 ring-black/10 dark:ring-white/10"
                        initial={{
                          x: 60,
                          opacity: 0,
                          y: i % 2 === 0 ? -12 : 12,
                        }}
                        animate={{
                          x: 0,
                          opacity: 1,
                          y: i % 2 === 0 ? -12 : 12,
                        }}
                        transition={{
                          duration: 0.5,
                          delay: 0.15 + i * 0.08,
                          ease: 'easeOut',
                        }}
                      >
                        <img
                          src={item.cover}
                          alt={item.title}
                          className="w-full h-full object-cover"
                          loading="lazy"
                          referrerPolicy="no-referrer"
                        />
                      </motion.div>
                    ))}
                  </div>
                </div>
              )}
              {/* 前景 */}
              <div className="relative z-10 h-full flex flex-col p-2.5">
                {/* 顶部：品味徽章 */}
                <motion.div
                  className={`w-fit max-w-[70%] px-2 py-0.5 rounded-md text-[9px] font-bold flex items-center gap-1 shadow-sm ${theme.badgeClass}`}
                  initial={{ scale: 0.8, opacity: 0 }}
                  animate={{ scale: 1, opacity: 1 }}
                  transition={{ duration: 0.3, delay: 0.2 }}
                >
                  <span className="text-[7px] shrink-0">●</span>
                  <span className="truncate">
                    {data?.taste_profile || labels.fallbackTaste}
                  </span>
                </motion.div>
                {/* 中部：数字区在徽章与左下角 Logo 安全区之间垂直居中（pb 略小于 Logo 区高度，整体略下沉） */}
                <div className="flex-1 min-h-0 flex items-center pb-9.5">
                  <div className="flex items-end gap-3 pl-1">
                    <motion.div
                      className="flex flex-col"
                      initial={{ y: 10, opacity: 0 }}
                      animate={{ y: 0, opacity: 1 }}
                      transition={{ duration: 0.4, delay: 0.2 }}
                    >
                      <motion.span
                        className="text-[40px] font-black text-gray-800 dark:text-gray-100 leading-none tabular-nums"
                        initial={{ scale: 0.5 }}
                        animate={{ scale: 1 }}
                        transition={{
                          duration: 0.5,
                          delay: 0.3,
                          type: 'spring',
                          stiffness: 200,
                        }}
                      >
                        {done}
                      </motion.span>
                      <span className="text-[8px] text-gray-500 dark:text-gray-400 uppercase tracking-widest font-bold mt-0.5">
                        {labels.done}
                      </span>
                    </motion.div>
                    <div className="flex gap-3">
                      {(
                        [
                          [doing, labels.doing],
                          [wish, labels.wish],
                        ] as const
                      ).map(([count, label], i) => (
                        <motion.div
                          key={label}
                          className="flex flex-col"
                          initial={{ y: 10, opacity: 0 }}
                          animate={{ y: 0, opacity: 1 }}
                          transition={{ duration: 0.4, delay: 0.4 + i * 0.1 }}
                        >
                          <span className="text-[22px] font-black text-gray-800 dark:text-gray-200 leading-none tabular-nums">
                            {count}
                          </span>
                          <span className="text-[8px] text-gray-500 dark:text-gray-400 uppercase tracking-widest font-bold mt-0.5">
                            {label}
                          </span>
                        </motion.div>
                      ))}
                    </div>
                  </div>
                </div>
                {/* 底部右侧：类型构成堆叠条 + 图例，绝对定位钉在右下 */}
                {barSegments.length > 0 && (
                  <div className="absolute bottom-3 right-3 w-[45%] flex flex-col items-end gap-1">
                    <div className="flex flex-wrap justify-end gap-x-2.5 gap-y-0.5">
                      {barSegments.map((segment) => (
                        <motion.span
                          key={segment.type}
                          className="flex items-center gap-1 text-[8px] font-bold text-gray-600 dark:text-gray-300"
                          initial={{ opacity: 0 }}
                          animate={{ opacity: 1 }}
                          transition={{ duration: 0.3, delay: segment.delay }}
                        >
                          <span
                            className="w-1.5 h-1.5 rounded-full"
                            style={{
                              backgroundColor:
                                theme.typeColors[segment.type] ||
                                theme.fallbackTypeColor,
                            }}
                          />
                          {labels.typeLabels[segment.type] || segment.type}
                          <span className="font-mono text-gray-500 dark:text-gray-400">
                            {segment.count}
                          </span>
                        </motion.span>
                      ))}
                    </div>
                    <div className="flex h-1.5 w-full rounded-full overflow-hidden bg-gray-200/80 dark:bg-white/10 ring-1 ring-black/5 dark:ring-white/10">
                      {barSegments.map((segment) => (
                        <motion.div
                          key={segment.type}
                          className="h-full"
                          style={{
                            backgroundColor:
                              theme.typeColors[segment.type] ||
                              theme.fallbackTypeColor,
                          }}
                          initial={{ width: 0 }}
                          animate={{ width: `${segment.pct}%` }}
                          transition={{
                            duration: segment.duration,
                            delay: segment.delay,
                            ease: 'linear',
                          }}
                        />
                      ))}
                    </div>
                  </div>
                )}
              </div>
            </div>
          </motion.div>
        ) : (
          <motion.div
            key={`lib-${currentIndex}`}
            initial={CONTENT_SLIDE_INITIAL}
            animate={CONTENT_SLIDE_ANIMATE}
            exit={CONTENT_SLIDE_EXIT}
            transition={CONTENT_SLIDE_TRANSITION}
            className="h-full w-full p-1.5"
          >
            {/* 两列封面（学网易云卡片） */}
            <div className="h-full w-full flex gap-1.5">
              {currentItems.map((item: any, idx: number) => (
                <div key={idx} className="flex-1 h-full">
                  <div className="relative h-full w-full rounded-lg overflow-hidden shadow-lg bg-white dark:bg-black/90">
                    <div className="absolute inset-0">
                      {item.cover ? (
                        <img
                          src={item.cover}
                          alt={item.title}
                          className="w-full h-full object-cover"
                          loading="lazy"
                          referrerPolicy="no-referrer"
                        />
                      ) : (
                        <div
                          className={`w-full h-full flex items-center justify-center text-4xl ${theme.emptyCoverClass}`}
                        >
                          {theme.fallbackIcon}
                        </div>
                      )}
                    </div>
                    {/* 资料库同款评分徽章（卡片内统一尺寸） */}
                    <RatingBadge
                      rate={item.rate}
                      className="absolute top-1.5 left-1.5 z-20"
                      sizeClass="w-7 h-7 text-sm"
                    />
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

export const BangumiWidget = memo((props: any) => (
  <AnimeListFace theme={ANIME_THEMES.bangumi} {...props} />
))

export const MalWidget = memo((props: any) => (
  <AnimeListFace theme={ANIME_THEMES.mal} {...props} />
))
