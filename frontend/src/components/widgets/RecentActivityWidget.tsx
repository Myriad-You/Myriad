/**
 * 最近活动小组件
 *
 * 展示由后端从平台快照中提炼出的语义事件。原始 JSON 字段路径只保留在
 * metadata_history 审计表，不再进入 UI。
 */

import type { TranslationKeys } from '../../i18n'
import type { WidgetComponentProps } from '../WidgetGrid'

import { motionShim as motion } from '@lib/motionShim'
import { memo, useCallback, useEffect, useMemo, useState } from 'react'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { proxyImageUrl } from '../../utils/proxyImageUrl'
import { RECENT_ACTIVITY_UPDATED_EVENT } from '../../utils/recentActivity'
import PlatformIcon from '../PlatformIcon'
import { PLATFORM_CONFIG } from './reportCard/platformConfig'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetShell } from './shared/WidgetShell'
import { WidgetSkeletonCover } from './shared/WidgetSkeleton'

const CACHE_KEY = 'recent_activities_cache_v4'
const CACHE_DURATION = 60 * 1000
const POLL_INTERVAL = 60 * 1000
/** 小组件只拉/只渲染最近 N 条；角标仍用返回列表长度。 */
const ACTIVITY_LOAD_LIMIT = 8

let globalFetchPromise: Promise<Activity[]> | null = null
let globalCacheData: Activity[] | null = null
let globalCacheTimestamp = 0

interface ActivityChange {
  kind: string
  subject_title?: string
  /** Cover / icon / avatar when the platform snapshot provides one. */
  subject_image?: string | null
  metric?: string
  old?: unknown
  new?: unknown
  delta?: unknown
}

interface Activity {
  platform_name: string
  event_type: 'imported' | 'updated' | 'legacy_updated' | string
  title: string
  changes: ActivityChange[]
  change_count: number
  change_date: string
  legacy?: boolean
}

function metricLabel(metric: string | undefined, t: TranslationKeys): string {
  const labels: Record<string, string> = {
    playtime_minutes: t.recentActivity.playTime,
    rating: t.recentActivity.rating,
    episodes_progress: t.recentActivity.episodesProgress,
    volumes_progress: t.recentActivity.volumesProgress,
    collection_status: t.recentActivity.status,
    stars: t.recentActivity.stargazersCount,
    forks: t.recentActivity.forksCount,
    watchers: t.recentActivity.watchersCount,
    open_issues: t.recentActivity.openIssuesCount,
    followers: t.recentActivity.followers,
    repositories_count: t.recentActivity.repositories,
    contributions: t.recentActivity.contributions,
    progress: t.recentActivity.progress,
    media_count: t.recentActivity.mediaItems,
    liked_songs_count: t.recentActivity.likedSongs,
    following_count: t.recentActivity.following,
    playlists_count: t.recentActivity.playlists,
    level: t.recentActivity.level,
    posts_count: t.recentActivity.posts,
    likes: t.recentActivity.likes,
    achievements_count: t.recentActivity.achievementCount,
    gamerscore: t.recentActivity.gamerscore,
    progress_percent: t.recentActivity.progress,
    servers_count: t.recentActivity.servers,
    connections_count: t.recentActivity.connections,
    trophy_level: t.recentActivity.trophyLevel,
    games_count: t.recentActivity.games,
    collections_count: t.recentActivity.collections,
    subscriptions_count: t.recentActivity.subscriptions,
    anime_count: t.recentActivity.anime,
    manga_count: t.recentActivity.manga,
    data_sections_count: t.recentActivity.dataChanges,
    data_changes: t.recentActivity.dataChanges,
  }
  return labels[metric || ''] || t.recentActivity.dataChanges
}

function formatDuration(minutes: number, t: TranslationKeys): string {
  const rounded = Math.round(Math.abs(minutes))
  if (rounded < 60) {
    return t.recentActivity.minutes.replace('{minutes}', String(rounded))
  }
  const hours = Math.floor(rounded / 60)
  const remainder = rounded % 60
  return t.recentActivity.hoursMinutes
    .replace('{hours}', String(hours))
    .replace('{minutes}', String(remainder))
}

function formatValue(
  value: unknown,
  metric: string | undefined,
  t: TranslationKeys,
): string {
  if (value === null || value === undefined) return ''
  if (metric === 'playtime_minutes' && typeof value === 'number') {
    return formatDuration(value, t)
  }
  if (metric === 'progress_percent' && typeof value === 'number') {
    return `${Math.round(value)}%`
  }
  if (typeof value === 'number') {
    return Number.isInteger(value)
      ? value.toLocaleString()
      : value.toLocaleString(undefined, { maximumFractionDigits: 1 })
  }
  if (typeof value === 'boolean') {
    return value ? t.recentActivity.yes : t.recentActivity.no
  }
  if (typeof value === 'string') return value
  return ''
}

/** Metric / delta only — subject name is secondary (or implied by the thumb). */
function formatChangeCore(change: ActivityChange, t: TranslationKeys): string {
  const subject = change.subject_title?.trim()
  if (change.kind === 'item_added') {
    return t.recentActivity.itemAdded.replace(
      '{subject}',
      subject || t.recentActivity.unknownProject,
    )
  }
  if (change.kind === 'item_removed') {
    return t.recentActivity.itemRemoved.replace(
      '{subject}',
      subject || t.recentActivity.unknownProject,
    )
  }

  const metric = metricLabel(change.metric, t)
  const oldValue = formatValue(change.old, change.metric, t)
  const newValue = formatValue(change.new, change.metric, t)
  const deltaValue =
    typeof change.delta === 'number'
      ? formatValue(Math.abs(change.delta), change.metric, t)
      : ''

  if (change.kind === 'baseline') {
    return `${metric} ${newValue}`.trim()
  }
  if (
    change.metric === 'playtime_minutes' &&
    typeof change.delta === 'number' &&
    change.delta !== 0
  ) {
    return `${metric} ${change.delta > 0 ? '+' : '−'}${deltaValue}`
  }
  if (typeof change.delta === 'number' && change.delta !== 0 && deltaValue) {
    return `${metric} ${change.delta > 0 ? '+' : '−'}${deltaValue}`
  }
  if (oldValue && newValue) {
    return `${metric} ${oldValue} → ${newValue}`
  }
  if (newValue) {
    return `${metric} ${newValue}`
  }
  return metric
}

function firstSubjectImage(activity: Activity): string | undefined {
  for (const change of activity.changes) {
    const url = proxyImageUrl(change.subject_image)
    if (url) return url
  }
  return undefined
}

function primarySubjectTitle(activity: Activity): string | null {
  for (const change of activity.changes) {
    const title = change.subject_title?.trim()
    if (title) return title
  }
  // Multi-change platform rollups used to put the platform label in `title` —
  // skip those so we don't re-surface "Steam" as a subject line.
  const title = activity.title?.trim()
  if (!title) return null
  const platform = activity.platform_name?.trim().toLowerCase()
  if (platform && title.toLowerCase() === platform) return null
  // platform_label style: "Steam", "GitHub", …
  if (/^(steam|github|bilibili|youtube|netease|bangumi|x|discord|myanimelist|mal|xbox|playstation|psn)$/i.test(title)) {
    return null
  }
  if (title === 'NetEase Cloud Music' || title === 'MyAnimeList') return null
  return title
}

/** Stable 0..1 pseudo-random from string (decorations must not reshuffle each render). */
function hash01(seed: string, salt = 0): number {
  let h = (salt * 2654435761) >>> 0
  for (let i = 0; i < seed.length; i++) {
    h = Math.imul(h ^ seed.charCodeAt(i), 16777619)
  }
  return (h >>> 0) / 4294967295
}

function primaryNumericHint(activity: Activity): number {
  const change = activity.changes[0]
  if (!change) return activity.change_count || 1
  if (typeof change.delta === 'number' && Number.isFinite(change.delta)) {
    return Math.abs(change.delta)
  }
  if (typeof change.new === 'number' && Number.isFinite(change.new)) {
    return Math.abs(change.new)
  }
  if (typeof change.old === 'number' && Number.isFinite(change.old)) {
    return Math.abs(change.old)
  }
  return Math.max(1, activity.change_count || 1)
}

function decorationSeed(activity: Activity): string {
  return [
    activity.platform_name,
    activity.change_date,
    activity.title,
    activity.changes[0]?.metric ?? '',
    activity.changes[0]?.subject_title ?? '',
  ].join('|')
}

/**
 * Platform-flavored filler when there is no subject cover.
 * Keeps brand cues (heatmap / waveform / bars) without competing with real media.
 */
const PlatformDecoration = memo(
  ({ activity }: { activity: Activity }) => {
    const platform = activity.platform_name.toLowerCase()
    const seed = decorationSeed(activity)
    const hint = primaryNumericHint(activity)
    const metric = activity.changes[0]?.metric

    if (platform === 'github') {
      // Contribution-style heatmap; lit cells scale with metric magnitude.
      const cols = 7
      const rows = 4
      const total = cols * rows
      // contributions / stars / followers → denser green; cap so it stays readable
      const lit = Math.min(
        total,
        Math.max(
          3,
          metric === 'contributions'
            ? Math.ceil(Math.min(hint, 400) / 20)
            : metric === 'stars' || metric === 'forks'
              ? Math.ceil(Math.min(hint, 80) / 4) + 4
              : Math.ceil(Math.log10(hint + 1) * 6) + 2,
        ),
      )
      // Level classes work in light + dark without double-mounting cells.
      const heatClass = [
        'bg-neutral-200/80 dark:bg-neutral-800/80',
        'bg-emerald-200/90 dark:bg-emerald-900/80',
        'bg-emerald-400/85 dark:bg-emerald-700/85',
        'bg-emerald-500/90 dark:bg-emerald-500/80',
        'bg-emerald-600 dark:bg-emerald-400/90',
      ]
      const cells = Array.from({ length: total }, (_, i) => {
        if (i >= lit) return 0
        // Bias hotter toward the “recent” end of the strip
        const t = i / Math.max(1, lit - 1)
        const base = 1 + Math.floor(t * 3)
        const jitter = Math.floor(hash01(seed, i) * 2)
        return Math.min(4, base + jitter)
      })
      return (
        <div
          className="pointer-events-none absolute inset-0 overflow-hidden"
          aria-hidden
        >
          <div className="absolute inset-0 bg-gradient-to-br from-emerald-500/8 via-transparent to-slate-500/10 dark:from-emerald-500/12 dark:to-white/5" />
          <div
            className="absolute right-1.5 bottom-5 grid gap-[2px]"
            style={{ gridTemplateColumns: `repeat(${cols}, 0.4rem)` }}
          >
            {cells.map((level, i) => (
              <span
                key={i}
                className={`h-1.5 w-1.5 rounded-[1px] ${heatClass[level]}`}
              />
            ))}
          </div>
        </div>
      )
    }

    if (platform === 'x') {
      // Abstract “timeline” strokes; count tracks posts / followers delta.
      const lines = Math.min(6, Math.max(3, Math.ceil(Math.log10(hint + 1) * 3)))
      return (
        <div
          className="pointer-events-none absolute inset-0 overflow-hidden"
          aria-hidden
        >
          <div className="absolute inset-0 bg-gradient-to-br from-neutral-900/5 to-sky-500/8 dark:from-white/5 dark:to-sky-400/10" />
          <div className="absolute right-2 bottom-5 flex w-[42%] flex-col gap-1 opacity-70">
            {Array.from({ length: lines }, (_, i) => (
              <span
                key={i}
                className="h-0.5 rounded-full bg-neutral-800/25 dark:bg-white/25"
                style={{
                  width: `${42 + hash01(seed, i) * 58}%`,
                  marginLeft: `${hash01(seed, i + 9) * 20}%`,
                }}
              />
            ))}
          </div>
        </div>
      )
    }

    if (platform === 'discord') {
      return (
        <div
          className="pointer-events-none absolute inset-0 overflow-hidden"
          aria-hidden
        >
          <div className="absolute -right-3 -bottom-4 h-16 w-16 rounded-full bg-[#5865f2]/25 blur-md" />
          <div className="absolute right-6 bottom-8 h-10 w-10 rounded-full bg-[#5865f2]/18 blur-sm" />
          <div className="absolute right-2 top-3 h-8 w-8 rounded-full bg-[#eb459e]/15 blur-sm" />
        </div>
      )
    }

    if (platform === 'netease' || platform === 'netease_music') {
      // Vinyl disc — music brand cue without fake equalizer bars
      return (
        <div
          className="pointer-events-none absolute inset-0 overflow-hidden"
          aria-hidden
        >
          <div className="absolute inset-0 bg-gradient-to-br from-rose-500/12 via-transparent to-red-900/5" />
          <div className="absolute -right-3 -bottom-4 h-[4.5rem] w-[4.5rem] rounded-full border-[6px] border-rose-500/20 bg-gradient-to-br from-rose-500/15 to-neutral-900/10 dark:border-rose-400/25 dark:from-rose-400/20 dark:to-black/20" />
          <div className="absolute right-3 bottom-2 h-7 w-7 rounded-full border-2 border-rose-500/30 bg-white/40 dark:border-rose-300/35 dark:bg-black/30" />
          <div className="absolute right-5 bottom-4 h-3 w-3 rounded-full bg-rose-500/45 dark:bg-rose-400/50" />
        </div>
      )
    }

    if (platform === 'steam') {
      // Soft playtime strips (cover usually exists; this is fallback)
      const strips = Math.min(5, Math.max(2, Math.ceil(hint / 60)))
      return (
        <div
          className="pointer-events-none absolute inset-0 overflow-hidden"
          aria-hidden
        >
          <div className="absolute inset-0 bg-gradient-to-br from-sky-900/10 via-transparent to-slate-500/10 dark:from-sky-400/10" />
          <div className="absolute right-2 bottom-5 flex w-[48%] flex-col gap-1 opacity-65">
            {Array.from({ length: strips }, (_, i) => (
              <span
                key={i}
                className="h-1 rounded-full bg-sky-600/30 dark:bg-sky-300/35"
                style={{ width: `${55 + hash01(seed, i) * 45}%` }}
              />
            ))}
          </div>
        </div>
      )
    }

    if (platform === 'bilibili') {
      return (
        <div
          className="pointer-events-none absolute inset-0 overflow-hidden"
          aria-hidden
        >
          <div className="absolute inset-0 bg-gradient-to-br from-pink-400/12 via-transparent to-sky-400/12" />
          <div className="absolute -right-2 bottom-2 h-14 w-14 rotate-12 rounded-lg border-2 border-pink-400/25" />
          <div className="absolute right-8 bottom-6 h-8 w-8 -rotate-6 rounded-md border-2 border-sky-400/30" />
        </div>
      )
    }

    if (platform === 'bangumi' || platform === 'mal' || platform === 'myanimelist') {
      // Stacked soft “poster” cards — media library cue, no progress dots
      const isBangumi = platform === 'bangumi'
      const accent = isBangumi
        ? {
            wash: 'from-pink-400/14 to-transparent',
            a: 'border-pink-400/35 bg-pink-400/10',
            b: 'border-pink-300/25 bg-pink-300/8',
            c: 'border-rose-300/20 bg-white/30 dark:bg-white/5',
          }
        : {
            wash: 'from-blue-500/14 to-transparent',
            a: 'border-blue-500/35 bg-blue-500/10',
            b: 'border-blue-400/25 bg-blue-400/8',
            c: 'border-sky-300/20 bg-white/30 dark:bg-white/5',
          }
      return (
        <div
          className="pointer-events-none absolute inset-0 overflow-hidden"
          aria-hidden
        >
          <div
            className={`absolute inset-0 bg-gradient-to-br ${accent.wash}`}
          />
          <div
            className={`absolute right-1 bottom-3 h-11 w-8 rotate-[14deg] rounded-md border ${accent.c}`}
          />
          <div
            className={`absolute right-3 bottom-3.5 h-11 w-8 rotate-[6deg] rounded-md border ${accent.b}`}
          />
          <div
            className={`absolute right-5 bottom-4 h-11 w-8 -rotate-[2deg] rounded-md border shadow-sm ${accent.a}`}
          />
        </div>
      )
    }

    if (platform === 'xbox') {
      const tiles = Math.min(9, Math.max(3, Math.ceil(Math.log10(hint + 1) * 4)))
      return (
        <div
          className="pointer-events-none absolute inset-0 overflow-hidden"
          aria-hidden
        >
          <div className="absolute inset-0 bg-gradient-to-br from-green-600/12 to-transparent" />
          <div className="absolute right-2 bottom-5 grid grid-cols-3 gap-1 opacity-70">
            {Array.from({ length: tiles }, (_, i) => (
              <span
                key={i}
                className="h-2.5 w-2.5 rounded-sm bg-green-600/40 dark:bg-green-400/45"
                style={{ opacity: 0.4 + hash01(seed, i) * 0.6 }}
              />
            ))}
          </div>
        </div>
      )
    }

    if (platform === 'psn' || platform === 'playstation') {
      return (
        <div
          className="pointer-events-none absolute inset-0 overflow-hidden"
          aria-hidden
        >
          <div className="absolute inset-0 bg-gradient-to-br from-blue-600/12 to-transparent" />
          <div className="absolute right-3 bottom-5 flex gap-1 opacity-75">
            {Array.from({ length: 4 }, (_, i) => (
              <span
                key={i}
                className="rounded-full bg-blue-500/40 dark:bg-blue-300/45"
                style={{
                  width: 6 + i * 2,
                  height: 6 + i * 2,
                  opacity: 0.35 + hash01(seed, i) * 0.55,
                }}
              />
            ))}
          </div>
        </div>
      )
    }

    // Generic soft watermark
    return (
      <div
        className="pointer-events-none absolute inset-0 overflow-hidden"
        aria-hidden
      >
        <div className="absolute inset-0 bg-gradient-to-br from-black/[0.03] to-transparent dark:from-white/[0.05]" />
        <div className="absolute -right-1 -bottom-1 opacity-[0.12] dark:opacity-[0.16]">
          <PlatformIcon
            platform={activity.platform_name}
            className="h-14 w-14"
          />
        </div>
      </div>
    )
  },
)

PlatformDecoration.displayName = 'PlatformDecoration'

/** Cover when available; otherwise platform decoration. */
const ActivityCardBackdrop = memo(
  ({
    imageUrl,
    activity,
    onShowCoverChange,
  }: {
    imageUrl?: string
    activity: Activity
    onShowCoverChange: (show: boolean) => void
  }) => {
    const [broken, setBroken] = useState(false)
    const showCover = Boolean(imageUrl) && !broken

    useEffect(() => {
      setBroken(false)
    }, [imageUrl])

    useEffect(() => {
      onShowCoverChange(showCover)
    }, [showCover, onShowCoverChange])

    if (showCover) {
      return (
        <>
          <img
            src={imageUrl}
            alt=""
            draggable={false}
            loading="lazy"
            decoding="async"
            className="absolute inset-0 h-full w-full object-cover"
            onError={() => setBroken(true)}
          />
          <div
            className="absolute inset-0 bg-gradient-to-b from-black/55 via-black/15 to-transparent"
            aria-hidden
          />
        </>
      )
    }

    return <PlatformDecoration activity={activity} />
  },
)

ActivityCardBackdrop.displayName = 'ActivityCardBackdrop'

/** Map activity platform_name → report-card PLATFORM_CONFIG key. */
function platformConfigId(platform: string): string {
  const p = platform.toLowerCase()
  if (p === 'netease_music' || p === 'netease music' || p === '网易云音乐') {
    return 'netease'
  }
  if (p === 'myanimelist') return 'mal'
  if (p === 'playstation') return 'psn'
  if (p === 'twitter' || p === 'x (twitter)') return 'x'
  if (p === 'yt') return 'youtube'
  return p
}

/**
 * Same as report-card CardLogoPill collapsed mark (bg / border / text / icon),
 * only smaller — do not invent alternate chrome.
 */
const PlatformLogoBadge = memo(
  ({ platform, compact }: { platform: string; compact: boolean }) => {
    const id = platformConfigId(platform)
    const config = PLATFORM_CONFIG[id] || PLATFORM_CONFIG.bilibili
    const box = compact ? 16 : 20
    const icon = compact ? 10 : 12

    return (
      <div
        className={`flex shrink-0 items-center justify-center rounded-lg backdrop-blur-sm shadow-lg ${config.textColor}`}
        style={{
          width: box,
          height: box,
          background: config.bgColor,
          border: `1px solid ${config.borderColor}`,
          fontSize: icon,
        }}
      >
        <span
          className="flex items-center justify-center [&>svg]:h-[1em] [&>svg]:w-[1em]"
          style={{ fontSize: icon }}
        >
          {config.icon}
        </span>
      </div>
    )
  },
)

PlatformLogoBadge.displayName = 'PlatformLogoBadge'

const ActivityItem = memo(
  ({
    activity,
    compact,
    t,
  }: {
    activity: Activity
    compact: boolean
    t: TranslationKeys
  }) => {
    // 两列视口下单卡偏窄，只展示首条变化，避免副行挤爆。
    const detailLimit = 1
    const changeLines = useMemo(
      () =>
        activity.changes
          .slice(0, detailLimit)
          .map((change) => formatChangeCore(change, t))
          .filter(Boolean),
      [activity.changes, detailLimit, t],
    )
    const remaining = Math.max(0, activity.change_count - detailLimit)
    const isImported = activity.event_type === 'imported'
    const imageUrl = useMemo(() => firstSubjectImage(activity), [activity])
    const subject = useMemo(() => primarySubjectTitle(activity), [activity])
    // When the primary line already embeds the subject (item_added/removed),
    // don't repeat it on the meta row.
    const showSubjectMeta =
      Boolean(subject) &&
      !changeLines.some((line) => subject && line.includes(subject))
    const [hasCover, setHasCover] = useState(Boolean(imageUrl))
    const onShowCoverChange = useCallback((show: boolean) => {
      setHasCover(show)
    }, [])

    const metaLine = [
      showSubjectMeta ? subject : null,
      changeLines.length > 1 ? changeLines.slice(1).join(' · ') : null,
      remaining > 0 ? `+${remaining}` : null,
    ]
      .filter(Boolean)
      .join(' · ')

    return (
      <motion.div
        initial={{ y: 6, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        className={`relative flex min-w-0 flex-col overflow-hidden rounded-xl border ${
          hasCover
            ? 'border-black/10 dark:border-white/10'
            : 'border-black/4 bg-white/55 dark:border-white/5 dark:bg-white/[0.04]'
        } ${compact ? 'min-h-[5.25rem] p-1.5' : 'min-h-[6.25rem] p-2'}`}
      >
        <ActivityCardBackdrop
          imageUrl={imageUrl}
          activity={activity}
          onShowCoverChange={onShowCoverChange}
        />

        {/* 内容叠在封面 / 装饰之上 */}
        <div className="relative z-10 flex min-h-0 min-w-0 flex-1 flex-col">
          <div className="min-w-0 flex-1">
            <div
              className={`line-clamp-2 font-semibold leading-snug ${
                hasCover
                  ? 'text-white drop-shadow-sm'
                  : 'text-gray-800 dark:text-gray-100'
              } ${compact ? 'text-[11px]' : 'text-xs'}`}
            >
              {changeLines[0] || activity.title}
            </div>
            {(isImported || metaLine) && (
              <div
                className={`mt-0.5 truncate ${
                  hasCover
                    ? 'text-white/85'
                    : 'text-gray-500 dark:text-gray-400'
                } ${compact ? 'text-[9px]' : 'text-[10px]'}`}
              >
                {isImported && (
                  <span
                    className={`mr-1 inline rounded px-1 py-0.5 font-medium ${
                      hasCover ? 'bg-white/20 text-white' : ''
                    }`}
                    style={
                      hasCover
                        ? undefined
                        : {
                            color: 'var(--color-primary)',
                            backgroundColor:
                              'color-mix(in srgb, var(--color-primary) 12%, transparent)',
                          }
                    }
                  >
                    {t.recentActivity.initialImport}
                  </span>
                )}
                {metaLine}
              </div>
            )}
          </div>

          {/* 左下：小平台 logo */}
          <div className="mt-auto flex items-end pt-1.5">
            <PlatformLogoBadge
              platform={activity.platform_name}
              compact={compact}
            />
          </div>
        </div>
      </motion.div>
    )
  },
)

ActivityItem.displayName = 'ActivityItem'

function clearActivityCache(): void {
  globalCacheData = null
  globalCacheTimestamp = 0
  localStorage.removeItem(CACHE_KEY)
}

function loadCachedActivities(): Activity[] | null {
  try {
    const cached = localStorage.getItem(CACHE_KEY)
    if (!cached) return null
    const parsed = JSON.parse(cached) as {
      data?: Activity[]
      timestamp?: number
    }
    if (
      Array.isArray(parsed.data) &&
      typeof parsed.timestamp === 'number' &&
      Date.now() - parsed.timestamp < CACHE_DURATION
    ) {
      return parsed.data
    }
  } catch {
    localStorage.removeItem(CACHE_KEY)
  }
  return null
}

function saveCachedActivities(data: Activity[]): void {
  try {
    localStorage.setItem(
      CACHE_KEY,
      JSON.stringify({ data, timestamp: Date.now() }),
    )
  } catch {
    // The in-memory cache still keeps the widget functional.
  }
}

async function requestActivities(force = false): Promise<Activity[]> {
  const now = Date.now()
  if (
    !force &&
    globalCacheData &&
    now - globalCacheTimestamp < CACHE_DURATION
  ) {
    return globalCacheData
  }
  if (globalFetchPromise) return globalFetchPromise

  globalFetchPromise = (async () => {
    const endpoint = API_URL
      ? `${API_URL}/api/activities?limit=${ACTIVITY_LOAD_LIMIT}`
      : `/api/activities?limit=${ACTIVITY_LOAD_LIMIT}`
    const response = await fetch(endpoint, {
      // This is an intentionally public, read-only projection. Never attach a
      // session cookie to the activity-card request.
      credentials: 'omit',
      cache: 'no-store',
      signal: AbortSignal.timeout(10000),
    })
    if (!response.ok) throw new Error(`HTTP ${response.status}`)
    const body = await response.json()
    const data =
      body.success && Array.isArray(body.activities)
        ? (body.activities as Activity[]).slice(0, ACTIVITY_LOAD_LIMIT)
        : []
    globalCacheData = data
    globalCacheTimestamp = Date.now()
    saveCachedActivities(data)
    return data
  })().finally(() => {
    globalFetchPromise = null
  })

  return globalFetchPromise
}

export const RecentActivityWidget = memo(
  ({ config, isPreview }: WidgetComponentProps) => {
    const { t } = useI18n()
    const anim = useAnimationLevel()
    const compact = config.size === '2x2'
    const [activities, setActivities] = useState<Activity[]>([])
    const [loading, setLoading] = useState(true)

    const refresh = useCallback(async (force = false) => {
      try {
        const data = await requestActivities(force)
        setActivities(data.slice(0, ACTIVITY_LOAD_LIMIT))
      } catch (error) {
        if (error instanceof Error && error.name !== 'AbortError') {
          console.error('Failed to fetch recent activities:', error)
        }
      } finally {
        setLoading(false)
      }
    }, [])

    const visibleActivities = useMemo(
      () => activities.slice(0, ACTIVITY_LOAD_LIMIT),
      [activities],
    )

    useEffect(() => {
      if (isPreview) {
        setActivities([
          {
            platform_name: 'steam',
            event_type: 'updated',
            title: 'Elden Ring',
            changes: [
              {
                kind: 'metric_changed',
                subject_title: 'Elden Ring',
                subject_image:
                  'https://cdn.cloudflare.steamstatic.com/steam/apps/1245620/library_600x900.jpg',
                metric: 'playtime_minutes',
                old: 3200,
                new: 3275,
                delta: 75,
              },
            ],
            change_count: 1,
            change_date: new Date().toISOString(),
          },
          {
            platform_name: 'bangumi',
            event_type: 'updated',
            title: '来自新世界',
            changes: [
              {
                kind: 'progress_changed',
                subject_title: '来自新世界',
                subject_image:
                  'https://lain.bgm.tv/r/200/pic/cover/l/8f/f3/9555_gDGgl.jpg',
                metric: 'episodes_progress',
                old: 8,
                new: 9,
                delta: 1,
              },
            ],
            change_count: 1,
            change_date: new Date(Date.now() - 3600000).toISOString(),
          },
          {
            platform_name: 'github',
            event_type: 'updated',
            title: 'GitHub',
            changes: [
              {
                kind: 'metric_changed',
                metric: 'contributions',
                old: 820,
                new: 864,
                delta: 44,
              },
            ],
            change_count: 1,
            change_date: new Date(Date.now() - 7200000).toISOString(),
          },
          {
            platform_name: 'x',
            event_type: 'updated',
            title: 'X',
            changes: [
              {
                kind: 'metric_changed',
                metric: 'followers',
                old: 1200,
                new: 1218,
                delta: 18,
              },
            ],
            change_count: 1,
            change_date: new Date(Date.now() - 86400000).toISOString(),
          },
        ])
        setLoading(false)
        return
      }

      const cached = loadCachedActivities()
      if (cached) {
        setActivities(cached.slice(0, ACTIVITY_LOAD_LIMIT))
        setLoading(false)
      }
      void refresh(false)

      const handleActivityUpdate = () => {
        clearActivityCache()
        void refresh(true)
      }
      const handleFocus = () => void refresh(false)
      const poll = window.setInterval(() => void refresh(false), POLL_INTERVAL)
      window.addEventListener(
        RECENT_ACTIVITY_UPDATED_EVENT,
        handleActivityUpdate,
      )
      window.addEventListener('focus', handleFocus)
      return () => {
        window.clearInterval(poll)
        window.removeEventListener(
          RECENT_ACTIVITY_UPDATED_EVENT,
          handleActivityUpdate,
        )
        window.removeEventListener('focus', handleFocus)
      }
    }, [isPreview, refresh])

    return (
      <WidgetShell
        padding={compact ? 8 : 12}
        contentClassName="flex min-h-0 flex-col"
        background={
          <GlowBackground
            color="#8b5cf6"
            animLevel={anim.level}
            shouldAnimate={anim.loop}
            variant="single"
            size="md"
          />
        }
      >
        <div
          className={`flex shrink-0 items-center ${compact ? 'mb-1.5 px-0.5' : 'mb-2 px-1'}`}
        >
          <h3
            className={`truncate font-semibold text-gray-700 dark:text-gray-300 ${
              compact ? 'text-[10px]' : 'text-xs'
            }`}
          >
            {t.recentActivity.widgetTitle}
          </h3>
        </div>

        <div className="relative min-h-0 flex-1 touch-pan-y overflow-y-auto overscroll-contain pr-0.5">
          {!loading && visibleActivities.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center text-center text-[10px] text-gray-500 dark:text-gray-400">
              <div className="mb-1.5 text-xl opacity-35">◌</div>
              {t.common.noResults}
            </div>
          ) : (
            <div
              className={`grid grid-cols-2 content-start ${
                compact ? 'gap-1' : 'gap-1.5'
              }`}
            >
              {visibleActivities.map((activity) => (
                <ActivityItem
                  key={`${activity.event_type}-${activity.platform_name}-${activity.change_date}`}
                  activity={activity}
                  compact={compact}
                  t={t}
                />
              ))}
              {visibleActivities.length > 0 ? (
                <p
                  className={`col-span-2 text-center text-gray-400 dark:text-gray-500 ${
                    compact ? 'py-0.5 text-[7px]' : 'py-1 text-[8px]'
                  }`}
                >
                  {t.recentActivity.loadedLimitHint.replace(
                    '{count}',
                    String(ACTIVITY_LOAD_LIMIT),
                  )}
                </p>
              ) : null}
            </div>
          )}
          <WidgetSkeletonCover
            active={loading}
            preset="media-grid"
            count={4}
            columns={2}
            accent="#8b5cf6"
            label={t.common.loading}
          />
        </div>
      </WidgetShell>
    )
  },
)

RecentActivityWidget.displayName = 'RecentActivityWidget'
