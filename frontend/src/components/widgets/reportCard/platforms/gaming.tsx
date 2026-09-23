import type { AnimationConfig } from '../../../../hooks/useAnimationLevel'
import type { SteamPresence } from '../types'
import { FaBolt, FaPlay, FaSteam, FaXbox, SiPlaystation } from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo, useEffect, useMemo, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { useAnimationLevel } from '../../../../hooks/useAnimationLevel'
import { apiService } from '../../../../services/api'
import { proxyImageUrl } from '../../../../utils/proxyImageUrl'
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
import { formatCompactNumber } from '../format'
import { useCountUp, useLibraryItemRotation } from '../hooks'
import { normalizeHttpsMediaUrl, normalizeXboxMediaUrl } from '../media'
import { fetchPlatformUserIds } from '../platformSocial'

let cachedSteamPresence: SteamPresence | null = null
let cachedSteamPresenceAt = 0
let steamPresencePromise: Promise<SteamPresence | null> | null = null
async function fetchSteamPresence(
  maxAgeMs = 45 * 1000,
): Promise<SteamPresence | null> {
  if (cachedSteamPresence && Date.now() - cachedSteamPresenceAt < maxAgeMs) {
    return cachedSteamPresence
  }
  if (steamPresencePromise) return steamPresencePromise

  steamPresencePromise = (async () => {
    try {
      const body = await apiService.get<{ success?: boolean, data?: unknown }>(
        '/steam/presence',
        { timeout: 10_000 },
      )
      if (body?.success && body?.data) {
        cachedSteamPresence = body.data as SteamPresence
        cachedSteamPresenceAt = Date.now()
        return cachedSteamPresence
      }
      cachedSteamPresence = null
      cachedSteamPresenceAt = 0
    } catch {
      cachedSteamPresence = null
      cachedSteamPresenceAt = 0
    } finally {
      steamPresencePromise = null
    }
    return null
  })()

  return steamPresencePromise
}

interface XboxPresence {
  gamertag?: string | null
  avatar?: string | null
  is_online?: boolean
  is_in_game?: boolean
  game_title?: string | null
  status?: string | null
  gamerscore?: number | null
  degraded?: boolean
  degrade_reason?: string | null
}

const xboxPresenceCache = new Map<string, { data: XboxPresence; at: number }>()
const xboxPresenceInflight = new Map<string, Promise<XboxPresence | null>>()
const MAX_PRESENCE_CACHE = 20

function setPresenceCache<T>(
  cache: Map<string, { data: T; at: number }>,
  key: string,
  data: T,
): void {
  cache.delete(key)
  cache.set(key, { data, at: Date.now() })
  while (cache.size > MAX_PRESENCE_CACHE) {
    const oldest = cache.keys().next().value
    if (oldest === undefined) break
    cache.delete(oldest)
  }
}

async function fetchXboxPresence(
  gamertag: string,
  maxAgeMs = 60 * 1000,
): Promise<XboxPresence | null> {
  const key = gamertag.trim()
  if (!key) return null
  const cached = xboxPresenceCache.get(key)
  if (cached && Date.now() - cached.at < maxAgeMs) return cached.data
  const inflight = xboxPresenceInflight.get(key)
  if (inflight) return inflight

  const promise = (async () => {
    try {
      // Any failure lands in the catch below: drop the cache entry, report offline-unknown.
      const body = await apiService.get<{ success?: boolean, data?: any }>('/game/presence', {
        params: { platform: 'xbox', id: key },
        timeout: 12_000,
      })
      const d = body?.data
      if (!body?.success || !d) {
        xboxPresenceCache.delete(key)
        return null
      }
      const degraded = Boolean(d.degraded)
      const status = String(d?.presence?.status || '').toLowerCase()
      const title = d?.presence?.title ? String(d.presence.title) : null
      const isOnline = degraded
        ? false
        : status === 'online' || status === 'away' || status === 'busy'
      const isInGame =
        !degraded && isOnline && Boolean(title && title !== 'Home')
      const gsRaw = d?.score?.value
      const gs =
        typeof gsRaw === 'string' || typeof gsRaw === 'number'
          ? Number(gsRaw)
          : null
      const presence: XboxPresence = {
        gamertag: d?.identity?.name || key,
        avatar: d?.identity?.avatar || null,
        is_online: isOnline,
        is_in_game: isInGame,
        game_title: isInGame ? title : null,
        status: degraded
          ? 'degraded'
          : d?.presence?.status || null,
        gamerscore: Number.isFinite(gs as number) ? (gs as number) : null,
        degraded,
        degrade_reason: d.degrade_reason
          ? String(d.degrade_reason)
          : null,
      }
      setPresenceCache(xboxPresenceCache, key, presence)
      return presence
    } catch {
      xboxPresenceCache.delete(key)
      return null
    } finally {
      xboxPresenceInflight.delete(key)
    }
  })()
  xboxPresenceInflight.set(key, promise)
  return promise
}

function getSteamPresenceFromData(data: any): SteamPresence | null {
  if (!data) return null
  if (
    data.personastate === undefined &&
    data.persona_state === undefined &&
    data.personastate_label === undefined &&
    data.online_status === undefined &&
    data.gameextrainfo === undefined &&
    data.avatar === undefined
  ) {
    return null
  }

  const personastate = data.personastate ?? data.persona_state
  const state =
    typeof personastate === 'number'
      ? personastate
      : Number.isFinite(Number(personastate))
        ? Number(personastate)
        : undefined
  const gameextrainfo =
    typeof data.gameextrainfo === 'string' ? data.gameextrainfo : null
  const gameid =
    typeof data.gameid === 'string' || typeof data.gameid === 'number'
      ? String(data.gameid)
      : null

  return {
    personastate: state,
    personastate_label:
      typeof data.personastate_label === 'string'
        ? data.personastate_label
        : typeof data.online_status === 'string'
          ? data.online_status
          : undefined,
    is_online:
      typeof data.is_online === 'boolean'
        ? data.is_online
        : state !== undefined
          ? state !== 0
          : undefined,
    is_in_game:
      typeof data.is_in_game === 'boolean'
        ? data.is_in_game
        : Boolean(gameextrainfo || gameid),
    gameextrainfo,
    gameid,
    avatar: typeof data.avatar === 'string' ? data.avatar : null,
    personaname: typeof data.personaname === 'string' ? data.personaname : null,
    recent_2weeks_minutes:
      typeof data.recent_2weeks_minutes === 'number'
        ? data.recent_2weeks_minutes
        : null,
  }
}

function getSteamPresenceText(
  presence: SteamPresence | null,
  t: ReturnType<typeof useI18n>['t'],
): string | null {
  if (!presence) return null
  if (presence.is_in_game && presence.gameextrainfo) {
    return `${t.reportCardWidget.steamPlaying}: ${presence.gameextrainfo}`
  }

  switch (presence.personastate_label) {
    case 'online':
      return t.reportCardWidget.steamOnline
    case 'busy':
      return t.reportCardWidget.steamBusy
    case 'away':
      return t.reportCardWidget.steamAway
    case 'snooze':
      return t.reportCardWidget.steamSnooze
    case 'looking_to_trade':
      return t.reportCardWidget.steamLookingToTrade
    case 'looking_to_play':
      return t.reportCardWidget.steamLookingToPlay
    case 'offline':
      return t.reportCardWidget.steamOffline
    default:
      if (presence.is_online === true) return t.reportCardWidget.steamOnline
      if (presence.is_online === false) return t.reportCardWidget.steamOffline
      return t.reportCardWidget.steamStatusUnknown
  }
}

function getSteamPresenceColor(presence: SteamPresence | null): string {
  if (!presence) return '#9ca3af'
  if (presence.is_in_game) return '#3b82f6'

  switch (presence.personastate_label) {
    case 'online':
      return '#22c55e'
    case 'busy':
      return '#ef4444'
    case 'away':
    case 'snooze':
      return '#f59e0b'
    case 'looking_to_trade':
    case 'looking_to_play':
      return '#8b5cf6'
    default:
      return presence.is_online ? '#22c55e' : '#9ca3af'
  }
}

const SCORE_BAR_SEGMENTS = 10

export const ScoreCardBody = memo(
  ({
    score,
    type,
    anim,
  }: {
    score: number
    type: string
    anim: AnimationConfig
  }) => {
    const { t } = useI18n()
    const displayScore = useCountUp(
      score,
      Math.round(900 * anim.durationScale),
      300,
    )
    const pct = Math.min(Math.max(displayScore, 0), 100)

    return (
      <>
        <motion.div
          className="flex items-center justify-between gap-2"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3, delay: 0.12 }}
        >
          <span className="flex shrink-0 items-center gap-1">
            <FaBolt className="h-2.5 w-2.5 shrink-0 text-[#66c0f4]" />
            <span className="bg-linear-to-r from-gray-700 to-[#417a9b] bg-clip-text text-[11px] font-black italic tracking-tight text-transparent dark:from-gray-100 dark:to-[#66c0f4]">
              {t.reportCardWidget.steamGamingScore}
            </span>
          </span>
          <div className="flex h-1.5 w-16 shrink-0 gap-[3px]">
            {Array.from({ length: SCORE_BAR_SEGMENTS }).map((_, i) => {
              const lit = i < Math.round((pct / 100) * SCORE_BAR_SEGMENTS)
              return (
                <div
                  key={i}
                  className={`h-full flex-1 rounded-[2px] transition-colors duration-150 ${
                    lit
                      ? 'bg-linear-to-b from-[#66c0f4] to-[#417a9b] shadow-[0_0_6px_rgba(102,192,244,0.5)]'
                      : 'bg-black/8 dark:bg-white/10'
                  }`}
                />
              )
            })}
          </div>
        </motion.div>

        <motion.div
          className="flex items-center justify-between gap-2.5"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3, delay: 0.2 }}
        >
          <span className="flex shrink-0 items-baseline gap-0.5">
            <span className="text-3xl font-black leading-none tracking-tight tabular-nums text-gray-800 dark:text-gray-100">
              {displayScore}
            </span>
            <span className="text-[11px] font-bold text-gray-500 dark:text-gray-400">
              /100
            </span>
          </span>
          <motion.span
            className="inline-flex min-w-0 items-center gap-1.5 bg-gray-800/90 py-1 pl-2.5 pr-3 dark:bg-white/90"
            style={{
              clipPath:
                'polygon(0 0, calc(100% - 7px) 0, 100% 7px, 100% 100%, 0 100%)',
            }}
            initial={{ opacity: 0, scale: 0.85 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={
              anim.spring
                ? { type: 'spring', stiffness: 300, damping: 20, delay: 0.22 }
                : { duration: 0.25, delay: 0.22 }
            }
          >
            <span
              className={`h-1.5 w-1.5 shrink-0 rounded-full bg-[#66c0f4] ${anim.loop ? 'animate-pulse' : ''}`}
            />
            <span className="truncate text-[10px] font-bold uppercase tracking-wide text-gray-100 dark:text-black">
              {type}
            </span>
          </motion.span>
        </motion.div>
      </>
    )
  },
)
ScoreCardBody.displayName = 'ScoreCardBody'

export const SteamStatsWidget = memo(({ data, isPreview }: any) => {
  const { t } = useI18n()
  const anim = useAnimationLevel()
  const fallbackPresence = useMemo(() => getSteamPresenceFromData(data), [data])
  const [livePresence, setLivePresence] = useState<SteamPresence | null>(null)
  const score = useMemo(() => data?.hardcore_score || 0, [data])
  const type = useMemo(() => {
    const raw = String(data?.player_type || '')
      .trim()
      .toLowerCase()
    if (
      raw === 'hardcore' ||
      raw.includes('硬核') ||
      raw.includes('hardcore')
    ) {
      return t.reportCardWidget.hardcorePlayer || t.reportCard.hardcorePlayer
    }
    if (
      raw === 'casual' ||
      raw.includes('休闲') ||
      raw.includes('casual')
    ) {
      return t.reportCard.casualPlayer
    }
    if (raw === 'balanced' || raw.includes('均衡')) {
      return t.reportCardWidget.balancedPlayer || t.reportCard.casualPlayer
    }
    if (raw) return String(data.player_type)
    return t.reportCard.casualPlayer
  }, [data, t])
  const gamesCount = useMemo(() => {
    const n = Number(data?.games_count)
    if (!Number.isFinite(n) || n < 0) return 0
    return Math.round(n)
  }, [data])
  const totalPlaytime = useMemo(() => {
    const hours = Number(data?.total_playtime)
    if (!Number.isFinite(hours) || hours < 0) return '0'
    const h = Math.round(hours)
    return h >= 1000 ? `${(h / 1000).toFixed(1)}k` : String(h)
  }, [data])
  const presence = livePresence ?? fallbackPresence
  const presenceText = useMemo(
    () => getSteamPresenceText(presence, t),
    [presence, t],
  )
  const presenceColor = useMemo(
    () => getSteamPresenceColor(presence),
    [presence],
  )
  const avatarUrl = useMemo(() => {
    const raw = presence?.avatar?.trim()
    if (!raw) return null
    if (raw.includes('/api/proxy/image')) return proxyImageUrl(raw) ?? raw
    const full = raw.replaceAll(
      /(_full|_medium)?\.(jpg|png)(\?.*)?$/gi,
      '_full.$2$3',
    )
    return proxyImageUrl(full) ?? full
  }, [presence])
  const isLive = Boolean(presence?.is_online || presence?.is_in_game)
  const nowPlaying =
    presence?.is_in_game && presence?.gameextrainfo
      ? presence.gameextrainfo
      : null
  const gameIconUrl =
    nowPlaying && presence?.gameid
      ? proxyImageUrl(
          `https://cdn.cloudflare.steamstatic.com/steam/apps/${presence.gameid}/header.jpg`,
        )
      : null
  const recent2wHours = useMemo(() => {
    const minutes = presence?.recent_2weeks_minutes
    if (typeof minutes !== 'number' || minutes <= 0) return null
    const hours = minutes / 60
    return hours >= 10 ? Math.round(hours).toString() : hours.toFixed(1)
  }, [presence])
  const statItems = useMemo(
    () => [
      { label: t.reportsPage.library, value: String(gamesCount), unit: '' },
      { label: t.reportsPage.playtime, value: totalPlaytime, unit: 'H' },
      {
        label: t.reportCardWidget.steamRecent2w,
        value: recent2wHours ?? '0',
        unit: 'H',
      },
    ],
    [t, gamesCount, totalPlaytime, recent2wHours],
  )
  const [slotIndex, setSlotIndex] = useState(0)
  useEffect(() => {
    if (!nowPlaying || !anim.loop) {
      setSlotIndex(nowPlaying ? 1 : 0)
      return
    }
    setSlotIndex(1)
    let cancelled = false
    let timeoutId: number | null = null
    const tick = () => {
      if (cancelled || document.hidden) return
      setSlotIndex((prev) => (prev === 0 ? 1 : 0))
      timeoutId = window.setTimeout(tick, 6000)
    }
    timeoutId = window.setTimeout(tick, 6000)

    const onVisibility = () => {
      if (document.hidden && timeoutId) {
        clearTimeout(timeoutId)
        timeoutId = null
      } else if (!document.hidden && !cancelled && !timeoutId) {
        timeoutId = window.setTimeout(tick, 6000)
      }
    }
    document.addEventListener('visibilitychange', onVisibility)

    return () => {
      cancelled = true
      if (timeoutId) clearTimeout(timeoutId)
      document.removeEventListener('visibilitychange', onVisibility)
    }
  }, [nowPlaying, anim.loop])
  const showNowPlaying = Boolean(nowPlaying) && slotIndex === 1

  useEffect(() => {
    if (isPreview) return
    let cancelled = false

    const refreshPresence = async () => {
      if (document.hidden) return
      const nextPresence = await fetchSteamPresence()
      if (cancelled) return
      setLivePresence(nextPresence)
    }

    refreshPresence()
    const intervalId = window.setInterval(refreshPresence, 120 * 1000)

    return () => {
      cancelled = true
      window.clearInterval(intervalId)
    }
  }, [isPreview])

  return (
    <div className="relative h-full w-full overflow-hidden">
      <div className="absolute inset-0 bg-linear-to-br from-[#66c0f4]/25 via-[#66c0f4]/8 to-transparent dark:from-[#66c0f4]/15 dark:via-[#66c0f4]/5 clip-diagonal" />

      <div className="relative z-10 flex h-full flex-col justify-between p-4">
        <motion.div
          className="flex min-w-0 items-center gap-3"
          initial={{ x: -12, opacity: 0 }}
          animate={{ x: 0, opacity: 1 }}
          transition={{ duration: 0.45 }}
        >
          <motion.div
            className="relative shrink-0"
            initial={{ scale: 0.7, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            whileHover={{ scale: 1.05 }}
            transition={
              anim.spring
                ? { type: 'spring', stiffness: 260, damping: 18, delay: 0.1 }
                : { duration: 0.35, delay: 0.1 }
            }
            title={presenceText ?? undefined}
          >
            {avatarUrl ? (
              <img
                src={avatarUrl}
                alt={presence?.personaname || 'Steam'}
                className="h-11 w-11 rounded-lg object-cover shadow-md ring-1 ring-black/10 dark:ring-white/15"
                loading="lazy"
                decoding="async"
              />
            ) : (
              <div className="flex h-11 w-11 items-center justify-center rounded-lg bg-gray-200/70 ring-1 ring-black/10 dark:bg-white/10 dark:ring-white/15">
                <FaSteam className="h-5 w-5 text-gray-400 dark:text-gray-500" />
              </div>
            )}
            {presence && (
              <span className="absolute -bottom-0.5 -right-0.5 h-3 w-3">
                {isLive && anim.loop && (
                  <span
                    className="absolute inset-0 rounded-full opacity-40 animate-ping"
                    style={{ backgroundColor: presenceColor }}
                  />
                )}
                <span
                  className="absolute inset-0 rounded-full border-2 border-white dark:border-gray-900"
                  style={{ backgroundColor: presenceColor }}
                />
              </span>
            )}
          </motion.div>
          <div className="flex min-w-0 flex-col gap-1.5">
            {presence?.personaname && (
              <span
                className="truncate text-base font-black tracking-tight text-gray-800 dark:text-gray-100"
                style={{ WebkitTextStroke: '0.4px currentcolor' }}
              >
                {presence.personaname}
              </span>
            )}
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5">
              {statItems.map((item, i) => (
                <motion.span
                  key={item.label}
                  className="flex items-baseline gap-1"
                  initial={{ y: 6, opacity: 0 }}
                  animate={{ y: 0, opacity: 1 }}
                  transition={{ duration: 0.35, delay: 0.3 + i * 0.08 }}
                >
                  <span className="flex items-baseline gap-0.5">
                    <span className="text-[11px] font-black leading-none text-gray-800 dark:text-gray-100">
                      {item.value}
                    </span>
                    {item.unit && (
                      <span className="text-[8px] font-bold text-gray-500 dark:text-gray-400">
                        {item.unit}
                      </span>
                    )}
                  </span>
                  <span className="text-[8px] font-bold text-gray-400 dark:text-gray-500">
                    {item.label}
                  </span>
                </motion.span>
              ))}
            </div>
          </div>
        </motion.div>

        <div className="translate-y-[3px] pl-13">
          <div className="relative h-16">
            <AnimatePresence mode="wait">
              {showNowPlaying ? (
                <motion.div
                  key="playing"
                  className="absolute inset-0 overflow-hidden rounded-lg shadow-sm ring-1 ring-black/10 dark:ring-white/15"
                  initial={{ opacity: 0, y: 12 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -12 }}
                  transition={{ duration: 0.4 }}
                  title={t.reportCardWidget.steamPlaying}
                >
                  {gameIconUrl ? (
                    <img
                      src={gameIconUrl}
                      alt=""
                      className="absolute inset-0 h-full w-full object-cover"
                      loading="lazy"
                    />
                  ) : (
                    <div className="absolute inset-0 flex items-center justify-center bg-[#1b2838]">
                      <FaSteam className="h-6 w-6 text-white/40" />
                    </div>
                  )}
                  <div className="absolute inset-0 bg-linear-to-t from-black/75 via-black/25 to-transparent" />
                  <div className="absolute inset-x-0 bottom-0 flex items-center gap-1.5 p-1.5">
                    <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-white text-gray-900 shadow-md">
                      <FaPlay className="h-2 w-2 translate-x-px" />
                    </span>
                    <span className="truncate text-[11px] font-bold text-white drop-shadow-sm">
                      {nowPlaying}
                    </span>
                  </div>
                </motion.div>
              ) : (
                <motion.div
                  key="score"
                  className="absolute inset-0 flex flex-col justify-center gap-1 rounded-lg glass-surface glass-45 px-3.5 ring-1 ring-black/5 dark:ring-white/10"
                  initial={{ opacity: 0, y: 12 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -12 }}
                  transition={{ duration: 0.4 }}
                >
                  <ScoreCardBody score={score} type={type} anim={anim} />
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        </div>
      </div>
    </div>
  )
})

export const SteamWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const libraryItems = useMemo(() => data?.library_items || [], [data])
  const { currentItem, currentItemIndex } = useLibraryItemRotation(
    libraryItems,
    showOverview,
  )

  useEffect(() => {
    if (!showOverview && currentItem) {
      onContentChange?.({ title: currentItem.title, type: 'game' })
    } else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItem, onContentChange])

  return (
    <AnimatePresence mode="wait">
      {showOverview || !currentItem ? (
        <motion.div
          key="stats"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full"
        >
          <SteamStatsWidget data={data} />
        </motion.div>
      ) : (
        <motion.div
          key={`lib-${currentItemIndex}`}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -10 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full p-1.5"
        >
          <div className="relative h-full w-full rounded-lg overflow-hidden shadow-lg bg-white dark:bg-black/90">
            <div className="absolute inset-0">
              <img
                src={
                  currentItem.cover ||
                  `https://ui-avatars.com/api/?name=${encodeURIComponent(currentItem.title)}&size=400&background=1b2838&color=fff`
                }
                alt={currentItem.title}
                className="w-full h-full object-cover"
                loading="lazy"
                referrerPolicy="no-referrer"
              />
              <div className="absolute inset-0 bg-linear-to-t from-black/80 via-black/40 to-transparent" />
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
})

export const TrophyTitleRow = memo(
  ({
    title,
    accent,
  }: {
    title: {
      name: string
      progress?: number
      platinum?: boolean
      gamerscore?: number
    }
    accent: string
  }) => (
    <div className="flex items-center gap-2 min-w-0">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 min-w-0">
          {title.platinum && (
            <span className="shrink-0 text-[10px]" title="Platinum">
              🏆
            </span>
          )}
          <span className="truncate text-xs font-medium text-gray-800 dark:text-gray-100">
            {title.name}
          </span>
          <span
            className="ml-auto shrink-0 text-[10px] tabular-nums font-semibold"
            style={{ color: accent }}
          >
            {Math.round(title.progress ?? 0)}%
          </span>
        </div>
        <div className="mt-1 h-1 rounded-full bg-black/10 dark:bg-white/10 overflow-hidden">
          <motion.div
            className="h-full rounded-full"
            style={{ background: accent }}
            initial={{ width: 0 }}
            animate={{
              width: `${Math.min(100, Math.max(0, title.progress ?? 0))}%`,
            }}
            transition={{ duration: 0.8, ease: 'easeOut' }}
          />
        </div>
      </div>
    </div>
  ),
)

TrophyTitleRow.displayName = 'TrophyTitleRow'

export const AchievementReportBody = memo(
  ({
    icon,
    accent,
    typeLabel,
    scoreValue,
    scoreLabel,
    stats,
    topTitles,
    showOverview,
    onContentChange,
  }: {
    icon: React.ReactNode
    accent: string
    typeLabel: string
    scoreValue: string
    scoreLabel: string
    stats: { label: string; value: string }[]
    topTitles: { name: string; progress?: number; platinum?: boolean }[]
    showOverview: boolean
    onContentChange?: (content: { titles?: string[] } | null) => void
  }) => {
    const [pageIndex, setPageIndex] = useState(0)
    const PAGE_SIZE = 3
    const pageCount = Math.max(1, Math.ceil(topTitles.length / PAGE_SIZE))

    useEffect(() => {
      if (showOverview || topTitles.length <= PAGE_SIZE) return
      const timer = window.setInterval(() => {
        setPageIndex((prev) => (prev + 1) % pageCount)
      }, 5000)
      return () => window.clearInterval(timer)
    }, [showOverview, topTitles.length, pageCount])

    const currentTitles = useMemo(
      () =>
        topTitles.slice(
          pageIndex * PAGE_SIZE,
          pageIndex * PAGE_SIZE + PAGE_SIZE,
        ),
      [topTitles, pageIndex],
    )

    useEffect(() => {
      if (!showOverview && currentTitles.length > 0) {
        onContentChange?.({ titles: currentTitles.map((t) => t.name) })
      } else {
        onContentChange?.(null)
      }
    }, [showOverview, currentTitles, onContentChange])

    return (
      <AnimatePresence mode="wait">
        {showOverview || currentTitles.length === 0 ? (
          <motion.div
            key="stats"
            initial={CONTENT_FADE_INITIAL}
            animate={CONTENT_FADE_ANIMATE}
            exit={CONTENT_FADE_EXIT}
            transition={CONTENT_FADE_TRANSITION}
            className="h-full w-full p-3 flex flex-col justify-between"
          >
            <div className="flex items-start justify-between gap-2">
              <div className="min-w-0">
                <div
                  className="flex items-center gap-1.5 text-sm font-bold"
                  style={{ color: accent }}
                >
                  {icon}
                  <span className="truncate">{typeLabel}</span>
                </div>
                <div className="mt-1.5 flex items-baseline gap-1.5">
                  <span className="text-3xl font-black tabular-nums text-gray-900 dark:text-gray-50 leading-none">
                    {scoreValue}
                  </span>
                  <span className="text-[10px] text-gray-500 dark:text-gray-400">
                    {scoreLabel}
                  </span>
                </div>
              </div>
            </div>

            <div className="grid grid-cols-3 gap-1.5">
              {stats.slice(0, 3).map((s) => (
                <div
                  key={s.label}
                  className="rounded-lg px-2 py-1.5 bg-black/5 dark:bg-white/5"
                >
                  <div
                    className="text-sm font-bold tabular-nums"
                    style={{ color: accent }}
                  >
                    {s.value}
                  </div>
                  <div className="text-[9px] text-gray-500 dark:text-gray-400 truncate">
                    {s.label}
                  </div>
                </div>
              ))}
            </div>
          </motion.div>
        ) : (
          <motion.div
            key={`titles-${pageIndex}`}
            initial={CONTENT_SLIDE_INITIAL}
            animate={CONTENT_SLIDE_ANIMATE}
            exit={CONTENT_SLIDE_EXIT}
            transition={CONTENT_SLIDE_TRANSITION}
            className="h-full w-full p-3 flex flex-col justify-center gap-2.5"
          >
            {currentTitles.map((title) => (
              <TrophyTitleRow key={title.name} title={title} accent={accent} />
            ))}
          </motion.div>
        )}
      </AnimatePresence>
    )
  },
)

AchievementReportBody.displayName = 'AchievementReportBody'

const XBOX_ACCENT_SOFT = '#3A9D23'

export const XboxScoreCardBody = memo(
  ({
    score,
    type,
    anim,
  }: {
    score: number
    type: string
    anim: AnimationConfig
  }) => {
    const { t } = useI18n()
    const displayScore = useCountUp(
      score,
      Math.round(900 * anim.durationScale),
      300,
    )
    const pct = Math.min(Math.max(displayScore, 0), 100)

    return (
      <>
        <motion.div
          className="flex items-center justify-between gap-2"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3, delay: 0.12 }}
        >
          <span className="flex shrink-0 items-center gap-1">
            <FaBolt
              className="h-2.5 w-2.5 shrink-0"
              style={{ color: XBOX_ACCENT_SOFT }}
            />
            <span className="bg-linear-to-r from-gray-700 to-[#107C10] bg-clip-text text-[11px] font-black italic tracking-tight text-transparent dark:from-gray-100 dark:to-[#3A9D23]">
              {t.reportCardWidget.xboxHunterScore}
            </span>
          </span>
          <div className="flex h-1.5 w-16 shrink-0 gap-[3px]">
            {Array.from({ length: SCORE_BAR_SEGMENTS }).map((_, i) => {
              const lit = i < Math.round((pct / 100) * SCORE_BAR_SEGMENTS)
              return (
                <div
                  key={i}
                  className={`h-full flex-1 rounded-[2px] transition-colors duration-150 ${
                    lit
                      ? 'bg-linear-to-b from-[#3A9D23] to-[#107C10] shadow-[0_0_6px_rgba(16,124,16,0.5)]'
                      : 'bg-black/8 dark:bg-white/10'
                  }`}
                />
              )
            })}
          </div>
        </motion.div>

        <motion.div
          className="flex items-center justify-between gap-2.5"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3, delay: 0.2 }}
        >
          <span className="flex shrink-0 items-baseline gap-0.5">
            <span className="text-3xl font-black leading-none tracking-tight tabular-nums text-gray-800 dark:text-gray-100">
              {displayScore}
            </span>
            <span className="text-[11px] font-bold text-gray-500 dark:text-gray-400">
              /100
            </span>
          </span>
          <motion.span
            className="inline-flex min-w-0 items-center gap-1.5 bg-gray-800/90 py-1 pl-2.5 pr-3 dark:bg-white/90"
            style={{
              clipPath:
                'polygon(0 0, calc(100% - 7px) 0, 100% 7px, 100% 100%, 0 100%)',
            }}
            initial={{ opacity: 0, scale: 0.85 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={
              anim.spring
                ? { type: 'spring', stiffness: 300, damping: 20, delay: 0.22 }
                : { duration: 0.25, delay: 0.22 }
            }
          >
            <span
              className={`h-1.5 w-1.5 shrink-0 rounded-full ${anim.loop ? 'animate-pulse' : ''}`}
              style={{ backgroundColor: XBOX_ACCENT_SOFT }}
            />
            <span className="truncate text-[10px] font-bold uppercase tracking-wide text-gray-100 dark:text-black">
              {type}
            </span>
          </motion.span>
        </motion.div>
      </>
    )
  },
)
XboxScoreCardBody.displayName = 'XboxScoreCardBody'

export const XboxStatsWidget = memo(({ data, isPreview }: any) => {
  const { t } = useI18n()
  const anim = useAnimationLevel()

  const gamertag = useMemo(
    () =>
      String(
        data?.gamertag || data?.display_gamertag || data?.username || '',
      ).trim(),
    [data],
  )
  const fallbackAvatar = useMemo(
    () => (typeof data?.avatar === 'string' ? data.avatar : null),
    [data],
  )
  const score = useMemo(() => {
    if (
      data?.hardcore_score !== undefined &&
      data?.hardcore_score !== null &&
      Number.isFinite(Number(data.hardcore_score))
    ) {
      return Math.min(100, Math.max(0, Math.round(Number(data.hardcore_score))))
    }
    const completion = Math.min(
      100,
      Math.max(0, Number(data?.completion_rate) || 0),
    )
    const completed = Number(data?.completed_games) || 0
    const gs = Number(data?.gamerscore) || 0
    const ach = Number(data?.total_achievements) || 0
    const gsPart = gs > 0 ? (Math.log(1 + gs) / Math.log(1 + 100_000)) * 20 : 0
    return Math.round(
      Math.min(
        100,
        completion * 0.45 +
          Math.min(completed * 5, 25) +
          gsPart +
          Math.min(ach / 50, 10),
      ),
    )
  }, [
    data?.hardcore_score,
    data?.completion_rate,
    data?.completed_games,
    data?.gamerscore,
    data?.total_achievements,
  ])
  const type = useMemo(
    () => data?.gamer_type || t.reportCardWidget.xboxGamerDefault,
    [data?.gamer_type, t.reportCardWidget.xboxGamerDefault],
  )
  const gamerscore = useMemo(
    () => Number(data?.gamerscore) || 0,
    [data?.gamerscore],
  )
  const gamesCount = useMemo(
    () => Number(data?.games_count) || 0,
    [data?.games_count],
  )
  const achievements = useMemo(
    () => Number(data?.total_achievements) || 0,
    [data?.total_achievements],
  )
  const completionRate = useMemo(
    () => Math.round(Number(data?.completion_rate) || 0),
    [data?.completion_rate],
  )
  const completedGames = useMemo(
    () => Number(data?.completed_games) || 0,
    [data?.completed_games],
  )

  const [livePresence, setLivePresence] = useState<XboxPresence | null>(null)

  useEffect(() => {
    // 预览 gamertag 是假的，不能拿去打接口。
    if (isPreview || !gamertag) return
    let cancelled = false
    const refresh = async () => {
      if (document.hidden) return
      const next = await fetchXboxPresence(gamertag)
      if (!cancelled) setLivePresence(next)
    }
    refresh()
    const intervalId = window.setInterval(refresh, 120 * 1000)
    return () => {
      cancelled = true
      window.clearInterval(intervalId)
    }
  }, [gamertag, isPreview])

  useEffect(() => {
    if (gamertag) return
    let cancelled = false
    fetchPlatformUserIds().then((ids) => {
      if (cancelled || !ids.xbox) return
      fetchXboxPresence(ids.xbox).then((next) => {
        if (!cancelled) setLivePresence(next)
      })
    })
    return () => {
      cancelled = true
    }
  }, [gamertag])

  const displayName = livePresence?.gamertag || gamertag || 'Xbox'
  const avatarUrl =
    proxyImageUrl(livePresence?.avatar) ||
    livePresence?.avatar ||
    fallbackAvatar
  const isDegraded = Boolean(livePresence?.degraded)
  const isLive = Boolean(
    !isDegraded && (livePresence?.is_online || livePresence?.is_in_game),
  )
  const nowPlaying =
    !isDegraded && livePresence?.is_in_game && livePresence?.game_title
      ? livePresence.game_title
      : null
  const presenceColor = isDegraded
    ? '#f59e0b'
    : livePresence?.is_in_game
      ? XBOX_ACCENT_SOFT
      : livePresence?.is_online
        ? '#22c55e'
        : '#9ca3af'
  const liveGs =
    livePresence?.gamerscore != null && livePresence.gamerscore > 0
      ? livePresence.gamerscore
      : gamerscore

  const statItems = useMemo(() => {
    const items: { label: string; value: string; unit: string }[] = [
      {
        label: t.reportCardWidget.gamesCount,
        value: String(gamesCount),
        unit: '',
      },
      {
        label: 'GS',
        value: formatCompactNumber(liveGs),
        unit: '',
      },
      {
        label: t.reportCardWidget.xboxAchievements,
        value: formatCompactNumber(achievements),
        unit: '',
      },
    ]
    if (completionRate > 0) {
      items.push({
        label: t.reportCardWidget.completionRate,
        value: String(completionRate),
        unit: '%',
      })
    }
    if (completedGames > 0) {
      items.push({
        label: t.reportCardWidget.completedGames,
        value: String(completedGames),
        unit: '',
      })
    }
    return items
  }, [t, gamesCount, liveGs, achievements, completionRate, completedGames])

  const [slotIndex, setSlotIndex] = useState(0)
  useEffect(() => {
    if (!nowPlaying || !anim.loop) {
      setSlotIndex(nowPlaying ? 1 : 0)
      return
    }
    setSlotIndex(1)
    let cancelled = false
    let timeoutId: number | null = null
    const tick = () => {
      if (cancelled || document.hidden) return
      setSlotIndex((prev) => (prev === 0 ? 1 : 0))
      timeoutId = window.setTimeout(tick, 6000)
    }
    timeoutId = window.setTimeout(tick, 6000)
    const onVisibility = () => {
      if (document.hidden && timeoutId) {
        clearTimeout(timeoutId)
        timeoutId = null
      } else if (!document.hidden && !cancelled && !timeoutId) {
        timeoutId = window.setTimeout(tick, 6000)
      }
    }
    document.addEventListener('visibilitychange', onVisibility)
    return () => {
      cancelled = true
      if (timeoutId) clearTimeout(timeoutId)
      document.removeEventListener('visibilitychange', onVisibility)
    }
  }, [nowPlaying, anim.loop])
  const showNowPlaying = Boolean(nowPlaying) && slotIndex === 1

  const safeAvatarUrl = useMemo(
    () => normalizeXboxMediaUrl(avatarUrl),
    [avatarUrl],
  )

  return (
    <div className="relative h-full w-full overflow-hidden">
      <div className="absolute inset-0 bg-linear-to-br from-[#107C10]/25 via-[#107C10]/8 to-transparent dark:from-[#107C10]/18 dark:via-[#107C10]/5 clip-diagonal" />

      <div className="relative z-10 flex h-full flex-col justify-between p-4">
        <motion.div
          className="flex min-w-0 items-center gap-3"
          initial={{ x: -12, opacity: 0 }}
          animate={{ x: 0, opacity: 1 }}
          transition={{ duration: 0.45 }}
        >
          <motion.div
            className="relative shrink-0"
            initial={{ scale: 0.7, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            whileHover={{ scale: 1.05 }}
            transition={
              anim.spring
                ? { type: 'spring', stiffness: 260, damping: 18, delay: 0.1 }
                : { duration: 0.35, delay: 0.1 }
            }
            title={
              livePresence?.degraded
                ? livePresence.degrade_reason ||
                  t.reportCardWidget.presenceDegraded
                : livePresence?.status
                  ? String(livePresence.status)
                  : undefined
            }
          >
            {safeAvatarUrl ? (
              <img
                src={safeAvatarUrl}
                alt={displayName}
                className="h-11 w-11 rounded-lg object-cover shadow-md ring-1 ring-black/10 dark:ring-white/15"
                loading="lazy"
                decoding="async"
              />
            ) : (
              <div className="flex h-11 w-11 items-center justify-center rounded-lg bg-[#107C10]/15 ring-1 ring-black/10 dark:bg-[#107C10]/25 dark:ring-white/15">
                <FaXbox className="h-5 w-5 text-[#107C10]" />
              </div>
            )}
            {livePresence && (
              <span className="absolute -bottom-0.5 -right-0.5 h-3 w-3">
                {isLive && anim.loop && (
                  <span
                    className="absolute inset-0 rounded-full opacity-40 animate-ping"
                    style={{ backgroundColor: presenceColor }}
                  />
                )}
                <span
                  className="absolute inset-0 rounded-full border-2 border-white dark:border-gray-900"
                  style={{ backgroundColor: presenceColor }}
                />
              </span>
            )}
          </motion.div>

          <div className="flex min-w-0 flex-col gap-1.5">
            <span
              className="truncate text-base font-black tracking-tight text-gray-800 dark:text-gray-100"
              style={{ WebkitTextStroke: '0.4px currentcolor' }}
            >
              {displayName}
            </span>
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5">
              {statItems.map((item, i) => (
                <motion.span
                  key={item.label}
                  className="flex items-baseline gap-1"
                  initial={{ y: 6, opacity: 0 }}
                  animate={{ y: 0, opacity: 1 }}
                  transition={{ duration: 0.35, delay: 0.3 + i * 0.08 }}
                >
                  <span className="flex items-baseline gap-0.5">
                    <span className="text-[11px] font-black leading-none text-gray-800 dark:text-gray-100">
                      {item.value}
                    </span>
                    {item.unit && (
                      <span className="text-[8px] font-bold text-gray-500 dark:text-gray-400">
                        {item.unit}
                      </span>
                    )}
                  </span>
                  <span className="text-[8px] font-bold text-gray-400 dark:text-gray-500">
                    {item.label}
                  </span>
                </motion.span>
              ))}
            </div>
          </div>
        </motion.div>

        <div className="translate-y-[3px] pl-13">
          <div className="relative h-16">
            <AnimatePresence mode="wait">
              {showNowPlaying ? (
                <motion.div
                  key="playing"
                  className="absolute inset-0 overflow-hidden rounded-lg shadow-sm ring-1 ring-black/10 dark:ring-white/15"
                  initial={{ opacity: 0, y: 12 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -12 }}
                  transition={{ duration: 0.4 }}
                  title={t.reportCardWidget.steamPlaying}
                >
                  <div className="absolute inset-0 bg-linear-to-br from-[#107C10] via-[#0B5A0B] to-[#062E06]" />
                  <div className="absolute inset-0 bg-[radial-gradient(circle_at_20%_20%,rgba(255,255,255,0.18),transparent_55%)]" />
                  <div className="absolute inset-x-0 bottom-0 flex items-center gap-1.5 p-1.5">
                    <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-white text-[#107C10] shadow-md">
                      <FaPlay className="h-2 w-2 translate-x-px" />
                    </span>
                    <span className="truncate text-[11px] font-bold text-white drop-shadow-sm">
                      {nowPlaying}
                    </span>
                  </div>
                </motion.div>
              ) : (
                <motion.div
                  key="score"
                  className="absolute inset-0 flex flex-col justify-center gap-1 rounded-lg glass-surface glass-45 px-3.5 ring-1 ring-black/5 dark:ring-white/10"
                  initial={{ opacity: 0, y: 12 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -12 }}
                  transition={{ duration: 0.4 }}
                >
                  <XboxScoreCardBody score={score} type={type} anim={anim} />
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        </div>
      </div>
    </div>
  )
})
XboxStatsWidget.displayName = 'XboxStatsWidget'

interface PsnPresence {
  online_id?: string | null
  avatar?: string | null
  is_online?: boolean
  is_in_game?: boolean
  game_title?: string | null
  status?: string | null
  trophy_level?: number | null
  platinum?: number | null
  degraded?: boolean
  degrade_reason?: string | null
}

const psnPresenceCache = new Map<string, { data: PsnPresence; at: number }>()
const psnPresenceInflight = new Map<string, Promise<PsnPresence | null>>()

async function fetchPsnPresence(
  onlineId: string,
  maxAgeMs = 60 * 1000,
): Promise<PsnPresence | null> {
  const key = onlineId.trim()
  if (!key) return null
  const cached = psnPresenceCache.get(key)
  if (cached && Date.now() - cached.at < maxAgeMs) return cached.data
  const inflight = psnPresenceInflight.get(key)
  if (inflight) return inflight

  const promise = (async () => {
    try {
      // Any failure lands in the catch below: drop the cache entry, report offline-unknown.
      const body = await apiService.get<{ success?: boolean, data?: any }>('/game/presence', {
        params: { platform: 'psn', id: key },
        timeout: 12_000,
      })
      const d = body?.data
      if (!body?.success || !d) {
        psnPresenceCache.delete(key)
        return null
      }
      const degraded = Boolean(d.degraded)
      const status = String(d?.presence?.status || '').toLowerCase()
      const title = d?.presence?.title ? String(d.presence.title) : null
      const isOnline = degraded
        ? false
        : status.includes('online') ||
          status === 'available' ||
          status === 'availabletoplay' ||
          (status.includes('available') && !status.includes('unavailable')) ||
          status === 'away' ||
          status === 'busy'
      const isInGame =
        !degraded && isOnline && Boolean(title && title !== 'Home')
      const lvRaw = d?.score?.value
      const lv =
        typeof lvRaw === 'string' || typeof lvRaw === 'number'
          ? Number(lvRaw)
          : null
      const platHighlight = Array.isArray(d?.highlights)
        ? d.highlights.find((h: any) =>
            String(h?.label || '')
              .toLowerCase()
              .includes('platinum'),
          )
        : null
      const plat =
        platHighlight?.value != null ? Number(platHighlight.value) : null
      const presence: PsnPresence = {
        online_id: d?.identity?.name || key,
        avatar: d?.identity?.avatar || null,
        is_online: isOnline,
        is_in_game: isInGame,
        game_title: isInGame ? title : null,
        status: degraded ? 'degraded' : d?.presence?.status || null,
        trophy_level: Number.isFinite(lv as number) ? (lv as number) : null,
        platinum: Number.isFinite(plat as number) ? (plat as number) : null,
        degraded,
        degrade_reason: d.degrade_reason
          ? String(d.degrade_reason)
          : null,
      }
      setPresenceCache(psnPresenceCache, key, presence)
      return presence
    } catch {
      psnPresenceCache.delete(key)
      return null
    } finally {
      psnPresenceInflight.delete(key)
    }
  })()
  psnPresenceInflight.set(key, promise)
  return promise
}

export const XboxWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const libraryItems = useMemo(() => {
    const mapItem = (t: any) => ({
      title: t.title || t.name,
      type: 'game',
      cover: normalizeXboxMediaUrl(t.cover || t.image),
      progress: t.progress,
      achievements_earned: t.achievements_earned,
      achievements_total: t.achievements_total,
      gamerscore: t.gamerscore,
    })
    const lib = Array.isArray(data?.library_items) ? data.library_items : []
    const fromLib = lib.map(mapItem).filter((x: any) => x.title)
    const withCover = fromLib.filter((x: any) => x.cover)
    if (withCover.length > 0) return withCover
    if (fromLib.length > 0) return fromLib
    const tops = Array.isArray(data?.top_titles) ? data.top_titles : []
    const fromTops = tops.map(mapItem).filter((x: any) => x.title)
    const topsCover = fromTops.filter((x: any) => x.cover)
    return topsCover.length > 0 ? topsCover : fromTops
  }, [data?.library_items, data?.top_titles])

  const { currentItem, currentItemIndex } = useLibraryItemRotation(
    libraryItems,
    showOverview,
  )

  useEffect(() => {
    if (!showOverview && currentItem) {
      onContentChange?.({ title: currentItem.title, type: 'game' })
    } else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItem, onContentChange])

  const progress = Math.round(Number(currentItem?.progress) || 0)
  const achEarned = Number(currentItem?.achievements_earned) || 0
  const achTotal = Number(currentItem?.achievements_total) || 0

  return (
    <AnimatePresence mode="wait">
      {showOverview || !currentItem ? (
        <motion.div
          key="stats"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full"
        >
          <XboxStatsWidget data={data} />
        </motion.div>
      ) : (
        <motion.div
          key={`lib-${currentItemIndex}`}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -10 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full p-1.5"
        >
          <div className="relative h-full w-full overflow-hidden rounded-lg bg-white shadow-lg dark:bg-black/90">
            <div className="absolute inset-0">
              {currentItem.cover ? (
                <img
                  src={currentItem.cover}
                  alt={currentItem.title}
                  className="h-full w-full object-cover"
                  loading="lazy"
                  referrerPolicy="no-referrer"
                />
              ) : (
                <div className="flex h-full w-full items-center justify-center bg-[#0B5A0B]">
                  <FaXbox className="h-10 w-10 text-white/30" />
                </div>
              )}
              <div className="absolute inset-0 bg-linear-to-t from-black/50 via-transparent to-transparent" />
            </div>
            {(progress > 0 || achTotal > 0) && (
              <div className="absolute top-2 right-2 z-10 flex items-center gap-1 rounded-md bg-black/55 px-1.5 py-0.5 ring-1 ring-white/15">
                <span
                  className="text-[10px] font-black tabular-nums text-white"
                  style={{ color: progress >= 100 ? '#a3e635' : undefined }}
                >
                  {progress}%
                </span>
                {achTotal > 0 && (
                  <span className="text-[9px] font-bold text-white/70">
                    {achEarned}/{achTotal}
                  </span>
                )}
              </div>
            )}
            {progress > 0 && (
              <div className="absolute inset-x-0 bottom-0 z-10 px-2.5 pb-2.5 pl-12">
                <div className="h-1 overflow-hidden rounded-full bg-white/20">
                  <motion.div
                    className="h-full rounded-full"
                    style={{
                      background:
                        progress >= 100
                          ? 'linear-gradient(90deg,#a3e635,#107C10)'
                          : XBOX_ACCENT_SOFT,
                    }}
                    initial={{ width: 0 }}
                    animate={{
                      width: `${Math.min(100, Math.max(0, progress))}%`,
                    }}
                    transition={{ duration: 0.8, ease: 'easeOut' }}
                  />
                </div>
              </div>
            )}
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
})

XboxWidget.displayName = 'XboxWidget'

const PSN_ACCENT_SOFT = '#3D9BFF'

export const PsnScoreCardBody = memo(
  ({
    score,
    type,
    anim,
  }: {
    score: number
    type: string
    anim: AnimationConfig
  }) => {
    const { t } = useI18n()
    const displayScore = useCountUp(
      score,
      Math.round(900 * anim.durationScale),
      300,
    )
    const pct = Math.min(Math.max(displayScore, 0), 100)

    return (
      <>
        <motion.div
          className="flex items-center justify-between gap-2"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3, delay: 0.12 }}
        >
          <span className="flex shrink-0 items-center gap-1">
            <FaBolt
              className="h-2.5 w-2.5 shrink-0"
              style={{ color: PSN_ACCENT_SOFT }}
            />
            <span className="bg-linear-to-r from-gray-700 to-[#0070D1] bg-clip-text text-[11px] font-black italic tracking-tight text-transparent dark:from-gray-100 dark:to-[#3D9BFF]">
              {t.reportCardWidget.psnHunterScore}
            </span>
          </span>
          <div className="flex h-1.5 w-16 shrink-0 gap-[3px]">
            {Array.from({ length: SCORE_BAR_SEGMENTS }).map((_, i) => {
              const lit = i < Math.round((pct / 100) * SCORE_BAR_SEGMENTS)
              return (
                <div
                  key={i}
                  className={`h-full flex-1 rounded-[2px] transition-colors duration-150 ${
                    lit
                      ? 'bg-linear-to-b from-[#3D9BFF] to-[#0070D1] shadow-[0_0_6px_rgba(0,112,209,0.5)]'
                      : 'bg-black/8 dark:bg-white/10'
                  }`}
                />
              )
            })}
          </div>
        </motion.div>

        <motion.div
          className="flex items-center justify-between gap-2.5"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3, delay: 0.2 }}
        >
          <span className="flex shrink-0 items-baseline gap-0.5">
            <span className="text-3xl font-black leading-none tracking-tight tabular-nums text-gray-800 dark:text-gray-100">
              {displayScore}
            </span>
            <span className="text-[11px] font-bold text-gray-500 dark:text-gray-400">
              /100
            </span>
          </span>
          <motion.span
            className="inline-flex min-w-0 items-center gap-1.5 bg-gray-800/90 py-1 pl-2.5 pr-3 dark:bg-white/90"
            style={{
              clipPath:
                'polygon(0 0, calc(100% - 7px) 0, 100% 7px, 100% 100%, 0 100%)',
            }}
            initial={{ opacity: 0, scale: 0.85 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={
              anim.spring
                ? { type: 'spring', stiffness: 300, damping: 20, delay: 0.22 }
                : { duration: 0.25, delay: 0.22 }
            }
          >
            <span
              className={`h-1.5 w-1.5 shrink-0 rounded-full ${anim.loop ? 'animate-pulse' : ''}`}
              style={{ backgroundColor: PSN_ACCENT_SOFT }}
            />
            <span className="truncate text-[10px] font-bold uppercase tracking-wide text-gray-100 dark:text-black">
              {type}
            </span>
          </motion.span>
        </motion.div>
      </>
    )
  },
)
PsnScoreCardBody.displayName = 'PsnScoreCardBody'

export const PsnStatsWidget = memo(({ data, isPreview }: any) => {
  const { t } = useI18n()
  const anim = useAnimationLevel()

  const onlineId = useMemo(
    () =>
      String(
        data?.online_id || data?.display_online_id || data?.username || '',
      ).trim(),
    [data],
  )
  const fallbackAvatar = useMemo(
    () => (typeof data?.avatar === 'string' ? data.avatar : null),
    [data],
  )
  const score = useMemo(() => {
    if (
      data?.hardcore_score !== undefined &&
      data?.hardcore_score !== null &&
      Number.isFinite(Number(data.hardcore_score))
    ) {
      return Math.min(100, Math.max(0, Math.round(Number(data.hardcore_score))))
    }
    const platinum = Number(data?.platinum_count) || 0
    const level = Number(data?.trophy_level) || 0
    const completion = Math.min(
      100,
      Math.max(0, Number(data?.completion_rate) || 0),
    )
    const completed = Number(data?.completed_games) || 0
    const games = Number(data?.games_count) || 0
    const platPart = Math.min(40, platinum * 4)
    const levelPart =
      level > 0 ? Math.min(25, (Math.log(level) / Math.log(400)) * 25) : 0
    const completionPart = completion * 0.25
    const completePart = games > 0 ? Math.min(10, (completed / games) * 10) : 0
    return Math.round(
      Math.min(100, platPart + levelPart + completionPart + completePart),
    )
  }, [
    data?.hardcore_score,
    data?.platinum_count,
    data?.trophy_level,
    data?.completion_rate,
    data?.completed_games,
    data?.games_count,
  ])
  const type = useMemo(
    () => data?.hunter_type || t.reportCardWidget.psnHunterDefault,
    [data?.hunter_type, t.reportCardWidget.psnHunterDefault],
  )
  const trophyLevel = useMemo(
    () => Number(data?.trophy_level) || 0,
    [data?.trophy_level],
  )
  const platinum = useMemo(
    () => Number(data?.platinum_count) || 0,
    [data?.platinum_count],
  )
  const gamesCount = useMemo(
    () => Number(data?.games_count) || 0,
    [data?.games_count],
  )
  const completionRate = useMemo(
    () => Math.round(Number(data?.completion_rate) || 0),
    [data?.completion_rate],
  )
  const completedGames = useMemo(
    () => Number(data?.completed_games) || 0,
    [data?.completed_games],
  )
  const totalTrophies = useMemo(
    () => Number(data?.total_trophies) || 0,
    [data?.total_trophies],
  )

  const [livePresence, setLivePresence] = useState<PsnPresence | null>(null)

  useEffect(() => {
    // 预览 onlineId 是假的，不能拿去打接口。
    if (isPreview || !onlineId) return
    let cancelled = false
    const refresh = async () => {
      if (document.hidden) return
      const next = await fetchPsnPresence(onlineId)
      if (!cancelled) setLivePresence(next)
    }
    refresh()
    const intervalId = window.setInterval(refresh, 120 * 1000)
    return () => {
      cancelled = true
      window.clearInterval(intervalId)
    }
  }, [onlineId, isPreview])

  useEffect(() => {
    if (onlineId) return
    let cancelled = false
    fetchPlatformUserIds().then((ids) => {
      if (cancelled || !ids.psn) return
      fetchPsnPresence(ids.psn).then((next) => {
        if (!cancelled) setLivePresence(next)
      })
    })
    return () => {
      cancelled = true
    }
  }, [onlineId])

  const displayName = livePresence?.online_id || onlineId || 'PlayStation'
  const avatarUrl =
    proxyImageUrl(livePresence?.avatar) ||
    normalizeHttpsMediaUrl(livePresence?.avatar) ||
    fallbackAvatar
  const isDegraded = Boolean(livePresence?.degraded)
  const isLive = Boolean(
    !isDegraded && (livePresence?.is_online || livePresence?.is_in_game),
  )
  const nowPlaying =
    !isDegraded && livePresence?.is_in_game && livePresence?.game_title
      ? livePresence.game_title
      : null
  const presenceColor = isDegraded
    ? '#f59e0b'
    : livePresence?.is_in_game
      ? PSN_ACCENT_SOFT
      : livePresence?.is_online
        ? '#22c55e'
        : '#9ca3af'
  const liveLevel =
    livePresence?.trophy_level != null && livePresence.trophy_level > 0
      ? livePresence.trophy_level
      : trophyLevel
  const livePlat =
    livePresence?.platinum != null && livePresence.platinum > 0
      ? livePresence.platinum
      : platinum

  const statItems = useMemo(() => {
    const items: { label: string; value: string; unit: string }[] = [
      {
        label: t.reportCardWidget.gamesCount,
        value: String(gamesCount),
        unit: '',
      },
      {
        label: t.reportCardWidget.platinumCount,
        value: formatCompactNumber(livePlat),
        unit: '',
      },
      {
        label: t.reportCardWidget.trophyLevel,
        value: String(liveLevel),
        unit: '',
      },
    ]
    if (completionRate > 0) {
      items.push({
        label: t.reportCardWidget.completionRate,
        value: String(completionRate),
        unit: '%',
      })
    }
    if (completedGames > 0) {
      items.push({
        label: t.reportCardWidget.completedGames,
        value: String(completedGames),
        unit: '',
      })
    } else if (totalTrophies > 0) {
      items.push({
        label: t.reportCardWidget.psnTrophies,
        value: formatCompactNumber(totalTrophies),
        unit: '',
      })
    }
    return items
  }, [
    t,
    gamesCount,
    livePlat,
    liveLevel,
    completionRate,
    completedGames,
    totalTrophies,
  ])

  const [slotIndex, setSlotIndex] = useState(0)
  useEffect(() => {
    if (!nowPlaying || !anim.loop) {
      setSlotIndex(nowPlaying ? 1 : 0)
      return
    }
    setSlotIndex(1)
    let cancelled = false
    let timeoutId: number | null = null
    const tick = () => {
      if (cancelled || document.hidden) return
      setSlotIndex((prev) => (prev === 0 ? 1 : 0))
      timeoutId = window.setTimeout(tick, 6000)
    }
    timeoutId = window.setTimeout(tick, 6000)
    const onVisibility = () => {
      if (document.hidden && timeoutId) {
        clearTimeout(timeoutId)
        timeoutId = null
      } else if (!document.hidden && !cancelled && !timeoutId) {
        timeoutId = window.setTimeout(tick, 6000)
      }
    }
    document.addEventListener('visibilitychange', onVisibility)
    return () => {
      cancelled = true
      if (timeoutId) clearTimeout(timeoutId)
      document.removeEventListener('visibilitychange', onVisibility)
    }
  }, [nowPlaying, anim.loop])
  const showNowPlaying = Boolean(nowPlaying) && slotIndex === 1

  return (
    <div className="relative h-full w-full overflow-hidden">
      <div className="absolute inset-0 bg-linear-to-br from-[#0070D1]/25 via-[#0070D1]/8 to-transparent dark:from-[#0070D1]/18 dark:via-[#0070D1]/5 clip-diagonal" />

      <div className="relative z-10 flex h-full flex-col justify-between p-4">
        <motion.div
          className="flex min-w-0 items-center gap-3"
          initial={{ x: -12, opacity: 0 }}
          animate={{ x: 0, opacity: 1 }}
          transition={{ duration: 0.45 }}
        >
          <motion.div
            className="relative shrink-0"
            initial={{ scale: 0.7, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            whileHover={{ scale: 1.05 }}
            transition={
              anim.spring
                ? { type: 'spring', stiffness: 260, damping: 18, delay: 0.1 }
                : { duration: 0.35, delay: 0.1 }
            }
            title={
              livePresence?.degraded
                ? livePresence.degrade_reason ||
                  t.reportCardWidget.presenceDegraded
                : livePresence?.status
                  ? String(livePresence.status)
                  : undefined
            }
          >
            {avatarUrl ? (
              <img
                src={avatarUrl}
                alt={displayName}
                className="h-11 w-11 rounded-lg object-cover shadow-md ring-1 ring-black/10 dark:ring-white/15"
                loading="lazy"
                decoding="async"
                referrerPolicy="no-referrer"
              />
            ) : (
              <div className="flex h-11 w-11 items-center justify-center rounded-lg bg-[#0070D1]/15 ring-1 ring-black/10 dark:bg-[#0070D1]/25 dark:ring-white/15">
                <SiPlaystation className="h-5 w-5 text-[#0070D1]" />
              </div>
            )}
            {livePresence && (
              <span className="absolute -bottom-0.5 -right-0.5 h-3 w-3">
                {isLive && anim.loop && (
                  <span
                    className="absolute inset-0 rounded-full opacity-40 animate-ping"
                    style={{ backgroundColor: presenceColor }}
                  />
                )}
                <span
                  className="absolute inset-0 rounded-full border-2 border-white dark:border-gray-900"
                  style={{ backgroundColor: presenceColor }}
                />
              </span>
            )}
          </motion.div>

          <div className="flex min-w-0 flex-col gap-1.5">
            <span
              className="truncate text-base font-black tracking-tight text-gray-800 dark:text-gray-100"
              style={{ WebkitTextStroke: '0.4px currentcolor' }}
            >
              {displayName}
            </span>
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5">
              {statItems.map((item, i) => (
                <motion.span
                  key={item.label}
                  className="flex items-baseline gap-1"
                  initial={{ y: 6, opacity: 0 }}
                  animate={{ y: 0, opacity: 1 }}
                  transition={{ duration: 0.35, delay: 0.3 + i * 0.08 }}
                >
                  <span className="flex items-baseline gap-0.5">
                    <span className="text-[11px] font-black leading-none text-gray-800 dark:text-gray-100">
                      {item.value}
                    </span>
                    {item.unit && (
                      <span className="text-[8px] font-bold text-gray-500 dark:text-gray-400">
                        {item.unit}
                      </span>
                    )}
                  </span>
                  <span className="text-[8px] font-bold text-gray-400 dark:text-gray-500">
                    {item.label}
                  </span>
                </motion.span>
              ))}
            </div>
          </div>
        </motion.div>

        <div className="translate-y-[3px] pl-13">
          <div className="relative h-16">
            <AnimatePresence mode="wait">
              {showNowPlaying ? (
                <motion.div
                  key="playing"
                  className="absolute inset-0 overflow-hidden rounded-lg shadow-sm ring-1 ring-black/10 dark:ring-white/15"
                  initial={{ opacity: 0, y: 12 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -12 }}
                  transition={{ duration: 0.4 }}
                  title={t.reportCardWidget.steamPlaying}
                >
                  <div className="absolute inset-0 bg-linear-to-br from-[#0070D1] via-[#0051A8] to-[#002D5C]" />
                  <div className="absolute inset-0 bg-[radial-gradient(circle_at_20%_20%,rgba(255,255,255,0.18),transparent_55%)]" />
                  <div className="absolute inset-x-0 bottom-0 flex items-center gap-1.5 p-1.5">
                    <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-white text-[#0070D1] shadow-md">
                      <FaPlay className="h-2 w-2 translate-x-px" />
                    </span>
                    <span className="truncate text-[11px] font-bold text-white drop-shadow-sm">
                      {nowPlaying}
                    </span>
                  </div>
                </motion.div>
              ) : (
                <motion.div
                  key="score"
                  className="absolute inset-0 flex flex-col justify-center gap-1 rounded-lg glass-surface glass-45 px-3.5 ring-1 ring-black/5 dark:ring-white/10"
                  initial={{ opacity: 0, y: 12 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -12 }}
                  transition={{ duration: 0.4 }}
                >
                  <PsnScoreCardBody score={score} type={type} anim={anim} />
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        </div>
      </div>
    </div>
  )
})
PsnStatsWidget.displayName = 'PsnStatsWidget'

export const PsnWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const libraryItems = useMemo(() => {
    const mapItem = (t: any) => ({
      title: t.title || t.name,
      type: 'game',
      cover: normalizeHttpsMediaUrl(t.cover || t.image),
      progress: t.progress,
      platinum: Boolean(t.platinum),
      platform: t.platform,
    })
    const lib = Array.isArray(data?.library_items) ? data.library_items : []
    const fromLib = lib.map(mapItem).filter((x: any) => x.title)
    const withCover = fromLib.filter((x: any) => x.cover)
    if (withCover.length > 0) return withCover
    if (fromLib.length > 0) return fromLib
    const tops = Array.isArray(data?.top_titles) ? data.top_titles : []
    const fromTops = tops.map(mapItem).filter((x: any) => x.title)
    const topsCover = fromTops.filter((x: any) => x.cover)
    return topsCover.length > 0 ? topsCover : fromTops
  }, [data?.library_items, data?.top_titles])

  const { currentItem, currentItemIndex } = useLibraryItemRotation(
    libraryItems,
    showOverview,
  )

  useEffect(() => {
    if (!showOverview && currentItem) {
      onContentChange?.({ title: currentItem.title, type: 'game' })
    } else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItem, onContentChange])

  const progress = Math.round(Number(currentItem?.progress) || 0)
  const hasPlatinum = Boolean(currentItem?.platinum)

  return (
    <AnimatePresence mode="wait">
      {showOverview || !currentItem ? (
        <motion.div
          key="stats"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full"
        >
          <PsnStatsWidget data={data} />
        </motion.div>
      ) : (
        <motion.div
          key={`lib-${currentItemIndex}`}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -10 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full p-1.5"
        >
          <div className="relative h-full w-full overflow-hidden rounded-lg bg-white shadow-lg dark:bg-black/90">
            <div className="absolute inset-0">
              {currentItem.cover ? (
                <img
                  src={currentItem.cover}
                  alt={currentItem.title}
                  className="h-full w-full object-cover"
                  loading="lazy"
                  referrerPolicy="no-referrer"
                />
              ) : (
                <div className="flex h-full w-full items-center justify-center bg-[#0051A8]">
                  <SiPlaystation className="h-10 w-10 text-white/30" />
                </div>
              )}
              <div className="absolute inset-0 bg-linear-to-t from-black/50 via-transparent to-transparent" />
            </div>
            {(progress > 0 || hasPlatinum) && (
              <div className="absolute top-2 right-2 z-10 flex items-center gap-1 rounded-md bg-black/55 px-1.5 py-0.5 ring-1 ring-white/15">
                {hasPlatinum && (
                  <span className="text-[10px]" title="Platinum">
                    🏆
                  </span>
                )}
                {progress > 0 && (
                  <span
                    className="text-[10px] font-black tabular-nums text-white"
                    style={{
                      color: progress >= 100 ? '#fbbf24' : undefined,
                    }}
                  >
                    {progress}%
                  </span>
                )}
              </div>
            )}
            {progress > 0 && (
              <div className="absolute inset-x-0 bottom-0 z-10 px-2.5 pb-2.5 pl-12">
                <div className="h-1 overflow-hidden rounded-full bg-white/20">
                  <motion.div
                    className="h-full rounded-full"
                    style={{
                      background:
                        progress >= 100
                          ? 'linear-gradient(90deg,#fbbf24,#0070D1)'
                          : PSN_ACCENT_SOFT,
                    }}
                    initial={{ width: 0 }}
                    animate={{
                      width: `${Math.min(100, Math.max(0, progress))}%`,
                    }}
                    transition={{ duration: 0.8, ease: 'easeOut' }}
                  />
                </div>
              </div>
            )}
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
})

PsnWidget.displayName = 'PsnWidget'
