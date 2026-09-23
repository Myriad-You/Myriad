import type { CSSProperties } from 'react'
import type { WidgetComponentProps } from '../widgetGridTypes'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useVisibilityInterval } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { armWidgetSettingsHost } from '../../lib/widgetSettingsHost'
import { ApiError, apiService } from '../../services/api'
import { useThemeMode } from '../../utils/themeSubscriber'
import { userFacingError } from '../../utils/userFacingError'
import {
  sanitizeWidgetFontUrl,
  uploadWidgetFont,
  widgetFontFaceUrl,
  widgetFontFamilyName,
} from '../../utils/widgetFonts'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetLongPressHint } from './shared/WidgetLongPressHint'
import {
  WidgetSettingsChoice,
  WidgetSettingsChoices,
  WidgetSettingsSection,
  WidgetSettingsTip,
} from './shared/WidgetSettingsTip'
import { WidgetShell } from './shared/WidgetShell'
import { WidgetSkeleton } from './shared/WidgetSkeleton'
import './GamePresenceWidget.css'

export type GamePlatformId = 'hoyolab'
export type HoyoGame = 'genshin' | 'hsr' | 'zzz'

export interface GamePresenceWidgetConfig {
  platformId?: GamePlatformId
  accountId?: string
  game?: HoyoGame
  fontUrl?: string
}

interface GameIdentity {
  id: string
  name: string
  avatar?: string | null
  subtitle?: string | null
}

interface GameScore {
  label: string
  value: string
}

interface GamePresenceInfo {
  status: string
  title?: string | null
  detail?: string | null
}

interface GameHighlight {
  label: string
  value: string
}

interface ShowcaseItem {
  name: string
  level?: number | null
  icon?: string | null
  art?: string | null
  rarity?: number | null
}

interface GamePresenceData {
  platform: string
  identity: GameIdentity
  score?: GameScore | null
  presence?: GamePresenceInfo | null
  highlights: GameHighlight[]
  showcase: ShowcaseItem[]
  profile_url?: string | null
  fetched_at: string
  degraded: boolean
  degrade_reason?: string | null
}

interface GameTheme {
  color: string
  darkColor: string
}

const HOYO_GAME_THEMES: Record<HoyoGame, GameTheme> = {
  genshin: { color: '#C9A227', darkColor: '#E8C547' },
  hsr: { color: '#6B5CE7', darkColor: '#9B8CFF' },
  zzz: { color: '#E8C547', darkColor: '#FFE566' },
}

const UID_PLACEHOLDER = '800123456'

const HOYO_GAMES: { id: HoyoGame, labelKey: 'genshin' | 'hsr' | 'zzz' }[] = [
  { id: 'genshin', labelKey: 'genshin' },
  { id: 'hsr', labelKey: 'hsr' },
  { id: 'zzz', labelKey: 'zzz' },
]

const GAME_META: Record<
  HoyoGame,
  {
    appIcon: string
    logoClass: string
    fontClass: string
    artPos: string
  }
> = {
  genshin: {
    appIcon: '/game-logos/genshin-icon.webp',
    logoClass: 'gp-logo-genshin',
    fontClass: 'gp-font-genshin',
    artPos: 'center top',
  },
  hsr: {
    appIcon: '/game-logos/starrail-icon.webp',
    logoClass: 'gp-logo-hsr',
    fontClass: 'gp-font-hsr',
    artPos: 'center 25%',
  },
  zzz: {
    appIcon: '/game-logos/zzz-icon.webp',
    logoClass: 'gp-logo-zzz',
    fontClass: 'gp-font-zzz',
    artPos: 'center top',
  },
}

function rarityRing(rarity: number | null | undefined, fallback: string): string {
  if (rarity === 5) return '#E8B33B'
  if (rarity === 4) return '#A47CE0'
  return fallback
}

function resolveTheme(
  game: HoyoGame,
  isDark: boolean,
): {
  primary: string
  softBgStrong: string
  border: string
} {
  const base = HOYO_GAME_THEMES[game] || HOYO_GAME_THEMES.genshin
  const primary = isDark ? base.darkColor : base.color

  return {
    primary,
    softBgStrong: hexToRgba(primary, isDark ? 0.22 : 0.16),
    border: hexToRgba(primary, isDark ? 0.35 : 0.28),
  }
}

function hexToRgba(hex: string, alpha: number): string {
  const h = hex.replaceAll('#', '')
  if (h.length !== 6) return `rgba(100,100,100,${alpha})`
  const r = Number.parseInt(h.slice(0, 2), 16)
  const g = Number.parseInt(h.slice(2, 4), 16)
  const b = Number.parseInt(h.slice(4, 6), 16)
  return `rgba(${r},${g},${b},${alpha})`
}

interface SettingsState {
  isOpen: boolean
  accountId: string
  game: HoyoGame
  fontUrl: string
  anchorRect?: DOMRect
  onSave?: (
    cfg: Required<
      Pick<GamePresenceWidgetConfig, 'platformId' | 'accountId' | 'game'>
    > & { fontUrl: string },
  ) => void
}

let globalSettings: SettingsState = {
  isOpen: false,
  accountId: '',
  game: 'genshin',
  fontUrl: '',
}

const settingsListeners = new Set<() => void>()

function openGamePresenceSettings(
  accountId: string,
  game: HoyoGame,
  fontUrl: string,
  anchorRect: DOMRect,
  onSave: SettingsState['onSave'],
) {
  armWidgetSettingsHost()
  globalSettings = {
    isOpen: true,
    accountId,
    game,
    fontUrl,
    anchorRect,
    onSave,
  }
  settingsListeners.forEach((l) => l())
}

function closeGamePresenceSettings() {
  globalSettings = { ...globalSettings, isOpen: false }
  settingsListeners.forEach((l) => l())
}

function subscribeSettings(listener: () => void) {
  settingsListeners.add(listener)
  return () => {
    settingsListeners.delete(listener)
  }
}

const GamePresenceSettingsModal = memo(() => {
  const { t } = useI18n()
  const [, forceUpdate] = useState({})
  const [draftAccountId, setDraftAccountId] = useState('')
  const [draftGame, setDraftGame] = useState<HoyoGame>('genshin')
  const [draftFontUrl, setDraftFontUrl] = useState('')
  const [fontBusy, setFontBusy] = useState(false)
  const [fontError, setFontError] = useState<string | null>(null)
  const fontInputRef = useRef<HTMLInputElement | null>(null)
  useEffect(() => subscribeSettings(() => forceUpdate({})), [])

  const { isOpen, anchorRect, onSave } = globalSettings
  const tw = t.gamePresenceWidget

  useEffect(() => {
    if (isOpen) {
      setDraftAccountId(globalSettings.accountId)
      setDraftGame(globalSettings.game)
      setDraftFontUrl(globalSettings.fontUrl)
      setFontBusy(false)
      setFontError(null)
    }
  }, [isOpen])

  const handlePickFont = useCallback(async (file: File | undefined) => {
    if (!file) return
    setFontBusy(true)
    setFontError(null)
    try {
      const url = await uploadWidgetFont(file)
      setDraftFontUrl(url)
    } catch (error) {
      setFontError(userFacingError(error, tw.customFontFailed))
    } finally {
      setFontBusy(false)
      if (fontInputRef.current) fontInputRef.current.value = ''
    }
  }, [tw.customFontFailed])

  const handleSave = useCallback(() => {
    const id = draftAccountId.trim()
    if (!id) return
    onSave?.({
      platformId: 'hoyolab',
      accountId: id,
      game: draftGame,
      fontUrl: draftFontUrl,
    })
    closeGamePresenceSettings()
  }, [draftAccountId, draftFontUrl, draftGame, onSave])

  return (
    <WidgetSettingsTip
      open={isOpen}
      anchor={anchorRect ?? null}
      title={t.widgets.gamePresence}
      width={300}
      height={440}
      onClose={closeGamePresenceSettings}
    >
      <WidgetSettingsSection label={tw.selectGame}>
        <WidgetSettingsChoices label={tw.selectGame} row>
          {HOYO_GAMES.map((g) => (
            <WidgetSettingsChoice
              key={g.id}
              selected={draftGame === g.id}
              onClick={() => setDraftGame(g.id)}
            >
              <img
                src={GAME_META[g.id].appIcon}
                alt=""
                className="w-7 h-7 rounded-lg object-cover shrink-0"
              />
              <span className="widget-settings-tip__choice-label">
                {tw[g.labelKey]}
              </span>
            </WidgetSettingsChoice>
          ))}
        </WidgetSettingsChoices>
      </WidgetSettingsSection>
      <WidgetSettingsSection label={tw.uidLabel}>
        <input
          type="text"
          value={draftAccountId}
          onChange={(e) => setDraftAccountId(e.target.value)}
          placeholder={UID_PLACEHOLDER}
          className="widget-settings-tip__field"
          autoComplete="off"
          spellCheck={false}
        />
        <p className="widget-settings-tip__subtitle" style={{ marginTop: '0.35rem' }}>
          {tw.publicOnlyHint}
        </p>
      </WidgetSettingsSection>
      <WidgetSettingsSection label={tw.customFont}>
        <input
          ref={fontInputRef}
          type="file"
          accept=".woff2,.woff,.ttf,.otf"
          hidden
          onChange={(e) => void handlePickFont(e.target.files?.[0])}
        />
        <WidgetSettingsChoices label={tw.customFont}>
          <WidgetSettingsChoice
            selected={!draftFontUrl}
            label={tw.customFontClear}
            onClick={() => {
              if (fontBusy) return
              setDraftFontUrl('')
              setFontError(null)
            }}
          />
          <WidgetSettingsChoice
            selected={Boolean(draftFontUrl)}
            label={fontBusy ? tw.customFontUploading : tw.customFontChoose}
            hint={draftFontUrl && !fontBusy ? tw.customFontInUse : undefined}
            onClick={() => {
              if (fontBusy) return
              fontInputRef.current?.click()
            }}
          />
        </WidgetSettingsChoices>
        <p className="widget-settings-tip__subtitle">{tw.customFontHint}</p>
        {fontError ? (
          <p className="widget-settings-tip__subtitle">{fontError}</p>
        ) : null}
        <button
          type="button"
          className="widget-settings-tip__save"
          onClick={handleSave}
          disabled={!draftAccountId.trim() || fontBusy}
        >
          {tw.save}
        </button>
      </WidgetSettingsSection>
    </WidgetSettingsTip>
  )
})

GamePresenceSettingsModal.displayName = 'GamePresenceSettingsModal'

const dataCache = new Map<string, { data: GamePresenceData, at: number }>()
const DATA_TTL = 6 * 3600 * 1000
const MAX_DATA_CACHE = 20
const inflight = new Map<string, Promise<GamePresenceData | null>>()

function setDataCache(key: string, data: GamePresenceData): void {
  dataCache.delete(key)
  dataCache.set(key, { data, at: Date.now() })
  while (dataCache.size > MAX_DATA_CACHE) {
    const oldest = dataCache.keys().next().value
    if (oldest === undefined) break
    dataCache.delete(oldest)
  }
}

async function fetchGamePresence(
  platformId: GamePlatformId,
  accountId: string,
  game: HoyoGame,
  lang: string,
): Promise<GamePresenceData | null> {
  const key = `${platformId}:${accountId}:${game}:${lang}`
  const cached = dataCache.get(key)
  if (cached && Date.now() - cached.at < DATA_TTL) {
    return cached.data
  }
  if (inflight.has(key)) return inflight.get(key)!

  const p = (async () => {
    try {
      const body = await apiService.get<{ success?: boolean, data?: unknown }>('/game/presence', {
        params: {
          platform: platformId,
          id: accountId,
          lang,
          game: platformId === 'hoyolab' ? game : undefined,
        },
        timeout: 15_000,
      })
      if (body?.success && body?.data) {
        setDataCache(key, body.data as GamePresenceData)
        return body.data as GamePresenceData
      }
      return null
    } catch (error) {
      // A slow upstream is "no data yet", not a widget error.
      if (error instanceof ApiError && error.code === 'TIMEOUT') return null
      throw error
    } finally {
      inflight.delete(key)
    }
  })()

  inflight.set(key, p)
  return p
}

function resolveConfig(config: WidgetComponentProps['config']): {
  platformId: GamePlatformId
  accountId: string
  game: HoyoGame
  fontUrl: string
} {
  const c = (config.config || {}) as GamePresenceWidgetConfig
  const game = (['genshin', 'hsr', 'zzz'] as const).includes(c.game as HoyoGame)
    ? (c.game as HoyoGame)
    : 'genshin'
  return {
    platformId: 'hoyolab',
    accountId: (c.accountId || '').trim(),
    game,
    fontUrl: sanitizeWidgetFontUrl(c.fontUrl),
  }
}

const GamePresenceWidget = memo(
  ({ config, isEditMode, isPreview, onConfigChange }: WidgetComponentProps) => {
    const { t, locale } = useI18n()
    const tw = t.gamePresenceWidget
    const isDark = useThemeMode()
    const anim = useAnimationLevel()
    const { fontScale, scale, containerRef } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const localRef = useRef<HTMLDivElement | null>(null)
    const setRefs = useCallback(
      (node: HTMLDivElement | null) => {
        localRef.current = node
        containerRef(node)
      },
      [containerRef],
    )

    const resolved = resolveConfig(config)
    const [accountId, setAccountId] = useState(resolved.accountId)
    const [game, setGame] = useState(resolved.game)
    const [fontUrl, setFontUrl] = useState(resolved.fontUrl)
    const [customFamily, setCustomFamily] = useState<string | null>(null)

    const [data, setData] = useState<GamePresenceData | null>(null)
    const [loading, setLoading] = useState(false)
    const [error, setError] = useState<string | null>(null)

    const longPressTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
    const isLongPressRef = useRef(false)

    useEffect(() => {
      const next = resolveConfig(config)
      setAccountId(next.accountId)
      setGame(next.game)
      setFontUrl(next.fontUrl)
    }, [config.config?.accountId, config.config?.fontUrl, config.config?.game])

    useEffect(() => {
      if (!fontUrl || isPreview) {
        setCustomFamily(null)
        return
      }
      let cancelled = false
      let face: FontFace | null = null
      const start = () => {
        if (cancelled) return
        const family = widgetFontFamilyName(fontUrl)
        face = new FontFace(family, `url(${widgetFontFaceUrl(fontUrl)})`, {
          display: 'swap',
        })
        void face
          .load()
          .then((loaded) => {
            if (cancelled) return
            document.fonts.add(loaded)
            setCustomFamily(family)
          })
          .catch(() => {
            if (!cancelled) setCustomFamily(null)
          })
      }
      const idle =
        typeof window.requestIdleCallback === 'function'
          ? window.requestIdleCallback(start, { timeout: 4000 })
          : window.setTimeout(start, 1)
      return () => {
        cancelled = true
        if (typeof window.requestIdleCallback === 'function') {
          window.cancelIdleCallback(idle)
        } else {
          window.clearTimeout(idle)
        }
        if (face) document.fonts.delete(face)
      }
    }, [fontUrl, isPreview])

    useEffect(() => {
      if (isPreview) return
      if (!accountId) {
        setData(null)
        setError(null)
        return
      }
      let cancelled = false
      let hasData = false
      let retryTimer: number | null = null

      const scheduleRetry = () => {
        if (cancelled || retryTimer != null) return
        retryTimer = window.setTimeout(() => {
          retryTimer = null
          if (cancelled) return
          if (document.hidden) {
            scheduleRetry()
            return
          }
          void load(false)
        }, 60 * 1000)
      }

      const load = async (isInitial: boolean) => {
        if (isInitial) {
          setLoading(true)
          setError(null)
        }
        try {
          const d = await fetchGamePresence('hoyolab', accountId, game, locale)
          if (cancelled) return
          if (isInitial) setLoading(false)
          if (d) {
            hasData = true
            setData(d)
            setError(null)
          } else if (!hasData) {
            setData(null)
            setError(tw.showcaseEmpty)
            scheduleRetry()
          }
        } catch (error) {
          if (cancelled) return
          if (isInitial) setLoading(false)
          if (!hasData) {
            setData(null)
            setError(userFacingError(error, tw.fetchFailed))
            scheduleRetry()
          }
        }
      }

      load(true)
      const intervalId = window.setInterval(() => {
        if (document.hidden) return
        load(false)
      }, DATA_TTL)

      return () => {
        cancelled = true
        if (retryTimer != null) window.clearTimeout(retryTimer)
        window.clearInterval(intervalId)
      }
    }, [accountId, game, locale, isPreview, tw.fetchFailed])

    const theme = useMemo(() => resolveTheme(game, isDark), [game, isDark])
    const iconColor = theme.primary

    const showcaseLen = data?.showcase?.length ?? 0
    const [focusIndex, setFocusIndex] = useState(0)
    const queueRef = useRef<HTMLDivElement | null>(null)

    useEffect(() => {
      const el = queueRef.current
      if (!el) return
      const btn = el.children[focusIndex] as HTMLElement | undefined
      if (!btn) return
      el.scrollTo({
        left: btn.offsetLeft - (el.clientWidth - btn.offsetWidth) / 2,
        behavior: 'smooth',
      })
    }, [focusIndex, showcaseLen])

    useEffect(() => {
      setFocusIndex(0)
    }, [accountId, game, showcaseLen])

    useVisibilityInterval(
      () => setFocusIndex((prev) => (prev + 1) % showcaseLen),
      { delay: 4000, enabled: !isPreview && anim.loop && showcaseLen > 1 },
    )

    const persist = useCallback(
      (next: {
        platformId: GamePlatformId
        accountId: string
        game: HoyoGame
        fontUrl: string
      }) => {
        setAccountId(next.accountId)
        setGame(next.game)
        setFontUrl(next.fontUrl)
        const payload = {
          ...config.config,
          platformId: next.platformId,
          accountId: next.accountId,
          game: next.game,
          fontUrl: next.fontUrl,
        }
        onConfigChange?.(payload)
      },
      [config.config, config.id, onConfigChange],
    )

    const openSettings = useCallback(() => {
      if (!localRef.current) return
      openGamePresenceSettings(
        accountId,
        game,
        fontUrl,
        localRef.current.getBoundingClientRect(),
        persist,
      )
    }, [accountId, fontUrl, game, persist])

    const handlePressStart = useCallback(() => {
      if (!isEditMode) return
      isLongPressRef.current = false
      longPressTimerRef.current = setTimeout(() => {
        isLongPressRef.current = true
        openSettings()
      }, 500)
    }, [isEditMode, openSettings])

    const handlePressEnd = useCallback(() => {
      if (longPressTimerRef.current) {
        clearTimeout(longPressTimerRef.current)
        longPressTimerRef.current = null
      }
    }, [])

    useEffect(() => {
      return () => {
        if (longPressTimerRef.current) clearTimeout(longPressTimerRef.current)
      }
    }, [])

    const handleClick = useCallback(() => {
      if (isLongPressRef.current) {
        isLongPressRef.current = false
        return
      }
      if (isEditMode) return
      const url = data?.profile_url
      if (url) window.open(url, '_blank', 'noopener,noreferrer')
    }, [isEditMode, data?.profile_url])

    const hasAccount = Boolean(accountId)
    const meta = GAME_META[game]

    const content = useMemo(() => {
      const appIcon = (size: number) => (
        <img
          src={meta.appIcon}
          alt={tw[game]}
          className="rounded-lg object-cover shrink-0 shadow-sm"
          style={{ width: `${size * fontScale}px`, height: `${size * fontScale}px` }}
          loading="lazy"
        />
      )
      const wordmarkBg = (
        <span
          className={`gp-logo ${meta.logoClass} absolute right-2 bottom-1 h-[42%] opacity-[0.05] dark:opacity-[0.07] text-gray-900 dark:text-gray-100 pointer-events-none`}
          aria-hidden
        />
      )

      if (!hasAccount) {
        return (
          <div className="relative h-full w-full flex flex-col items-center justify-center gap-2.5 text-center px-4">
            {wordmarkBg}
            {appIcon(44)}
            <span
              className="text-gray-500 dark:text-gray-400"
              style={{ fontSize: `${12 * fontScale}px` }}
            >
              {isEditMode ? tw.longPressToSetup : tw.notConfigured}
            </span>
          </div>
        )
      }

      if (loading && !data) {
        return (
          <WidgetSkeleton
            preset="media-row"
            accent={theme.primary}
            label={t.common.loading}
          />
        )
      }

      if (error && !data) {
        return (
          <div className="relative h-full w-full flex flex-col items-center justify-center gap-2 px-4 text-center">
            {wordmarkBg}
            {appIcon(36)}
            <span
              className="text-gray-500 dark:text-gray-400"
              style={{ fontSize: `${12 * fontScale}px` }}
            >
              {error}
            </span>
          </div>
        )
      }

      const name = data?.identity.name || accountId
      const score = data?.score
      const showcase = (data?.showcase || []).slice(0, 6)
      const focused = showcase[focusIndex % Math.max(1, showcase.length)]

      const roundAvatar = (
        s: (typeof showcase)[number],
        size: number,
        ring: string,
        ringWidth = 2,
      ) =>
        s.icon ? (
          <img
            src={s.icon}
            alt={s.name}
            className="rounded-full object-cover"
            referrerPolicy="no-referrer"
            style={{
              width: `${size * fontScale}px`,
              height: `${size * fontScale}px`,
              background: theme.softBgStrong,
              boxShadow: `0 0 0 ${ringWidth}px ${ring}`,
            }}
            loading="lazy"
          />
        ) : (
          <div
            className="rounded-full flex items-center justify-center font-bold"
            style={{
              width: `${size * fontScale}px`,
              height: `${size * fontScale}px`,
              background: theme.softBgStrong,
              color: theme.primary,
              boxShadow: `0 0 0 ${ringWidth}px ${ring}`,
              fontSize: `${size * 0.36 * fontScale}px`,
            }}
          >
            {s.name.slice(0, 1)}
          </div>
        )

      return (
        <div className={`relative h-full w-full flex gap-3 min-h-0 ${meta.fontClass}`}>
          {showcase.length > 0 && focused && (
            <div
              className="relative w-[34%] shrink-0 h-full rounded-lg overflow-hidden"
              style={{
                background: theme.softBgStrong,
                boxShadow: `inset 0 0 0 1.5px ${theme.border}`,
              }}
            >
              <AnimatePresence mode="wait">
                <motion.div
                  key={`${focused.name}-${focusIndex}`}
                  initial={{ opacity: 0, scale: 1.06 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0 }}
                  transition={{ duration: 0.4, ease: 'easeOut' }}
                  className="absolute inset-0"
                >
                  {focused.art || focused.icon ? (
                    <img
                      src={focused.art || focused.icon || undefined}
                      alt={focused.name}
                      className={`w-full h-full object-cover ${focused.art
                        ? `gp-art gp-art-zoom ${game === 'genshin' || game === 'zzz' ? 'gp-art-upper' : ''}`
                        : ''}`}
                      style={{ objectPosition: meta.artPos }}
                      loading="lazy"
                      referrerPolicy="no-referrer"
                    />
                  ) : (
                    <div
                      className="w-full h-full flex items-center justify-center font-bold"
                      style={{
                        color: theme.primary,
                        fontSize: `${30 * fontScale}px`,
                      }}
                    >
                      {focused.name.slice(0, 1)}
                    </div>
                  )}
                </motion.div>
              </AnimatePresence>

              <div className="absolute inset-x-0 bottom-0 px-2 pt-7 pb-1.5 bg-linear-to-t from-black/75 via-black/30 to-transparent pointer-events-none">
                <div
                  className="text-white font-bold truncate leading-tight"
                  style={{ fontSize: `${12.5 * fontScale}px` }}
                >
                  {focused.name}
                </div>
                <div
                  className="mt-0.5 flex items-center gap-1.5 leading-none tabular-nums"
                  style={{ fontSize: `${9.5 * fontScale}px` }}
                >
                  {focused.level != null && (
                    <span className="text-white/90 font-semibold">
                      Lv.
                      {focused.level}
                    </span>
                  )}
                  {focused.rarity != null && (
                    <span
                      style={{
                        color: rarityRing(focused.rarity, '#ffffff'),
                      }}
                    >
                      {'★'.repeat(Math.min(5, Math.max(1, focused.rarity)))}
                    </span>
                  )}
                </div>
              </div>
            </div>
          )}

          <div className="relative flex-1 flex flex-col min-w-0 min-h-0">
            {wordmarkBg}

            {showcase.length > 0 ? (
              <>
                <div
                  ref={queueRef}
                  className="scrollbar-hide w-full flex items-center gap-2 overflow-x-auto px-1.5 py-1.5 shrink-0"
                >
                  {showcase.map((s, i) => (
                    <button
                      key={`${s.name}-${i}`}
                      type="button"
                      title={s.level != null ? `${s.name} · Lv.${s.level}` : s.name}
                      onClick={(e) => {
                        e.stopPropagation()
                        setFocusIndex(i)
                      }}
                      className="rounded-full transition-all duration-300 shrink-0"
                      style={{
                        opacity: i === focusIndex ? 1 : 0.45,
                        transform: i === focusIndex ? 'scale(1.1)' : 'scale(1)',
                      }}
                    >
                      {roundAvatar(
                        s,
                        30,
                        i === focusIndex
                          ? theme.primary
                          : rarityRing(s.rarity, theme.border),
                        i === focusIndex ? 2 : 1.5,
                      )}
                    </button>
                  ))}
                </div>

                <div className="mt-2 flex items-center gap-4 shrink-0 overflow-hidden">
                  {[
                    ...(score ? [{ label: score.label, value: score.value }] : []),
                    ...(data?.highlights.slice(0, 3) || []),
                  ].map((h) => (
                    <div
                      key={`${h.label}-${h.value}`}
                      className="flex flex-col items-center justify-center gap-1 shrink-0"
                    >
                      <span
                        className="text-gray-500 dark:text-gray-400 leading-none whitespace-nowrap"
                        style={{ fontSize: `${12 * fontScale}px` }}
                      >
                        {h.label}
                      </span>
                      <span
                        className="font-bold tabular-nums leading-none whitespace-nowrap"
                        style={{ fontSize: `${19 * fontScale}px`, color: theme.primary }}
                      >
                        {h.value}
                      </span>
                    </div>
                  ))}
                </div>
              </>
            ) : (
              <div
                className="flex-1 flex items-center justify-center text-center text-gray-400 dark:text-gray-500"
                style={{ fontSize: `${12 * fontScale}px` }}
              >
                {tw.showcaseEmpty}
              </div>
            )}

            <div className="flex-1 min-h-0" />

            <div className="flex items-center gap-2 shrink-0 min-w-0">
              {appIcon(30)}
              <div className="flex flex-col justify-center min-w-0 gap-0.5">
                <span
                  className="font-semibold text-gray-900 dark:text-gray-50 truncate leading-none"
                  style={{ fontSize: `${13 * fontScale}px` }}
                >
                  {name}
                </span>
                <span
                  className="tabular-nums text-gray-400 dark:text-gray-500 truncate leading-none"
                  style={{ fontSize: `${10 * fontScale}px` }}
                >
                  UID {accountId}
                </span>
              </div>
            </div>
          </div>
        </div>
      )
    }, [
      hasAccount,
      loading,
      data,
      error,
      fontScale,
      isEditMode,
      tw,
      accountId,
      game,
      meta,
      theme,
      focusIndex,
    ])

    return (
      <WidgetShell
        containerRef={setRefs}
        scale={scale}
        background={
          <GlowBackground
            color={iconColor}
            animLevel={anim.level}
            shouldAnimate={anim.loop}
            variant="single"
            size="md"
          />
        }
        contentClassName={`flex flex-col ${!isEditMode && hasAccount ? 'cursor-pointer' : ''}`}
        className={`select-none ${meta.fontClass}`}
        style={
          customFamily
            ? ({
                '--gp-font': `'${customFamily}', system-ui, sans-serif`,
              } as CSSProperties)
            : undefined
        }
      >
        <div
          className="h-full w-full"
          onClick={handleClick}
          onMouseDown={handlePressStart}
          onMouseUp={handlePressEnd}
          onMouseLeave={handlePressEnd}
          onTouchStart={handlePressStart}
          onTouchEnd={handlePressEnd}
          onTouchCancel={handlePressEnd}
        >
          {content}
        </div>

        <WidgetLongPressHint visible={isEditMode} title={tw.longPressHint} onClick={openSettings} />
      </WidgetShell>
    )
  },
)

GamePresenceWidget.displayName = 'GamePresenceWidget'

export { GamePresenceSettingsModal, GamePresenceWidget }
export default GamePresenceWidget
