import type { MouseEvent as ReactMouseEvent } from 'react'
import { LuClock, LuEye, LuHeart } from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import {
  memo,

  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
} from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
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
import { formatCompactNumber, formatYoutubeDuration } from '../format'
import { useCountUp, useLibraryItemRotation } from '../hooks'

function safeNonNegInt(v: unknown): number {
  const n = Number(v)
  if (!Number.isFinite(n) || n < 0) return 0
  return Math.round(n)
}

function normalizeHandle(raw?: string | null): string | null {
  if (!raw || typeof raw !== 'string') return null
  const s = raw.trim()
  if (!s) return null
  return s.startsWith('@') ? s : `@${s}`
}

type YtAwardTier = 'none' | 'silver' | 'gold' | 'diamond' | 'red_diamond'

function awardTierFromSubs(subscribers: number): YtAwardTier {
  if (subscribers >= 1_000_000) return 'red_diamond'
  if (subscribers >= 100_000) return 'diamond'
  if (subscribers >= 10_000) return 'gold'
  if (subscribers >= 1_000) return 'silver'
  return 'none'
}

interface GradStop { offset: string; color: string; opacity?: number }

interface AwardStyle {
  wash: string
  shadow: string
  body: [string, string, string]
  textGrad: GradStop[]
  textHalo: string
}

const YT_BADGE_PATH =
  'M66.52 7.74c-.78-2.93-2.49-5.41-5.42-6.19C55.79.13 34 0 34 0S12.21.13 6.9 1.55C3.97 2.33 2.27 4.81 1.48 7.74.06 13.05 0 24 0 24s.06 10.95 1.48 16.26c.78 2.93 2.49 5.41 5.42 6.19C12.21 47.87 34 48 34 48s21.79-.13 27.1-1.55c2.93-.78 4.64-3.26 5.42-6.19C67.94 34.95 68 24 68 24s-.06-10.95-1.48-16.26z'

function cssStops(stops: GradStop[], angleDeg: number): string {
  return `linear-gradient(${angleDeg}deg, ${stops
    .map((s) => `${s.color} ${s.offset}`)
    .join(', ')})`
}

const PLAQUE_TILT_MAX = 9

const AWARD_STYLES: Record<YtAwardTier, AwardStyle> = {
  none: {
    wash: 'rgba(255, 140, 140, 0.14)',
    shadow: 'rgba(255, 190, 190, 0.45)',
    body: ['#FFB0B0', '#FF7A7A', '#F05555'],
    textHalo: 'rgba(255, 235, 235, 0.9)',
    textGrad: [
      { offset: '0%', color: '#FFD4D4' },
      { offset: '50%', color: '#FF8080' },
      { offset: '100%', color: '#FFA8A8' },
    ],
  },
  silver: {
    wash: 'rgba(180, 180, 190, 0.14)',
    shadow: 'rgba(200, 200, 215, 0.45)',
    body: ['#D0D0D8', '#A8A8B4', '#7C7C88'],
    textHalo: 'rgba(230, 230, 238, 0.8)',
    textGrad: [
      { offset: '0%', color: '#E0E0E8' },
      { offset: '45%', color: '#9A9AA8' },
      { offset: '100%', color: '#C4C4D0' },
    ],
  },
  gold: {
    wash: 'rgba(220, 180, 60, 0.14)',
    shadow: 'rgba(240, 200, 100, 0.42)',
    body: ['#E8C850', '#D4A828', '#A88210'],
    textHalo: 'rgba(240, 220, 150, 0.8)',
    textGrad: [
      { offset: '0%', color: '#F0D878' },
      { offset: '45%', color: '#C9A020' },
      { offset: '100%', color: '#E0C040' },
    ],
  },
  diamond: {
    wash: 'rgba(160, 210, 230, 0.16)',
    shadow: 'rgba(160, 210, 230, 0.45)',
    body: ['#C8E4F0', '#88C0D4', '#4A98B4'],
    textHalo: 'rgba(200, 230, 245, 0.8)',
    textGrad: [
      { offset: '0%', color: '#C0E0EC' },
      { offset: '45%', color: '#5AA8C0' },
      { offset: '100%', color: '#98D0E0' },
    ],
  },
  red_diamond: {
    wash: 'rgba(200, 50, 70, 0.12)',
    shadow: 'rgba(255, 140, 160, 0.4)',
    body: ['#E86878', '#D03850', '#A82038'],
    textHalo: 'rgba(255, 200, 210, 0.8)',
    textGrad: [
      { offset: '0%', color: '#F0A8B4' },
      { offset: '45%', color: '#E04860' },
      { offset: '100%', color: '#F08898' },
    ],
  },
}

interface YtVideoItem {
  title: string
  type: string
  image?: string
  cover?: string
  url?: string
  video_id?: string
  view_count?: number | null
  like_count?: number | null
  comment_count?: number | null
  published_at?: string | null
  duration?: string | null
}

const YoutubeStatsWidget = memo(({ data }: { data: any }) => {
  const { t } = useI18n()
  const subscribers = useMemo(
    () => safeNonNegInt(data?.subscriber_count),
    [data?.subscriber_count],
  )
  const views = useMemo(
    () => safeNonNegInt(data?.view_count),
    [data?.view_count],
  )
  const videos = useMemo(
    () => safeNonNegInt(data?.video_count),
    [data?.video_count],
  )
  const channelTitle = useMemo(
    () =>
      (data?.channel_title as string) ||
      (data?.username as string) ||
      'YouTube',
    [data?.channel_title, data?.username],
  )
  const handle = useMemo(
    () =>
      normalizeHandle(data?.custom_url) ||
      normalizeHandle(
        typeof data?.channel_url === 'string' &&
          data.channel_url.includes('/@')
          ? data.channel_url.split('/@').pop()?.split(/[/?#]/)[0]
          : null,
      ),
    [data?.custom_url, data?.channel_url],
  )
  const avatar = useMemo(
    () =>
      (typeof data?.avatar === 'string' && data.avatar) ||
      (typeof data?.profile?.avatar === 'string' && data.profile.avatar) ||
      null,
    [data?.avatar, data?.profile],
  )

  const vibe = useMemo(() => {
    const raw = data?.vibe
    if (typeof raw !== 'string') return null
    const s = raw.trim()
    return s.length > 0 ? s : null
  }, [data?.vibe])
  const channelType = useMemo(() => {
    const raw = data?.channel_type
    if (typeof raw !== 'string') return null
    const s = raw.trim()
    return s.length > 0 ? s : null
  }, [data?.channel_type])

  const isEmptyChannel =
    data?.is_empty_channel === true || videos === 0

  const awardTier = useMemo(
    () => awardTierFromSubs(subscribers),
    [subscribers],
  )
  const award = AWARD_STYLES[awardTier]
  const gradId = useId().replaceAll(':', '')

  const subsDisplay = useCountUp(subscribers, 700, 80)
  const viewsDisplay = useCountUp(views, 700, 160)
  const videosDisplay = useCountUp(videos, 700, 220)

  // 倾斜不走 React state，setState 会让整张 face 60fps 重绘。
  const cardRef = useRef<HTMLDivElement>(null)
  const medalRef = useRef<HTMLDivElement>(null)
  const specRef = useRef<SVGLinearGradientElement>(null)
  const glintRef = useRef<SVGRadialGradientElement>(null)
  const cardRectRef = useRef<DOMRect | null>(null)
  const tiltTarget = useRef({ rx: 0, ry: 0 })
  const tiltCurrent = useRef({ rx: 0, ry: 0 })
  const tiltRaf = useRef(0)
  const reduceMotion = useMemo(() => {
    if (typeof window === 'undefined' || !window.matchMedia) return false
    return window.matchMedia('(prefers-reduced-motion: reduce)').matches
  }, [])

  const paintTilt = useCallback((rx: number, ry: number) => {
    const medal = medalRef.current
    if (medal) {
      medal.style.transform = `rotateX(${rx}deg) rotateY(${ry}deg) translateZ(8px)`
    }
    const spec = specRef.current
    if (spec) {
      spec.setAttribute('x1', `${22 + ry * 1.2}%`)
      spec.setAttribute('y1', `${8 - rx * 1.1}%`)
      spec.setAttribute('x2', `${78 + ry * 0.8}%`)
      spec.setAttribute('y2', `${92 - rx * 0.6}%`)
    }
    const glint = glintRef.current
    if (glint) {
      glint.setAttribute('cx', `${30 + ry * 0.9}%`)
      glint.setAttribute('cy', `${26 - rx * 0.9}%`)
    }
  }, [])

  const runTiltLoop = useCallback(() => {
    const c = tiltCurrent.current
    const t = tiltTarget.current
    c.rx += (t.rx - c.rx) * 0.14
    c.ry += (t.ry - c.ry) * 0.14
    const done =
      Math.abs(c.rx - t.rx) < 0.02 && Math.abs(c.ry - t.ry) < 0.02
    if (done) {
      c.rx = t.rx
      c.ry = t.ry
      paintTilt(c.rx, c.ry)
      tiltRaf.current = 0
      return
    }
    paintTilt(c.rx, c.ry)
    tiltRaf.current = requestAnimationFrame(runTiltLoop)
  }, [paintTilt])

  const kickTilt = useCallback(() => {
    if (reduceMotion) return
    if (!tiltRaf.current) {
      tiltRaf.current = requestAnimationFrame(runTiltLoop)
    }
  }, [reduceMotion, runTiltLoop])

  const onCardPointerEnter = useCallback(() => {
    if (reduceMotion) return
    const el = cardRef.current
    if (el) cardRectRef.current = el.getBoundingClientRect()
  }, [reduceMotion])

  const onCardPointerMove = useCallback(
    (e: ReactMouseEvent<HTMLDivElement>) => {
      if (reduceMotion) return
      let r = cardRectRef.current
      if (!r) {
        const el = cardRef.current
        if (!el) return
        r = el.getBoundingClientRect()
        cardRectRef.current = r
      }
      if (r.width < 1 || r.height < 1) return
      const px = (e.clientX - r.left) / r.width - 0.5
      const py = (e.clientY - r.top) / r.height - 0.5
      tiltTarget.current = {
        rx: Math.max(
          -PLAQUE_TILT_MAX,
          Math.min(PLAQUE_TILT_MAX, -py * 2 * PLAQUE_TILT_MAX),
        ),
        ry: Math.max(
          -PLAQUE_TILT_MAX,
          Math.min(PLAQUE_TILT_MAX, px * 2 * PLAQUE_TILT_MAX),
        ),
      }
      kickTilt()
    },
    [kickTilt, reduceMotion],
  )

  const onCardPointerLeave = useCallback(() => {
    cardRectRef.current = null
    tiltTarget.current = { rx: 0, ry: 0 }
    kickTilt()
  }, [kickTilt])

  useEffect(() => {
    return () => {
      if (tiltRaf.current) cancelAnimationFrame(tiltRaf.current)
    }
  }, [])

  const logoStats = [
    {
      key: 'subs',
      value: subsDisplay,
      label: t.reportCardWidget.ytSubscribers,
    },
    {
      key: 'views',
      value: viewsDisplay,
      label: t.reportCardWidget.ytViews,
    },
    {
      key: 'videos',
      value: videosDisplay,
      label: t.reportCardWidget.ytVideos,
    },
  ] as const

  return (
    <div
      ref={cardRef}
      className="relative h-full w-full overflow-hidden"
      onMouseEnter={onCardPointerEnter}
      onMouseMove={onCardPointerMove}
      onMouseLeave={onCardPointerLeave}
    >
      <div
        className="absolute inset-0 opacity-80 dark:opacity-45"
        style={{
          background: `linear-gradient(to bottom right, ${award.wash} 0%, transparent 55%)`,
        }}
      />

      <motion.div
        className="absolute top-3 right-3 z-20 flex items-baseline gap-2"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ duration: 0.4, delay: 0.3 }}
      >
        {logoStats.map((row) => (
          <span key={row.key} className="flex items-baseline gap-0.5">
            <span className="text-[10px] font-black tabular-nums text-gray-900 dark:text-gray-100">
              {formatCompactNumber(row.value)}
            </span>
            <span className="text-[9px] font-medium text-gray-500 dark:text-gray-400">
              {row.label}
            </span>
          </span>
        ))}
      </motion.div>

      <motion.div
        className="pointer-events-none absolute right-5 bottom-5 z-10 flex w-[6.5rem] flex-col items-center gap-1.5"
        initial={{ x: 20, opacity: 0, scale: 0.92 }}
        animate={{ x: 0, opacity: 1, scale: 1 }}
        transition={{ duration: 0.45, delay: 0.15, ease: 'easeOut' }}
        style={{ perspective: 520 }}
        title={
          awardTier === 'none'
            ? undefined
            : `YouTube ${awardTier.replace('_', ' ')} play button`
        }
      >
        <div
          ref={medalRef}
          className="relative w-full will-change-transform"
          style={{
            aspectRatio: '68 / 48',
            transform: 'rotateX(0deg) rotateY(0deg) translateZ(8px)',
            transformStyle: 'preserve-3d',
            filter: `drop-shadow(0 0 3px ${award.shadow}) drop-shadow(0 6px 14px ${award.shadow})`,
          }}
        >
          <svg
            className="h-full w-full"
            viewBox="0 0 68 48"
            preserveAspectRatio="xMidYMid meet"
            aria-hidden
          >
            <defs>
              <linearGradient
                id={`yt-body-${gradId}`}
                x1="0%"
                y1="0%"
                x2="100%"
                y2="100%"
              >
                <stop offset="0%" stopColor={award.body[0]} />
                <stop offset="48%" stopColor={award.body[1]} />
                <stop offset="100%" stopColor={award.body[2]} />
              </linearGradient>
              <linearGradient
                ref={specRef}
                id={`yt-spec-${gradId}`}
                x1="22%"
                y1="8%"
                x2="78%"
                y2="92%"
              >
                <stop offset="0%" stopColor="#fff" stopOpacity="0.38" />
                <stop offset="38%" stopColor="#fff" stopOpacity="0.06" />
                <stop offset="48%" stopColor="#fff" stopOpacity="0.42" />
                <stop offset="55%" stopColor="#fff" stopOpacity="0.08" />
                <stop offset="100%" stopColor="#fff" stopOpacity="0" />
              </linearGradient>
              <radialGradient
                ref={glintRef}
                id={`yt-glint-${gradId}`}
                cx="30%"
                cy="26%"
                r="46%"
              >
                <stop offset="0%" stopColor="#fff" stopOpacity="0.45" />
                <stop offset="45%" stopColor="#fff" stopOpacity="0.08" />
                <stop offset="100%" stopColor="#fff" stopOpacity="0" />
              </radialGradient>
              <clipPath id={`yt-clip-${gradId}`}>
                <path d={YT_BADGE_PATH} />
              </clipPath>
            </defs>
            <path d={YT_BADGE_PATH} fill={`url(#yt-body-${gradId})`} />
            <path
              d={YT_BADGE_PATH}
              fill={`url(#yt-spec-${gradId})`}
              style={{ mixBlendMode: 'soft-light' }}
            />
            <path
              d={YT_BADGE_PATH}
              fill={`url(#yt-glint-${gradId})`}
              style={{ mixBlendMode: 'screen' }}
            />
            <g clipPath={`url(#yt-clip-${gradId})`}>
              <path
                d={YT_BADGE_PATH}
                fill="none"
                stroke="rgba(255,255,255,0.5)"
                strokeWidth={1.15}
                vectorEffect="non-scaling-stroke"
              />
              <path
                d={YT_BADGE_PATH}
                fill="none"
                stroke="rgba(0,0,0,0.1)"
                strokeWidth={0.7}
                vectorEffect="non-scaling-stroke"
                opacity={0.7}
                transform="translate(0.5 0.55)"
              />
            </g>
            <path d="M27 15v18l16-9-16-9z" fill="#F0F0F0" opacity={0.88} />
            <path
              d="M29.5 19.5v9.5l8.5-4.75-8.5-4.75z"
              fill="#fff"
              opacity={0.35}
            />
          </svg>
        </div>
        {channelType && (
          <span className="relative inline-flex max-w-[90%] items-center justify-center">
            <span
              aria-hidden
              className="pointer-events-none absolute inset-0 truncate text-center text-[10px] font-black leading-tight tracking-wide"
              style={{
                color: award.textHalo,
                WebkitTextStroke: `1px ${award.textHalo}`,
                textShadow: `0 0 3px ${award.textHalo}, 0 0 6px ${award.shadow}`,
              }}
            >
              {channelType}
            </span>
            <span
              className="relative truncate text-center text-[10px] font-black leading-tight tracking-wide"
              style={{
                backgroundImage: cssStops(award.textGrad, 120),
                WebkitBackgroundClip: 'text',
                backgroundClip: 'text',
                color: 'transparent',
                WebkitTextFillColor: 'transparent',
              }}
            >
              {channelType}
            </span>
          </span>
        )}
      </motion.div>

      <div className="relative z-10 flex h-full flex-col p-3 pb-9 pointer-events-none">
        <motion.div
          className="flex min-w-0 max-w-[70%] items-center gap-2"
          initial={{ y: 8, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          transition={{ duration: 0.4, delay: 0.1 }}
        >
          {avatar ? (
            <img
              src={avatar}
              alt=""
              className="h-8 w-8 shrink-0 rounded-full object-cover shadow-sm ring-2 ring-white/80 dark:ring-black/50"
              loading="lazy"
              referrerPolicy="no-referrer"
            />
          ) : (
            <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-red-500/15 text-[10px] font-black text-red-600 dark:text-red-400">
              YT
            </div>
          )}
          <div className="min-w-0">
            <div className="truncate text-[11px] font-bold leading-tight text-gray-900 dark:text-gray-100">
              {channelTitle}
            </div>
            {handle && (
              <div className="truncate font-mono text-[9px] text-gray-500 dark:text-gray-400">
                {handle}
              </div>
            )}
          </div>
        </motion.div>

        {(vibe || isEmptyChannel) && (
          <div className="flex min-h-0 flex-1 items-center pt-2 pb-4">
            <motion.div
              className="max-w-[68%]"
              initial={{ y: 8, opacity: 0 }}
              animate={{ y: 0, opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.2 }}
            >
              <span className="block text-[17px] font-black leading-snug line-clamp-2 text-balance text-gray-900 dark:text-gray-100">
                {vibe || t.reportCardWidget.ytEmptyChannel}
              </span>
            </motion.div>
          </div>
        )}

        {!vibe && !isEmptyChannel && <div className="min-h-0 flex-1" />}
      </div>
    </div>
  )
})
YoutubeStatsWidget.displayName = 'YoutubeStatsWidget'

function YtMetaIcon({
  Icon,
  optical = 1,
}: {
  Icon: typeof LuClock
  optical?: number
}) {
  const px = 10
  return (
    <span
      className="inline-flex shrink-0 items-center justify-center opacity-90"
      style={{ width: px, height: px }}
      aria-hidden
    >
      <Icon
        size={px}
        strokeWidth={2.25}
        style={{
          width: px * optical,
          height: px * optical,
        }}
      />
    </span>
  )
}

const YoutubeVideoSlide = memo(({ item }: { item: YtVideoItem }) => {
  const cover = item.image || item.cover
  const durationLabel = formatYoutubeDuration(item.duration)
  const views =
    item.view_count != null ? safeNonNegInt(item.view_count) : null
  const likes =
    item.like_count != null ? safeNonNegInt(item.like_count) : null

  const parts: string[] = []
  if (durationLabel) parts.push('dur')
  if (views != null) parts.push('views')
  if (likes != null && likes > 0) parts.push('likes')

  return (
    <div className="relative h-full w-full overflow-hidden rounded-lg bg-black/10 shadow-lg dark:bg-black/40">
      {cover ? (
        <img
          src={cover}
          alt=""
          className="absolute inset-0 h-full w-full object-cover"
          loading="lazy"
          referrerPolicy="no-referrer"
        />
      ) : (
        <div className="absolute inset-0 bg-linear-to-br from-red-600/40 to-black/80" />
      )}

      {parts.length > 0 && (
        <div className="absolute right-1.5 top-1.5 z-10 flex items-center gap-1 rounded-md bg-black/70 px-1.5 py-0.5 text-[9px] font-bold tabular-nums leading-none text-white shadow">
          {parts.map((key, i) => (
            <span key={key} className="inline-flex items-center gap-1">
              {i > 0 && (
                <span className="text-white/35" aria-hidden>
                  ·
                </span>
              )}
              {key === 'dur' && (
                <span className="inline-flex items-center gap-0.5">
                  <YtMetaIcon Icon={LuClock} optical={0.82} />
                  {durationLabel}
                </span>
              )}
              {key === 'views' && (
                <span className="inline-flex items-center gap-0.5">
                  <YtMetaIcon Icon={LuEye} />
                  {formatCompactNumber(views!)}
                </span>
              )}
              {key === 'likes' && (
                <span className="inline-flex items-center gap-0.5">
                  <YtMetaIcon Icon={LuHeart} optical={0.92} />
                  {formatCompactNumber(likes!)}
                </span>
              )}
            </span>
          ))}
        </div>
      )}
    </div>
  )
})
YoutubeVideoSlide.displayName = 'YoutubeVideoSlide'

export const YoutubeWidget = memo(
  ({ data, showOverview, onContentChange }: any) => {
    const libraryItems = useMemo<YtVideoItem[]>(() => {
      const lib = Array.isArray(data?.library_items) ? data.library_items : []
      const recent = Array.isArray(data?.recent_videos)
        ? data.recent_videos
        : []
      const raw = lib.length > 0 ? lib : recent
      const byId = new Map<string, any>()
      for (const r of recent) {
        if (r?.video_id) byId.set(String(r.video_id), r)
      }
      return raw.map((item: any) => {
        const extra = item?.video_id ? byId.get(String(item.video_id)) : null
        return {
          title: String(item.title || extra?.title || 'Untitled'),
          type: item.type || 'video',
          image: item.image || item.cover || extra?.cover,
          cover: item.cover || item.image || extra?.cover,
          url: item.url || extra?.url,
          video_id: item.video_id || extra?.video_id,
          view_count: item.view_count ?? extra?.view_count,
          like_count: item.like_count ?? extra?.like_count,
          comment_count: item.comment_count ?? extra?.comment_count,
          published_at: item.published_at ?? extra?.published_at,
          duration: item.duration ?? extra?.duration,
        }
      })
    }, [data?.library_items, data?.recent_videos])

    const { currentItem, currentItemIndex } = useLibraryItemRotation(
      libraryItems,
      showOverview,
    )

    useEffect(() => {
      if (!showOverview && currentItem) {
        onContentChange?.({
          title: currentItem.title,
          type: currentItem.type || 'video',
        })
      } else {
        onContentChange?.(null)
      }
    }, [showOverview, currentItem, onContentChange])

    return (
      <AnimatePresence mode="wait">
        {showOverview || !currentItem || libraryItems.length === 0 ? (
          <motion.div
            key="yt-stats"
            initial={CONTENT_FADE_INITIAL}
            animate={CONTENT_FADE_ANIMATE}
            exit={CONTENT_FADE_EXIT}
            transition={CONTENT_FADE_TRANSITION}
            className="h-full w-full"
          >
            <YoutubeStatsWidget data={data} />
          </motion.div>
        ) : (
          <motion.div
            key={`yt-vid-${currentItemIndex}`}
            initial={CONTENT_SLIDE_INITIAL}
            animate={CONTENT_SLIDE_ANIMATE}
            exit={CONTENT_SLIDE_EXIT}
            transition={CONTENT_SLIDE_TRANSITION}
            className="h-full w-full p-1.5"
          >
            <YoutubeVideoSlide item={currentItem} />
          </motion.div>
        )}
      </AnimatePresence>
    )
  },
)
YoutubeWidget.displayName = 'YoutubeWidget'
