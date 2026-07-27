/**
 * 最近活动小组件
 *
 * 展示由后端从平台快照中提炼出的语义事件。原始 JSON 字段路径只保留在
 * metadata_history 审计表，不再进入 UI。
 */

import type { TranslationKeys } from '../../i18n'
import type { WidgetComponentProps } from '../WidgetGrid'

import { LuRefreshCw } from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import { memo, useCallback, useEffect, useMemo, useState } from 'react'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { RECENT_ACTIVITY_UPDATED_EVENT } from '../../utils/recentActivity'
import { useThemeMode } from '../../utils/themeSubscriber'
import PlatformIcon from '../PlatformIcon'
import { Spinner } from '../Spinner'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetShell } from './shared/WidgetShell'

const CACHE_KEY = 'recent_activities_cache_v2'
const CACHE_DURATION = 60 * 1000
const POLL_INTERVAL = 60 * 1000

let globalFetchPromise: Promise<Activity[]> | null = null
let globalCacheData: Activity[] | null = null
let globalCacheTimestamp = 0

interface ActivityChange {
  kind: string
  subject_title?: string
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

interface PlatformTheme {
  color: string
  darkColor: string
}

const PLATFORM_THEMES: Record<string, PlatformTheme> = {
  steam: { color: '#1b2838', darkColor: '#c7d5e0' },
  github: { color: '#24292f', darkColor: '#e6edf3' },
  bilibili: { color: '#00a1d6', darkColor: '#00a1d6' },
  netease: { color: '#e60026', darkColor: '#ff4d67' },
  netease_music: { color: '#e60026', darkColor: '#ff4d67' },
  bangumi: { color: '#f09199', darkColor: '#f6a9b0' },
  x: { color: '#111111', darkColor: '#e7e9ea' },
  discord: { color: '#5865f2', darkColor: '#8b9cff' },
  mal: { color: '#2e51a2', darkColor: '#8ba6f8' },
  myanimelist: { color: '#2e51a2', darkColor: '#8ba6f8' },
  xbox: { color: '#107c10', darkColor: '#72d35c' },
  psn: { color: '#0070d1', darkColor: '#65b5ff' },
  playstation: { color: '#0070d1', darkColor: '#65b5ff' },
}

const FALLBACK_PLATFORM_THEME: PlatformTheme = {
  color: '#64748b',
  darkColor: '#cbd5e1',
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

function formatChange(change: ActivityChange, t: TranslationKeys): string {
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
  const prefix = subject ? `${subject} · ` : ''
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
    return `${prefix}${metric} ${change.delta > 0 ? '+' : '−'}${deltaValue}`
  }
  if (oldValue && newValue) {
    return `${prefix}${metric} ${oldValue} → ${newValue}`
  }
  if (newValue) {
    return `${prefix}${metric} ${newValue}`
  }
  return `${prefix}${metric}`
}

function formatTimeAgo(dateString: string, t: TranslationKeys): string {
  const date = new Date(dateString.replace(' ', 'T'))
  const diff = Math.max(0, Date.now() - date.getTime())
  const hours = Math.floor(diff / 3600000)
  const days = Math.floor(hours / 24)
  if (days > 7) {
    return date.toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
    })
  }
  if (days > 0) {
    return t.recentActivity.daysAgo.replace('{days}', String(days))
  }
  if (hours > 0) {
    return t.recentActivity.hoursAgo.replace('{hours}', String(hours))
  }
  return t.recentActivity.justNow
}

const ActivityItem = memo(
  ({
    activity,
    compact,
    isDark,
    t,
  }: {
    activity: Activity
    compact: boolean
    isDark: boolean
    t: TranslationKeys
  }) => {
    const details = useMemo(
      () => activity.changes.map((change) => formatChange(change, t)),
      [activity.changes, t],
    )
    const detailLimit = compact ? 1 : 2
    const remaining = Math.max(0, activity.change_count - detailLimit)
    const isImported = activity.event_type === 'imported'
    const platformTheme =
      PLATFORM_THEMES[activity.platform_name.toLowerCase()] ||
      FALLBACK_PLATFORM_THEME
    const platformColor = isDark ? platformTheme.darkColor : platformTheme.color

    return (
      <motion.div
        initial={{ x: -8, opacity: 0 }}
        animate={{ x: 0, opacity: 1 }}
        className={`flex items-start rounded-lg border border-black/4 bg-white/45 dark:border-white/5 dark:bg-white/3 ${
          compact ? 'gap-1.5 p-1.5' : 'gap-2 p-2'
        }`}
      >
        <div
          className={`mt-0.5 flex shrink-0 items-center justify-center rounded-md bg-white/65 shadow-sm dark:bg-white/8 ${
            compact ? 'h-6 w-6' : 'h-7 w-7'
          }`}
          style={{ color: platformColor }}
        >
          <PlatformIcon
            platform={activity.platform_name}
            className={compact ? 'h-3.5 w-3.5' : 'h-4 w-4'}
          />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-1.5">
            <div
              className={`min-w-0 flex-1 truncate font-semibold text-gray-800 dark:text-gray-100 ${
                compact ? 'text-[10px]' : 'text-[11px]'
              }`}
            >
              {activity.title}
            </div>
            <span className="shrink-0 text-[8px] text-gray-400 dark:text-gray-500">
              {formatTimeAgo(activity.change_date, t)}
            </span>
          </div>
          <div
            className={`mt-0.5 truncate text-gray-600 dark:text-gray-400 ${
              compact ? 'text-[8px]' : 'text-[9px]'
            }`}
          >
            {isImported && (
              <span className="mr-1 rounded bg-violet-500/10 px-1 py-0.5 text-violet-600 dark:text-violet-300">
                {t.recentActivity.initialImport}
              </span>
            )}
            {details.slice(0, detailLimit).join(' · ')}
            {remaining > 0 && ` +${remaining}`}
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
      ? `${API_URL}/api/activities?limit=12`
      : '/api/activities?limit=12'
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
      body.success && Array.isArray(body.activities) ? body.activities : []
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
    const isDark = useThemeMode()
    const compact = config.size === '2x2'
    const [activities, setActivities] = useState<Activity[]>([])
    const [loading, setLoading] = useState(true)
    const [refreshing, setRefreshing] = useState(false)

    const refresh = useCallback(async (force = false) => {
      if (force) setRefreshing(true)
      try {
        const data = await requestActivities(force)
        setActivities(data)
      } catch (error) {
        if (error instanceof Error && error.name !== 'AbortError') {
          console.error('Failed to fetch recent activities:', error)
        }
      } finally {
        setLoading(false)
        setRefreshing(false)
      }
    }, [])

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
                metric: 'episodes_progress',
                old: 8,
                new: 9,
                delta: 1,
              },
            ],
            change_count: 1,
            change_date: new Date(Date.now() - 3600000).toISOString(),
          },
        ])
        setLoading(false)
        return
      }

      const cached = loadCachedActivities()
      if (cached) {
        setActivities(cached)
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
          className={`flex shrink-0 items-center justify-between ${compact ? 'mb-1.5 px-0.5' : 'mb-2 px-1'}`}
        >
          <div className="flex min-w-0 items-center gap-1.5">
            <h3
              className={`truncate font-semibold text-gray-700 dark:text-gray-300 ${
                compact ? 'text-[10px]' : 'text-xs'
              }`}
            >
              {t.recentActivity.widgetTitle}
            </h3>
            {!loading && activities.length > 0 && (
              <span className="rounded-full bg-violet-500/10 px-1.5 py-0.5 text-[8px] font-medium text-violet-600 dark:text-violet-300">
                {activities.length}
              </span>
            )}
          </div>
          {!isPreview && (
            <button
              type="button"
              onClick={() => void refresh(true)}
              disabled={refreshing}
              aria-label={t.common.refresh}
              className="flex h-5 w-5 items-center justify-center rounded-full text-gray-400 transition-colors hover:bg-black/5 hover:text-violet-500 disabled:opacity-50 dark:hover:bg-white/8"
            >
              {refreshing ? (
                <Spinner size="xs" color="current" />
              ) : (
                <LuRefreshCw className="h-3 w-3" />
              )}
            </button>
          )}
        </div>

        <div
          className={`min-h-0 flex-1 touch-pan-y overflow-y-auto overscroll-contain pr-0.5 ${compact ? 'space-y-1' : 'space-y-1.5'}`}
        >
          {loading ? (
            <div
              className="flex h-full items-center justify-center"
              role="status"
            >
              <Spinner size="sm" color="primary" />
            </div>
          ) : activities.length === 0 ? (
            <div className="flex h-full flex-col items-center justify-center text-center text-[10px] text-gray-500 dark:text-gray-400">
              <div className="mb-1.5 text-xl opacity-35">◌</div>
              {t.common.noResults}
            </div>
          ) : (
            activities.map((activity) => (
              <ActivityItem
                key={`${activity.event_type}-${activity.platform_name}-${activity.change_date}`}
                activity={activity}
                compact={compact}
                isDark={isDark}
                t={t}
              />
            ))
          )}
        </div>
      </WidgetShell>
    )
  },
)

RecentActivityWidget.displayName = 'RecentActivityWidget'
