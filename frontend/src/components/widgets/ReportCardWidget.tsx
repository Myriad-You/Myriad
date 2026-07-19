/**
 * 报告页平台卡片小组件 - 完整版（非阉割）
 * 完全复用 Reports.tsx 中的所有子组件实现
 */

import type { AnimationConfig } from '../../hooks/useAnimationLevel'
import type { WidgetConfig } from '../WidgetGrid'
import {
  FaBolt,
  FaGithub,
  FaPlay,
  FaSteam,
  FaTimes,
  FaXbox,
  FaXTwitter,
  LuGitFork,
  LuStar,
  SiBangumi,
  SiBilibili,
  SiDiscord,
  SiMyanimelist,
  SiNeteasecloudmusic,
  SiPlaystation,
} from '@lib/icons'

import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useNavigate } from 'react-router-dom'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { useLoopAnimation } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { extractColorsFromLoadedImage } from '../../utils/colorExtractor'
import {
  extractCardVisuals,
  findPlatformReport,
  hasRenderableCardVisuals,
  resolveReportPlatformId,
} from '../../utils/reportCardVisuals'
import { getLatestReportDeduped } from '../../utils/requestDedup'
import { RatingBadge } from '../RatingBadge'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetShell } from './shared/WidgetShell'

// 语言构成条分段类型
interface LangSegment {
  name: string
  pct: number
  delay: number
  duration: number
}

type ReportCardClickAction = 'report' | 'social'

interface SteamPresence {
  personastate?: number
  personastate_label?: string
  is_online?: boolean
  is_in_game?: boolean
  gameextrainfo?: string | null
  gameid?: string | null
  avatar?: string | null
  personaname?: string | null
  recent_2weeks_minutes?: number | null
}

// 🔧 性能优化：预生成热力图网格索引，避免在渲染时调用 Array.from
const HEATMAP_WEEKS = Array.from({ length: 12 }, (_, i) => i)
const HEATMAP_DAYS = Array.from({ length: 5 }, (_, i) => i)
const LANES_ARRAY = Array.from({ length: 5 }, (_, i) => i)

// ==================== 静态动画常量（避免每次渲染创建新对象）====================
// 弹幕动画 - 有限次数，配合调度器 duration=11000ms
const DANMAKU_INITIAL = { x: '100%', opacity: 0 }
const DANMAKU_ANIMATE = { x: '-100%', opacity: [0, 1, 1, 0] }
function createDanmakuTransition(duration: number, delay: number) {
  return {
    repeat: 0, // 只运行一轮，由调度器控制重新播放
    duration,
    delay,
    ease: 'linear' as const,
  }
}

// 内容切换动画
const CONTENT_FADE_INITIAL = { opacity: 0 }
const CONTENT_FADE_ANIMATE = { opacity: 1 }
const CONTENT_FADE_EXIT = { opacity: 0 }
const CONTENT_FADE_TRANSITION = { duration: 0.5 }

const CONTENT_SLIDE_INITIAL = { opacity: 0, y: 10 }
const CONTENT_SLIDE_ANIMATE = { opacity: 1, y: 0 }
const CONTENT_SLIDE_EXIT = { opacity: 0, y: -10 }
const CONTENT_SLIDE_TRANSITION = { duration: 0.5 }

export interface ReportCardWidgetProps {
  config: WidgetConfig
  isEditMode: boolean
  isPreview?: boolean
  /** 外部直接提供 card_visuals，提供时不再自行请求（用于报告页复用） */
  data?: any
  /** 去掉自带 glass 外壳与背景光效，供已有外壳的容器内嵌 */
  bare?: boolean
  /**
   * 外部控制概览/详情切换（如舞台模式按篇章驱动）。
   * 传入后禁用内部 10s 自动轮播，与外部状态完全同步。
   */
  showOverview?: boolean
  /** 小组件配置变更回调（用于持久化长按设置） */
  onConfigChange?: (newConfig: any) => void
}

// 各平台社交主页链接（长按设置里“打开社交主页”用）
const PLATFORM_SOCIAL: Record<
  string,
  { publicName: string; fieldKey: string; getUserUrl: (id: string) => string }
> = {
  bilibili: {
    publicName: 'Bilibili',
    fieldKey: 'uid',
    getUserUrl: (u) => `https://space.bilibili.com/${u}`,
  },
  steam: {
    publicName: 'Steam',
    fieldKey: 'steam_id',
    getUserUrl: (u) => `https://steamcommunity.com/profiles/${u}`,
  },
  github: {
    publicName: 'GitHub',
    fieldKey: 'username',
    getUserUrl: (u) => `https://github.com/${u}`,
  },
  netease: {
    publicName: 'Netease Music',
    fieldKey: 'user_id',
    getUserUrl: (u) => `https://music.163.com/#/user/home?id=${u}`,
  },
  bangumi: {
    publicName: 'Bangumi',
    fieldKey: 'username',
    getUserUrl: (u) => `https://bgm.tv/user/${u}`,
  },
  mal: {
    publicName: 'MyAnimeList',
    fieldKey: 'username',
    getUserUrl: (u) => `https://myanimelist.net/profile/${u}`,
  },
  x: {
    publicName: 'X',
    fieldKey: 'username',
    getUserUrl: (u) => `https://x.com/${String(u).replace(/^@/, '')}`,
  },
  xbox: {
    publicName: 'Xbox',
    fieldKey: 'gamertag',
    getUserUrl: (u) =>
      `https://www.xbox.com/play/user/${encodeURIComponent(u)}`,
  },
  psn: {
    publicName: 'PlayStation',
    fieldKey: 'online_id',
    getUserUrl: (u) =>
      `https://profile.playstation.com/me/profile/${encodeURIComponent(u)}`,
  },
  discord: {
    publicName: 'Discord',
    fieldKey: 'user_id',
    getUserUrl: (u) => `https://discord.com/users/${u}`,
  },
}

// 从公开配置取各平台用户ID（模块级缓存，避免重复请求）
let cachedUserIds: Record<string, string> | null = null
let userIdsPromise: Promise<Record<string, string>> | null = null
async function fetchPlatformUserIds(): Promise<Record<string, string>> {
  if (cachedUserIds) return cachedUserIds
  if (userIdsPromise) return userIdsPromise
  userIdsPromise = (async () => {
    const map: Record<string, string> = {}
    try {
      const res = await fetch(`${API_URL}/api/config/public`)
      if (res.ok) {
        const data = await res.json()
        if (Array.isArray(data.platforms)) {
          for (const p of data.platforms) {
            if (!p.enabled) continue
            const entry = Object.entries(PLATFORM_SOCIAL).find(
              ([, s]) => s.publicName === p.name,
            )
            if (!entry) continue
            const [pid, s] = entry
            const field = (p.config_fields || []).find(
              (f: { key: string; value?: string }) =>
                f.key === s.fieldKey && f.value,
            )
            if (field) map[pid] = field.value as string
          }
        }
      }
    } catch {
      // 静默：拿不到就走报告页兜底
    }
    cachedUserIds = map
    return map
  })()
  return userIdsPromise
}

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
      const res = await fetch(`${API_URL}/api/steam/presence`, {
        signal: AbortSignal.timeout(10000),
      })
      if (!res.ok) return null
      const body = await res.json()
      if (body?.success && body?.data) {
        cachedSteamPresence = body.data as SteamPresence
        cachedSteamPresenceAt = Date.now()
        return cachedSteamPresence
      }
    } catch {
      // Steam 状态属于增强信息，失败时保留报告卡片原内容。
    } finally {
      steamPresencePromise = null
    }
    return null
  })()

  return steamPresencePromise
}

/** Xbox 实时状态（OpenXBL presence，走 game/presence 公共接口） */
interface XboxPresence {
  gamertag?: string | null
  avatar?: string | null
  is_online?: boolean
  is_in_game?: boolean
  game_title?: string | null
  status?: string | null
  gamerscore?: number | null
}

const xboxPresenceCache = new Map<string, { data: XboxPresence; at: number }>()
const xboxPresenceInflight = new Map<string, Promise<XboxPresence | null>>()

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
      const params = new URLSearchParams({
        platform: 'xbox',
        id: key,
      })
      const res = await fetch(
        `${API_URL}/api/game/presence?${params.toString()}`,
        { signal: AbortSignal.timeout(12000) },
      )
      if (!res.ok) return null
      const body = await res.json()
      const d = body?.data
      if (!body?.success || !d) return null
      const status = String(d?.presence?.status || '').toLowerCase()
      const title = d?.presence?.title ? String(d.presence.title) : null
      const isOnline =
        status === 'online' || status === 'away' || status === 'busy'
      const isInGame = Boolean(title && title !== 'Home')
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
        status: d?.presence?.status || null,
        gamerscore: Number.isFinite(gs as number) ? (gs as number) : null,
      }
      xboxPresenceCache.set(key, { data: presence, at: Date.now() })
      return presence
    } catch {
      return null
    } finally {
      xboxPresenceInflight.delete(key)
    }
  })()
  xboxPresenceInflight.set(key, promise)
  return promise
}

/** 协议相对 / http 升 https（PSN 图标、通用外链图） */
function normalizeHttpsMediaUrl(url?: string | null): string | null {
  if (!url || typeof url !== 'string') return null
  let u = url.trim()
  if (!u) return null
  if (u.startsWith('//')) u = `https:${u}`
  if (u.startsWith('http://')) u = `https://${u.slice(7)}`
  return u
}

/**
 * Xbox / MS 商店图常给 http:// 或非 SSL 域名，HTTPS 页面会因混合内容被拦。
 * 统一升到 https，并把 images-eds → images-eds-ssl。
 */
function normalizeXboxMediaUrl(url?: string | null): string | null {
  const base = normalizeHttpsMediaUrl(url)
  if (!base) return null
  return base.replace(
    '://images-eds.xboxlive.com',
    '://images-eds-ssl.xboxlive.com',
  )
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

interface ReportCardSettingsModalState {
  isOpen: boolean
  selectedAction: ReportCardClickAction
  anchorRect?: DOMRect
  onSelect?: (action: ReportCardClickAction) => void
  onClose?: () => void
}

let reportCardSettingsModalState: ReportCardSettingsModalState = {
  isOpen: false,
  selectedAction: 'report',
}

const reportCardSettingsModalListeners: Set<() => void> = new Set()
const REPORT_CARD_SETTINGS_MODAL_WIDTH = 286
const REPORT_CARD_SETTINGS_MODAL_HEIGHT = 106
const REPORT_CARD_SETTINGS_MODAL_PADDING = 12

function openReportCardSettingsModal(
  selectedAction: ReportCardClickAction,
  anchorRect: DOMRect,
  onSelect: (action: ReportCardClickAction) => void,
  onClose?: () => void,
) {
  reportCardSettingsModalState = {
    isOpen: true,
    selectedAction,
    anchorRect,
    onSelect,
    onClose,
  }
  reportCardSettingsModalListeners.forEach((listener) => listener())
}

function closeReportCardSettingsModal() {
  const onClose = reportCardSettingsModalState.onClose
  reportCardSettingsModalState = {
    ...reportCardSettingsModalState,
    isOpen: false,
    onClose: undefined,
  }
  onClose?.()
  reportCardSettingsModalListeners.forEach((listener) => listener())
}

function subscribeToReportCardSettingsModal(listener: () => void) {
  reportCardSettingsModalListeners.add(listener)
  return () => {
    reportCardSettingsModalListeners.delete(listener)
  }
}

const ReportCardSettingsModal = memo(() => {
  const [, forceUpdate] = useState({})
  const { t } = useI18n()
  const modalRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    return subscribeToReportCardSettingsModal(() => {
      forceUpdate({})
    })
  }, [])

  const { isOpen, selectedAction, anchorRect, onSelect } =
    reportCardSettingsModalState

  const position = useMemo(() => {
    if (!anchorRect) return { top: 0, left: 0 }

    let top = anchorRect.bottom + 8
    let left =
      anchorRect.left +
      (anchorRect.width - REPORT_CARD_SETTINGS_MODAL_WIDTH) / 2

    if (
      left + REPORT_CARD_SETTINGS_MODAL_WIDTH >
      window.innerWidth - REPORT_CARD_SETTINGS_MODAL_PADDING
    ) {
      left =
        window.innerWidth -
        REPORT_CARD_SETTINGS_MODAL_WIDTH -
        REPORT_CARD_SETTINGS_MODAL_PADDING
    }
    if (left < REPORT_CARD_SETTINGS_MODAL_PADDING) {
      left = REPORT_CARD_SETTINGS_MODAL_PADDING
    }
    if (
      top + REPORT_CARD_SETTINGS_MODAL_HEIGHT >
      window.innerHeight - REPORT_CARD_SETTINGS_MODAL_PADDING
    ) {
      top = anchorRect.top - REPORT_CARD_SETTINGS_MODAL_HEIGHT - 8
    }
    if (top < REPORT_CARD_SETTINGS_MODAL_PADDING) {
      top = REPORT_CARD_SETTINGS_MODAL_PADDING
    }

    return { top, left }
  }, [anchorRect])

  useEffect(() => {
    if (!isOpen) return

    const handleClickOutside = (e: MouseEvent) => {
      if (modalRef.current && !modalRef.current.contains(e.target as Node)) {
        closeReportCardSettingsModal()
      }
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        closeReportCardSettingsModal()
      }
    }

    const timer = setTimeout(() => {
      document.addEventListener('mousedown', handleClickOutside, {
        passive: true,
      })
      document.addEventListener('keydown', handleKeyDown)
    }, 100)

    return () => {
      clearTimeout(timer)
      document.removeEventListener('mousedown', handleClickOutside)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [isOpen])

  const handleSelect = useCallback(
    (action: ReportCardClickAction) => {
      onSelect?.(action)
      closeReportCardSettingsModal()
    },
    [onSelect],
  )

  if (!isOpen) return null

  return createPortal(
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="fixed inset-0 z-10000"
      style={{ pointerEvents: 'none' }}
    >
      <motion.div
        ref={modalRef}
        initial={{ opacity: 0, scale: 0.95, y: -5 }}
        animate={{ opacity: 1, scale: 1, y: 0 }}
        exit={{ opacity: 0, scale: 0.95, y: -5 }}
        transition={{ duration: 0.15 }}
        className="absolute glass rounded-xl shadow-xl overflow-hidden border border-white/15 dark:border-white/10 p-3"
        style={{
          top: position.top,
          left: position.left,
          width: REPORT_CARD_SETTINGS_MODAL_WIDTH,
          pointerEvents: 'auto',
        }}
      >
        <div className="flex items-center justify-between gap-2 px-1 pb-2">
          <span className="text-sm font-bold text-gray-800 dark:text-gray-200">
            {t.platformCard.settingsTitle}
          </span>
          <button
            type="button"
            onClick={closeReportCardSettingsModal}
            className="w-5 h-5 flex items-center justify-center rounded-md hover:bg-black/5 dark:hover:bg-white/10 transition-colors"
            aria-label="Close"
          >
            <FaTimes className="w-2.5 h-2.5 text-gray-500" />
          </button>
        </div>

        <div className="flex gap-2.5">
          {(['social', 'report'] as const).map((action) => (
            <button
              key={action}
              type="button"
              onClick={() => handleSelect(action)}
              className={`flex-1 px-4 py-3 rounded-lg text-xs font-bold text-center transition-all ${
                selectedAction === action
                  ? 'bg-blue-500 text-white shadow-sm'
                  : 'bg-black/5 dark:bg-white/10 text-gray-700 dark:text-gray-200 hover:bg-black/10 dark:hover:bg-white/15'
              }`}
            >
              {action === 'social'
                ? t.platformCard.clickToSocial
                : t.platformCard.clickToReport}
            </button>
          ))}
        </div>
      </motion.div>
    </motion.div>,
    document.body,
  )
})

ReportCardSettingsModal.displayName = 'ReportCardSettingsModal'

// ==================== 工具函数 ====================
function getBilibiliProxyUrl(cover?: string, title?: string): string {
  if (!cover) {
    return `https://ui-avatars.com/api/?name=${encodeURIComponent(title || 'B')}&size=400&background=00A1D6&color=fff`
  }
  if (cover.startsWith('/api/proxy/')) return cover
  if (cover.includes('hdslb.com') || cover.includes('bilibili.com')) {
    return `${API_URL || ''}/api/proxy/image?url=${encodeURIComponent(cover)}`
  }
  return cover
}

function useLibraryItemRotation(libraryItems: any[], showOverview: boolean) {
  const [currentItemIndex, setCurrentItemIndex] = useState(0)
  const prevShowOverviewRef = useRef(showOverview)

  useEffect(() => {
    // 当从概览模式切换到库项目模式时，更新索引
    if (
      prevShowOverviewRef.current &&
      !showOverview &&
      libraryItems.length > 0
    ) {
      setCurrentItemIndex((prev) => (prev + 1) % libraryItems.length)
    }
    prevShowOverviewRef.current = showOverview
  }, [showOverview, libraryItems.length])

  return { currentItem: libraryItems[currentItemIndex], currentItemIndex }
}

// ==================== B站组件（完整版）====================
const DanmakuWidget = memo(
  ({
    data,
    allowLoop = true,
    triggerKey,
  }: {
    data?: { danmaku?: string[] }
    allowLoop?: boolean
    triggerKey?: unknown
  }) => {
    const { t } = useI18n()
    const defaultDanmaku = t.reportCard.danmakuDefault as unknown as string[]
    const texts = useMemo(
      () => data?.danmaku || defaultDanmaku,
      [data?.danmaku, defaultDanmaku],
    )

    // 🆕 使用触发式动画 - triggerKey 变化时播放一轮，完成后自动释放
    useLoopAnimation({
      duration: 11000, // 弹幕滚动约8秒 + 额外保持3秒
      trigger: triggerKey, // 状态切换时触发
      enabled: allowLoop, // 低端设备禁用
    })

    // 🆕 低性能模式：限制弹幕数量不超过3条
    // 🔧 用 useMemo 锁定：仅在 loop 状态变化时重算随机，避免每次渲染重新洗牌弹幕
    const maxDanmakuCount = useMemo(
      () =>
        allowLoop
          ? Math.random() < 0.7
            ? Math.random() < 0.5
              ? 3
              : 4
            : 5
          : 3,
      [allowLoop],
    )

    const animations = useMemo(() => {
      // 🔧 使用预生成的 LANES_ARRAY 进行洗牌
      const availableLanes = [...LANES_ARRAY]
      for (let i = availableLanes.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1))
        ;[availableLanes[i], availableLanes[j]] = [
          availableLanes[j],
          availableLanes[i],
        ]
      }
      return texts.slice(0, maxDanmakuCount).map((_, i) => ({
        duration: 6 + Math.random() * 4,
        delay: i * 0.7 + Math.random() * 0.5,
        top: `${10 + availableLanes[i] * 18}%`,
        opacity: 0.4 + Math.random() * 0.3,
      }))
    }, [texts, maxDanmakuCount])

    return (
      <div className="relative h-full w-full overflow-hidden">
        {animations.map((anim, i) => (
          <motion.div
            key={`${texts[i]}-${i}`}
            initial={DANMAKU_INITIAL}
            animate={DANMAKU_ANIMATE}
            transition={createDanmakuTransition(anim.duration, anim.delay)}
            className="absolute whitespace-nowrap text-base font-bold danmaku-text-color gpu-accelerated"
            style={{
              top: anim.top,
              opacity: anim.opacity,
            }}
          >
            {texts[i]}
          </motion.div>
        ))}
      </div>
    )
  },
)
DanmakuWidget.displayName = 'DanmakuWidget'

const BilibiliWidget = memo(
  ({ data, showOverview, onContentChange, allowLoop = true }: any) => {
    const libraryItems = useMemo(
      () => data?.library_items || [],
      [data?.library_items],
    )
    const { currentItem, currentItemIndex } = useLibraryItemRotation(
      libraryItems,
      showOverview,
    )

    useEffect(() => {
      if (!showOverview && currentItem) {
        onContentChange?.({ title: currentItem.title, type: currentItem.type })
      } else {
        onContentChange?.(null)
      }
    }, [showOverview, currentItem, onContentChange])

    return (
      <AnimatePresence mode="wait">
        {showOverview || !currentItem ? (
          <motion.div
            key="danmaku"
            initial={CONTENT_FADE_INITIAL}
            animate={CONTENT_FADE_ANIMATE}
            exit={CONTENT_FADE_EXIT}
            transition={CONTENT_FADE_TRANSITION}
            className="h-full w-full"
          >
            <DanmakuWidget
              data={data}
              allowLoop={allowLoop}
              triggerKey={showOverview}
            />
          </motion.div>
        ) : (
          <motion.div
            key={`lib-${currentItemIndex}`}
            initial={CONTENT_SLIDE_INITIAL}
            animate={CONTENT_SLIDE_ANIMATE}
            exit={CONTENT_SLIDE_EXIT}
            transition={CONTENT_SLIDE_TRANSITION}
            className="h-full w-full p-1.5"
          >
            <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
              <div className="absolute inset-0">
                <img
                  src={getBilibiliProxyUrl(
                    currentItem.cover,
                    currentItem.title,
                  )}
                  alt={currentItem.title}
                  className="w-full h-full object-cover"
                  loading="lazy"
                />
                <div className="absolute inset-0 bg-linear-to-t from-black/80 via-black/40 to-transparent" />
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    )
  },
)

// ==================== Steam组件（完整版）====================
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

// 分数滚动计数：一次性 rAF 动画，duration<=0 时直接返回终值（降级/低端设备）
// 同一个值驱动数字与进度条宽度，保证两者完全同步；
// delay 让计数等卡片入场动画完成后再开跑，增长过程不会被淡入盖掉
function useCountUp(value: number, duration = 800, delay = 0) {
  const [display, setDisplay] = useState(() => (duration > 0 ? 0 : value))

  useEffect(() => {
    if (duration <= 0) {
      setDisplay(value)
      return
    }
    let raf = 0
    const start = performance.now() + delay
    const tick = (now: number) => {
      // delay 期间 p 被夹在 0，setState(0) 与旧值相同时 React 会跳过重渲染
      const p = Math.min(Math.max((now - start) / duration, 0), 1)
      const eased = 1 - (1 - p) ** 3
      setDisplay(Math.round(value * eased))
      if (p < 1) raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [value, duration, delay])

  return display
}

// 评分能量条分段数
const SCORE_BAR_SEGMENTS = 10

// 评分卡内容：抽成组件，使计数/进度条在每次轮播入场时重新播放
const ScoreCardBody = memo(
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
    // 延迟 300ms 起跑：等卡片与分数行入场完成，增长过程完整可见
    const displayScore = useCountUp(
      score,
      Math.round(900 * anim.durationScale),
      300,
    )
    const pct = Math.min(Math.max(displayScore, 0), 100)

    return (
      <>
        {/* 标题「游戏力评分」+ 分段能量条（缩短，与标题同排） */}
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

        {/* 分数 + 类型标签（放大，与分数同排） */}
        <motion.div
          className="flex items-center justify-between gap-2.5"
          initial={{ opacity: 0, y: 6 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3, delay: 0.2 }}
        >
          <span className="flex shrink-0 items-baseline gap-0.5">
            {/* tabular-nums：计数过程数字等宽，右侧内容不抖动 */}
            <span className="text-3xl font-black leading-none tracking-tight tabular-nums text-gray-800 dark:text-gray-100">
              {displayScore}
            </span>
            <span className="text-[11px] font-bold text-gray-500 dark:text-gray-400">
              /100
            </span>
          </span>
          {/* 类型标签：切角徽章，像游戏内稀有度/成就标签 */}
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

const SteamStatsWidget = memo(({ data }: any) => {
  const { t } = useI18n()
  const anim = useAnimationLevel()
  const fallbackPresence = useMemo(() => getSteamPresenceFromData(data), [data])
  const [livePresence, setLivePresence] = useState<SteamPresence | null>(null)
  const score = useMemo(() => data?.hardcore_score || 0, [data])
  const type = useMemo(
    () => data?.player_type || t.reportCard.casualPlayer,
    [data, t.reportCard.casualPlayer],
  )
  const gamesCount = useMemo(() => data?.games_count || 0, [data])
  const totalPlaytime = useMemo(() => {
    const hours = data?.total_playtime || 0
    return hours >= 1000 ? `${(hours / 1000).toFixed(1)}k` : hours.toString()
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
    // Steam 同一 hash 有 无后缀(32) / _medium(64) / _full(184) 三种尺寸，
    // 统一升到 _full，避免拿到小图放大发糊
    return raw.replace(/(_full|_medium)?\.(jpg|png)(\?.*)?$/i, '_full.$2$3')
  }, [presence])
  const isLive = Boolean(presence?.is_online || presence?.is_in_game)
  const nowPlaying =
    presence?.is_in_game && presence?.gameextrainfo
      ? presence.gameextrainfo
      : null
  // 正在玩的游戏图标：用 appid 取 Steam 商店头图（方形裁切），无 appid 时回退到 Steam 图标
  const gameIconUrl =
    nowPlaying && presence?.gameid
      ? `https://cdn.cloudflare.steamstatic.com/steam/apps/${presence.gameid}/header.jpg`
      : null
  // 近两周游玩时长（小时），无数据时不显示该项
  const recent2wHours = useMemo(() => {
    const minutes = presence?.recent_2weeks_minutes
    if (typeof minutes !== 'number' || minutes <= 0) return null
    const hours = minutes / 60
    return hours >= 10 ? Math.round(hours).toString() : hours.toFixed(1)
  }, [presence])
  // 右列三项统计（顶对齐分数、底对齐内边距，justify-between 均布）
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
  // 底部卡槽轮播：游戏中在「正在玩卡」与「评分卡」间循环，不玩时停在评分卡。
  // 低端设备/减少动画时不轮播：游戏中固定正在玩卡（信息优先）。
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
    let cancelled = false

    const refreshPresence = async () => {
      // 后台标签页跳过请求，回到前台后由下一个 interval tick 恢复
      if (document.hidden) return
      const nextPresence = await fetchSteamPresence()
      if (!cancelled && nextPresence) {
        setLivePresence(nextPresence)
      }
    }

    refreshPresence()
    // 仅在线状态需要实时性，120s 一次足够；后端有 120s 共享缓存，多访客不会各自打 Steam
    const intervalId = window.setInterval(refreshPresence, 120 * 1000)

    return () => {
      cancelled = true
      window.clearInterval(intervalId)
    }
  }, [])

  return (
    <div className="relative h-full w-full overflow-hidden">
      {/* 背景：Steam 亮蓝对角渐变 */}
      <div className="absolute inset-0 bg-linear-to-br from-[#66c0f4]/25 via-[#66c0f4]/8 to-transparent dark:from-[#66c0f4]/15 dark:via-[#66c0f4]/5 clip-diagonal" />

      {/* 主体：身份块在顶、轮播卡槽沉底，justify-between 撑出中部呼吸带 */}
      <div className="relative z-10 flex h-full flex-col justify-between p-4">
        {/* 身份块：头像 + （昵称/徽章同行 + 指标 tag 行） */}
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
                className="h-11 w-11 rounded-xl object-cover shadow-md ring-1 ring-black/10 dark:ring-white/15"
                loading="lazy"
                decoding="async"
              />
            ) : (
              <div className="flex h-11 w-11 items-center justify-center rounded-xl bg-gray-200/70 ring-1 ring-black/10 dark:bg-white/10 dark:ring-white/15">
                <FaSteam className="h-5 w-5 text-gray-400 dark:text-gray-500" />
              </div>
            )}
            {/* 状态点：头像右下角，在线时外圈呼吸扩散 */}
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
            {/* 昵称；文字描边补足 CJK 字重 */}
            {presence?.personaname && (
              <span
                className="truncate text-base font-black tracking-tight text-gray-800 dark:text-gray-100"
                style={{ WebkitTextStroke: '0.4px currentcolor' }}
              >
                {presence.personaname}
              </span>
            )}
            {/* 三项指标：退化为无背景 tag 行 */}
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

        {/* 底部卡槽：评分卡 ⇄ 正在玩卡 循环轮播。
            pl 约等于 头像(44)+gap(12) 让左缘对齐昵称文本、越过浮动 Logo；
            整体下移 3px 与上方指标行拉开距离 */}
        <div className="translate-y-[3px] pl-13">
          <div className="relative h-16">
            <AnimatePresence mode="wait">
              {showNowPlaying ? (
                // 正在玩卡：满宽封面横幅 + 压暗渐变 + 播放角标/游戏名
                <motion.div
                  key="playing"
                  className="absolute inset-0 overflow-hidden rounded-xl shadow-sm ring-1 ring-black/10 dark:ring-white/15"
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
                // 评分卡：类型 + 分数进度条（横向卡片专属，取代圆环）
                <motion.div
                  key="score"
                  className="absolute inset-0 flex flex-col justify-center gap-1 rounded-xl bg-white/45 px-3.5 ring-1 ring-black/5 backdrop-blur-md dark:bg-white/8 dark:ring-white/10"
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

const SteamWidget = memo(({ data, showOverview, onContentChange }: any) => {
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
          <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
            <div className="absolute inset-0">
              <img
                src={
                  currentItem.cover ||
                  `https://ui-avatars.com/api/?name=${encodeURIComponent(currentItem.title)}&size=400&background=1b2838&color=fff`
                }
                alt={currentItem.title}
                className="w-full h-full object-cover"
                loading="lazy"
              />
              <div className="absolute inset-0 bg-linear-to-t from-black/80 via-black/40 to-transparent" />
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
})

// ==================== GitHub组件（完整版）====================
const GithubStatsWidget = memo(({ data }: any) => {
  const { t } = useI18n()
  const langs = useMemo(() => data?.languages || [], [data?.languages])
  // 语言构成条：模仿 Bangumi 类型占比设计，各色段首尾相接连续填充
  const langSegments = useMemo<LangSegment[]>(() => {
    const items = langs
      .filter((l: any) => l.percentage > 0)
      .sort((a: any, b: any) => b.percentage - a.percentage)
      .slice(0, 4)
    const total = items.reduce((sum: number, l: any) => sum + l.percentage, 0)
    if (total === 0) return []
    const fillDuration = 0.9
    const baseDelay = 0.55
    let acc = 0
    return items.map((lang: any) => {
      const segment: LangSegment = {
        name: lang.name as string,
        pct: (lang.percentage / total) * 100,
        delay: baseDelay + (acc / total) * fillDuration,
        duration: (lang.percentage / total) * fillDuration,
      }
      acc += lang.percentage
      return segment
    })
  }, [langs])
  const level = useMemo(
    () => data?.contribution_level || t.reportCard.beginnerDev,
    [data?.contribution_level, t.reportCard.beginnerDev],
  )
  const contributions = useMemo(
    () => data?.total_contributions || 0,
    [data?.total_contributions],
  )
  const reposCount = useMemo(() => data?.repos_count || 0, [data?.repos_count])
  const totalStars = useMemo(() => data?.total_stars || 0, [data?.total_stars])
  const contributionCalendar = useMemo(
    () => data?.contribution_calendar,
    [data?.contribution_calendar],
  )

  const levelColor = useMemo(() => {
    const colorMap: { [key: string]: string } = {
      [t.reportCardWidget.beginnerDev]: '#22c55e',
      [t.reportCardWidget.intermediateDev]: '#3b82f6',
      [t.reportCardWidget.seniorDev]: '#a855f7',
      [t.reportCardWidget.veteranDev]: '#f97316',
      [t.reportCardWidget.legendaryDev]: '#ef4444',
    }
    return colorMap[level] || '#6b7280'
  }, [level, t])

  const generateHeatmapGrid = () => {
    const grid: Array<{
      week: number
      day: number
      opacity: number
      count: number
    }> = []

    if (contributionCalendar && Array.isArray(contributionCalendar)) {
      const recentDays = contributionCalendar.slice(-60)
      const maxCount = Math.max(...recentDays.map((d: any) => d.count || 0), 1)

      for (let week = 0; week < 12; week++) {
        for (let day = 0; day < 5; day++) {
          const index = week * 5 + day
          const dayData = recentDays[index]
          const count = dayData?.count || 0
          const opacity =
            count > 0 ? Math.min((count / maxCount) * 0.85 + 0.15, 1) : 0.12
          grid.push({ week, day, opacity, count })
        }
      }
    } else {
      const avgPerDay = contributions / 365
      for (let week = 0; week < 12; week++) {
        for (let day = 0; day < 5; day++) {
          const lambda = avgPerDay * (0.5 + Math.random())
          const count = Math.floor(-Math.log(1 - Math.random()) * lambda)
          const opacity =
            count > 0
              ? Math.min((count / (avgPerDay * 2)) * 0.7 + 0.15, 1)
              : 0.12
          grid.push({ week, day, opacity, count })
        }
      }
    }
    return grid
  }

  const heatmapData = useMemo(
    () => generateHeatmapGrid(),
    [contributionCalendar, contributions],
  )

  const getLanguageColor = (lang: string) => {
    const colorMap: { [key: string]: string } = {
      TypeScript: '#3178c6',
      JavaScript: '#f1e05a',
      Python: '#3572A5',
      Rust: '#dea584',
      Go: '#00ADD8',
      Java: '#b07219',
      'C++': '#f34b7d',
      'C#': '#178600',
      Ruby: '#701516',
      PHP: '#4F5D95',
    }
    return colorMap[lang] || levelColor
  }

  return (
    <div className="relative h-full w-full overflow-hidden">
      <div className="relative h-full flex flex-col p-2 justify-between">
        <div className="space-y-2">
          <div className="flex items-start justify-between">
            <div className="flex flex-col gap-2 items-start">
              <motion.div
                className="px-2 py-0.5 rounded-md text-[9px] font-bold flex items-center gap-1 shadow-sm w-fit"
                style={{
                  backgroundColor: `${levelColor}20`,
                  color: levelColor,
                  border: `1px solid ${levelColor}30`,
                }}
                initial={{ scale: 0.8, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                transition={{ duration: 0.3, delay: 0.2 }}
              >
                <span className="text-[7px]">●</span>
                <span>{level}</span>
              </motion.div>
              <div className="flex flex-col gap-1.5">
                <motion.div
                  className="flex items-baseline gap-1.5"
                  initial={{ y: 10, opacity: 0 }}
                  animate={{ y: 0, opacity: 1 }}
                  transition={{ duration: 0.4, delay: 0.3 }}
                >
                  <span className="text-2xl font-black text-gray-800 dark:text-gray-100 leading-none">
                    {contributions}
                  </span>
                  <span className="text-[9px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold">
                    {t.reportsPage.commits}
                  </span>
                </motion.div>
                <motion.div
                  className="flex items-baseline gap-1.5"
                  initial={{ y: 10, opacity: 0 }}
                  animate={{ y: 0, opacity: 1 }}
                  transition={{ duration: 0.4, delay: 0.4 }}
                >
                  <span className="text-2xl font-black text-gray-800 dark:text-gray-100 leading-none">
                    {reposCount}
                  </span>
                  <span className="text-[9px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold">
                    {t.reportsPage.repos}
                  </span>
                </motion.div>
              </div>
            </div>
            <div className="flex flex-col items-end gap-1.5">
              <div className="flex gap-[2.5px]">
                {HEATMAP_WEEKS.map((week) => (
                  <div key={week} className="flex flex-col gap-[2.5px]">
                    {HEATMAP_DAYS.map((day) => {
                      // heatmapData 按 week*5+day 顺序生成，直接下标取，避免 O(n²) find
                      const cell = heatmapData[week * 5 + day]
                      return (
                        <motion.div
                          key={`${week}-${day}`}
                          className="w-2.5 h-2.5 rounded-0.5"
                          style={{
                            backgroundColor: levelColor,
                            opacity: cell?.opacity || 0.15,
                          }}
                          initial={{ scale: 0, opacity: 0 }}
                          animate={{ scale: 1, opacity: cell?.opacity || 0.15 }}
                          transition={{
                            duration: 0.2,
                            delay: (week * 5 + day) * 0.004,
                          }}
                        />
                      )
                    })}
                  </div>
                ))}
              </div>
              {totalStars > 0 && (
                <motion.div
                  className="px-1.5 py-0.5 rounded-full text-[9px] font-bold flex items-center gap-1"
                  style={{
                    backgroundColor: `${levelColor}1a`,
                    color: levelColor,
                  }}
                  initial={{ scale: 0.8, opacity: 0 }}
                  animate={{ scale: 1, opacity: 1 }}
                  transition={{ duration: 0.3, delay: 0.5 }}
                >
                  <LuStar size={9} />
                  <span>
                    {totalStars >= 1000
                      ? `${(totalStars / 1000).toFixed(1)}k`
                      : totalStars}
                  </span>
                </motion.div>
              )}
            </div>
          </div>
        </div>
        {langSegments.length > 0 && (
          <div className="absolute bottom-3 right-3 w-[45%] flex flex-col items-end gap-1">
            <div className="flex flex-wrap justify-end gap-x-2.5 gap-y-0.5">
              {langSegments.map((segment) => (
                <motion.span
                  key={segment.name}
                  className="flex items-center gap-1 text-[8px] font-bold text-gray-600 dark:text-gray-300"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={{ duration: 0.3, delay: segment.delay }}
                >
                  <span
                    className="w-1.5 h-1.5 rounded-full"
                    style={{ backgroundColor: getLanguageColor(segment.name) }}
                  />
                  {segment.name}
                  <span className="font-mono text-gray-500 dark:text-gray-400">
                    {Math.round(segment.pct)}%
                  </span>
                </motion.span>
              ))}
            </div>
            <div className="flex h-1 w-full rounded-full overflow-hidden bg-gray-200/80 dark:bg-white/10 ring-1 ring-black/5 dark:ring-white/10">
              {langSegments.map((segment) => (
                <motion.div
                  key={segment.name}
                  className="h-full"
                  style={{ backgroundColor: getLanguageColor(segment.name) }}
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
  )
})

const GithubWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const libraryItems = useMemo(
    () => data?.library_items || [],
    [data?.library_items],
  )
  const { currentItem, currentItemIndex } = useLibraryItemRotation(
    libraryItems,
    showOverview,
  )

  useEffect(() => {
    if (!showOverview && currentItem) {
      onContentChange?.({
        title: currentItem.title,
        type: currentItem.language || 'repo',
      })
    } else {
      onContentChange?.(null)
    }
  }, [showOverview, currentItem, onContentChange])

  return (
    <AnimatePresence mode="wait">
      {showOverview || !currentItem || libraryItems.length === 0 ? (
        <motion.div
          key="stats"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.5 }}
          className="h-full w-full"
        >
          <GithubStatsWidget data={data} />
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
          <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
            <div className="absolute inset-0 bg-linear-to-br from-gray-800 to-gray-900 dark:from-black dark:to-black/90">
              <div className="absolute inset-0 flex flex-col p-2.5 pb-[20%]">
                <div className="flex items-center gap-2.5 mb-2">
                  {currentItem.stars !== undefined && (
                    <div className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-gray-700/50">
                      <LuStar size={10} className="text-amber-400" />
                      <span className="text-[10px] font-bold text-gray-100">
                        {currentItem.stars >= 1000
                          ? `${(currentItem.stars / 1000).toFixed(1)}k`
                          : currentItem.stars}
                      </span>
                    </div>
                  )}
                  {currentItem.forks !== undefined && (
                    <div className="flex items-center gap-1 px-1.5 py-0.5 rounded bg-gray-700/50">
                      <LuGitFork size={10} className="text-gray-100" />
                      <span className="text-[10px] font-bold text-gray-100">
                        {currentItem.forks >= 1000
                          ? `${(currentItem.forks / 1000).toFixed(1)}k`
                          : currentItem.forks}
                      </span>
                    </div>
                  )}
                </div>
                {currentItem.description && (
                  <div className="text-[10px] leading-snug text-gray-200 line-clamp-4 px-1">
                    {currentItem.description}
                  </div>
                )}
              </div>
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
})

// ==================== Netease组件（完整版）====================
const MusicStatsWidget = memo(
  ({
    data,
    allowLoop = true,
    triggerKey,
  }: {
    data?: any
    allowLoop?: boolean
    triggerKey?: unknown
  }) => {
    const { t } = useI18n()

    // 🆕 使用触发式动画 - triggerKey 变化时播放一轮，完成后自动释放
    const { isAnimating } = useLoopAnimation({
      duration: 5000, // 气泡动画约5秒
      trigger: triggerKey, // 状态切换时触发
      enabled: allowLoop, // 低端设备禁用
    })

    const canAnimate = allowLoop && isAnimating

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
              <motion.div
                key={bubble.tag}
                className="absolute flex items-center justify-center rounded-full font-bold backdrop-blur-[1px] pointer-events-auto cursor-default"
                style={{
                  left: `${bubble.x}%`,
                  top: `${bubble.y}%`,
                  width: `${bubble.size}px`,
                  height: `${bubble.size}px`,
                  marginLeft: `-${bubble.size / 2}px`,
                  marginTop: `-${bubble.size / 2}px`,
                  background: `radial-gradient(120% 120% at 30% 30%, rgba(255,255,255,0.6) 0%, ${bubble.color}20 20%, ${bubble.color}60 100%)`,
                  border: `1px solid rgba(255,255,255,0.3)`,
                  color: bubble.color,
                  fontSize: `${Math.min(Math.max(10, bubble.size / 4), 16)}px`,
                  textShadow: `0 1px 1px rgba(255,255,255,0.8)`,
                  zIndex: 10,
                  willChange: 'transform', // GPU 加速
                  transform: 'translateZ(0)',
                  backfaceVisibility: 'hidden',
                }}
                initial={{ scale: 0, opacity: 0 }}
                animate={{
                  scale: 1,
                  opacity: 1,
                  y: [0, -8, 0, 8, 0],
                  // 移除动态 boxShadow 动画，使用静态样式代替
                }}
                transition={{
                  scale: {
                    type: 'spring',
                    stiffness: 260,
                    damping: 20,
                    delay: i * 0.1,
                  },
                  opacity: { duration: 0.6, delay: i * 0.1 },
                  y: {
                    duration: bubble.floatDuration,
                    repeat: canAnimate ? Infinity : 0,
                    ease: 'easeInOut',
                    delay: bubble.floatDelay,
                  },
                }}
                whileHover={{
                  scale: 1.15,
                  zIndex: 50,
                  transition: { duration: 0.3, ease: 'easeOut' },
                }}
              >
                <div className="absolute top-[15%] left-[15%] w-[20%] h-[10%] bg-white/30 rounded-full blur-[1px] transform -rotate-45" />
                <span className="relative z-10 mix-blend-multiply dark:mix-blend-normal">
                  {bubble.tag}
                </span>
              </motion.div>
            ))}
          </div>
          <div className="absolute bottom-3 right-3 flex flex-col items-end gap-2 z-20">
            <motion.div
              className="px-2.5 py-0.5 rounded-full text-[9px] font-bold flex items-center gap-1 backdrop-blur-md shadow-lg bg-linear-to-br from-red-50 to-red-100 dark:from-red-950/80 dark:to-red-900/60 text-red-600 dark:text-red-300"
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
              className="flex items-center gap-2 px-3 py-1.5 rounded-xl backdrop-blur-md shadow-lg bg-white/90 dark:bg-black/90 border border-white/30 dark:border-white/10"
              style={{ backdropFilter: 'blur(10px)' }}
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

const NeteaseWidget = memo(
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
    const [currentItemIndex, setCurrentItemIndex] = useState(0)
    const prevShowOverviewRef = useRef(showOverview)

    // 当从概览切换到库项目模式时，立即更新索引
    useEffect(() => {
      if (
        prevShowOverviewRef.current &&
        !showOverview &&
        libraryItems.length > 0
      ) {
        setCurrentItemIndex((prev) => (prev + 2) % libraryItems.length)
      }
      prevShowOverviewRef.current = showOverview
    }, [showOverview, libraryItems.length])

    // 在非概览模式下，定时轮换项目 - timeout 链 + 可见性暂停
    useEffect(() => {
      if (!showOverview && libraryItems.length > 0) {
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
    }, [showOverview, libraryItems.length])

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
              triggerKey={showOverview}
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
                  <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
                    <div className="absolute inset-0">
                      <img
                        src={
                          item.cover ||
                          `https://ui-avatars.com/api/?name=${encodeURIComponent(item.title)}&size=200&background=e60026&color=fff`
                        }
                        alt={item.title}
                        className="w-full h-full object-cover"
                        loading="lazy"
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

// ==================== 平台配置 ====================
// ==================== Xbox / PSN 共用：成就/奖杯型标题轮播 ====================
// 两个平台都没有时长数据，卡片走"成就完成度"叙事：
// 概览 = 核心分数 + 完成度统计；详情 = 作品完成度轮播。

const TrophyTitleRow = memo(
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

/** 成就/奖杯向报告卡的通用骨架，Xbox / PSN 以配色和统计项区分 */
const AchievementReportBody = memo(
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

// ==================== Xbox：对齐 Steam 卡的身份+指标+底槽结构 ====================
// 叙事：成就向（无时长）。概览 = 头像/在线 + GS/库/成就 + 硬核指数/正在玩；
// 详情 = 作品封面轮播（带完成度角标）。

const XBOX_ACCENT_SOFT = '#3A9D23'

const XboxScoreCardBody = memo(
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

const XboxStatsWidget = memo(({ data }: any) => {
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
    // 显式有 hardcore_score 字段时信任后端（含 0）；缺失才前端兜底
    if (
      data?.hardcore_score !== undefined &&
      data?.hardcore_score !== null &&
      Number.isFinite(Number(data.hardcore_score))
    ) {
      return Math.min(100, Math.max(0, Math.round(Number(data.hardcore_score))))
    }
    // 旧报告无 hardcore_score 时用完成度/全成就/GS 做轻量兜底
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
    if (!gamertag) return
    let cancelled = false
    const refresh = async () => {
      if (document.hidden) return
      const next = await fetchXboxPresence(gamertag)
      if (!cancelled && next) setLivePresence(next)
    }
    refresh()
    const intervalId = window.setInterval(refresh, 120 * 1000)
    return () => {
      cancelled = true
      window.clearInterval(intervalId)
    }
  }, [gamertag])

  // 无 gamertag 时尝试从公开配置取
  useEffect(() => {
    if (gamertag) return
    let cancelled = false
    fetchPlatformUserIds().then((ids) => {
      if (cancelled || !ids.xbox) return
      fetchXboxPresence(ids.xbox).then((next) => {
        if (!cancelled && next) setLivePresence(next)
      })
    })
    return () => {
      cancelled = true
    }
  }, [gamertag])

  const displayName = livePresence?.gamertag || gamertag || 'Xbox'
  const avatarUrl = livePresence?.avatar || fallbackAvatar
  const isLive = Boolean(livePresence?.is_online || livePresence?.is_in_game)
  const nowPlaying =
    livePresence?.is_in_game && livePresence?.game_title
      ? livePresence.game_title
      : null
  const presenceColor = livePresence?.is_in_game
    ? XBOX_ACCENT_SOFT
    : livePresence?.is_online
      ? '#22c55e'
      : '#9ca3af'
  const liveGs =
    livePresence?.gamerscore != null && livePresence.gamerscore > 0
      ? livePresence.gamerscore
      : gamerscore

  // 主指标 + 副指标同一行：游戏数 / GS / 成就 / 完成度 / 全成就
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

  // 底槽：有正在玩时在「正在玩」与「猎人指数」间轮播
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
      {/* 背景：Xbox 绿对角渐变 */}
      <div className="absolute inset-0 bg-linear-to-br from-[#107C10]/25 via-[#107C10]/8 to-transparent dark:from-[#107C10]/18 dark:via-[#107C10]/5 clip-diagonal" />

      <div className="relative z-10 flex h-full flex-col justify-between p-4">
        {/* 身份块：头像 + 昵称 + 指标 tag（主+副同一行） */}
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
              livePresence?.status ? String(livePresence.status) : undefined
            }
          >
            {safeAvatarUrl ? (
              <img
                src={safeAvatarUrl}
                alt={displayName}
                className="h-11 w-11 rounded-xl object-cover shadow-md ring-1 ring-black/10 dark:ring-white/15"
                loading="lazy"
                decoding="async"
              />
            ) : (
              <div className="flex h-11 w-11 items-center justify-center rounded-xl bg-[#107C10]/15 ring-1 ring-black/10 dark:bg-[#107C10]/25 dark:ring-white/15">
                <FaXbox className="h-5 w-5 text-[#107C10]" />
              </div>
            )}
            {/* 在线状态点 */}
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

        {/* 底部卡槽：猎人指数 ⇄ 正在玩 */}
        <div className="translate-y-[3px] pl-13">
          <div className="relative h-16">
            <AnimatePresence mode="wait">
              {showNowPlaying ? (
                <motion.div
                  key="playing"
                  className="absolute inset-0 overflow-hidden rounded-xl shadow-sm ring-1 ring-black/10 dark:ring-white/15"
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
                  className="absolute inset-0 flex flex-col justify-center gap-1 rounded-xl bg-white/45 px-3.5 ring-1 ring-black/5 backdrop-blur-md dark:bg-white/8 dark:ring-white/10"
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

/** PSN 实时状态（走 game/presence） */
interface PsnPresence {
  online_id?: string | null
  avatar?: string | null
  is_online?: boolean
  is_in_game?: boolean
  game_title?: string | null
  status?: string | null
  trophy_level?: number | null
  platinum?: number | null
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
      const params = new URLSearchParams({ platform: 'psn', id: key })
      const res = await fetch(
        `${API_URL}/api/game/presence?${params.toString()}`,
        { signal: AbortSignal.timeout(12000) },
      )
      if (!res.ok) return null
      const body = await res.json()
      const d = body?.data
      if (!body?.success || !d) return null
      const status = String(d?.presence?.status || '').toLowerCase()
      const title = d?.presence?.title ? String(d.presence.title) : null
      const isOnline =
        status.includes('online') ||
        status === 'available' ||
        status === 'away' ||
        status === 'busy'
      const isInGame = Boolean(title)
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
        status: d?.presence?.status || null,
        trophy_level: Number.isFinite(lv as number) ? (lv as number) : null,
        platinum: Number.isFinite(plat as number) ? (plat as number) : null,
      }
      psnPresenceCache.set(key, { data: presence, at: Date.now() })
      return presence
    } catch {
      return null
    } finally {
      psnPresenceInflight.delete(key)
    }
  })()
  psnPresenceInflight.set(key, promise)
  return promise
}

const XboxWidget = memo(({ data, showOverview, onContentChange }: any) => {
  // 详情优先 library_items（带封面）；无则回退 top_titles。封面统一升 https。
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
    // 有封面的优先轮播；全无封面时仍展示文字进度
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
          <div className="relative h-full w-full overflow-hidden rounded-xl bg-white shadow-lg dark:bg-black/90">
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
              {/* 轻量底渐变即可；标题交给左下角浮动 Logo，避免与背景文字重复 */}
              <div className="absolute inset-0 bg-linear-to-t from-black/50 via-transparent to-transparent" />
            </div>
            {/* 完成度角标（右上）；标题只走 onContentChange → 左下 Logo */}
            {(progress > 0 || achTotal > 0) && (
              <div className="absolute top-2 right-2 z-10 flex items-center gap-1 rounded-md bg-black/55 px-1.5 py-0.5 backdrop-blur-sm ring-1 ring-white/15">
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
            {/* 底部进度条：pl 避开左下浮动 Logo */}
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

// ==================== PSN：对齐 Xbox/Steam 的身份+指标+底槽结构 ====================
// 叙事：奖杯向（无时长）。概览 = 头像/在线 + 白金/等级/库 + 猎人指数/正在玩；
// 详情 = 作品封面轮播（完成度 + 白金角标）。

const PSN_ACCENT_SOFT = '#3D9BFF'

const PsnScoreCardBody = memo(
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

const PsnStatsWidget = memo(({ data }: any) => {
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
    () =>
      typeof data?.avatar === 'string'
        ? normalizeHttpsMediaUrl(data.avatar)
        : null,
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
    if (!onlineId) return
    let cancelled = false
    const refresh = async () => {
      if (document.hidden) return
      const next = await fetchPsnPresence(onlineId)
      if (!cancelled && next) setLivePresence(next)
    }
    refresh()
    const intervalId = window.setInterval(refresh, 120 * 1000)
    return () => {
      cancelled = true
      window.clearInterval(intervalId)
    }
  }, [onlineId])

  useEffect(() => {
    if (onlineId) return
    let cancelled = false
    fetchPlatformUserIds().then((ids) => {
      if (cancelled || !ids.psn) return
      fetchPsnPresence(ids.psn).then((next) => {
        if (!cancelled && next) setLivePresence(next)
      })
    })
    return () => {
      cancelled = true
    }
  }, [onlineId])

  const displayName = livePresence?.online_id || onlineId || 'PlayStation'
  const avatarUrl =
    normalizeHttpsMediaUrl(livePresence?.avatar) || fallbackAvatar
  const isLive = Boolean(livePresence?.is_online || livePresence?.is_in_game)
  const nowPlaying =
    livePresence?.is_in_game && livePresence?.game_title
      ? livePresence.game_title
      : null
  const presenceColor = livePresence?.is_in_game
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
              livePresence?.status ? String(livePresence.status) : undefined
            }
          >
            {avatarUrl ? (
              <img
                src={avatarUrl}
                alt={displayName}
                className="h-11 w-11 rounded-xl object-cover shadow-md ring-1 ring-black/10 dark:ring-white/15"
                loading="lazy"
                decoding="async"
                referrerPolicy="no-referrer"
              />
            ) : (
              <div className="flex h-11 w-11 items-center justify-center rounded-xl bg-[#0070D1]/15 ring-1 ring-black/10 dark:bg-[#0070D1]/25 dark:ring-white/15">
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
                  className="absolute inset-0 overflow-hidden rounded-xl shadow-sm ring-1 ring-black/10 dark:ring-white/15"
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
                  className="absolute inset-0 flex flex-col justify-center gap-1 rounded-xl bg-white/45 px-3.5 ring-1 ring-black/5 backdrop-blur-md dark:bg-white/8 dark:ring-white/10"
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

const PsnWidget = memo(({ data, showOverview, onContentChange }: any) => {
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
          <div className="relative h-full w-full overflow-hidden rounded-xl bg-white shadow-lg dark:bg-black/90">
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
            {/* 右上：完成度 + 白金标记；标题只走左下 Logo */}
            {(progress > 0 || hasPlatinum) && (
              <div className="absolute top-2 right-2 z-10 flex items-center gap-1 rounded-md bg-black/55 px-1.5 py-0.5 backdrop-blur-sm ring-1 ring-white/15">
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

const PLATFORM_CONFIG: Record<
  string,
  {
    icon: React.ReactNode
    color: string
    bgColor: string
    borderColor: string
    label: string
    textColor: string
  }
> = {
  bilibili: {
    icon: <SiBilibili />,
    color: '#00A1D6',
    bgColor: 'rgba(0, 161, 214, 0.15)',
    borderColor: 'rgba(0, 161, 214, 0.3)',
    label: 'Bilibili',
    textColor: 'text-[#00A1D6]',
  },
  steam: {
    icon: <FaSteam />,
    color: '#1b2838',
    bgColor: 'rgba(27, 40, 56, 0.15)',
    borderColor: 'rgba(27, 40, 56, 0.3)',
    label: 'Steam',
    textColor: 'text-gray-700 dark:text-gray-300',
  },
  github: {
    icon: <FaGithub />,
    color: '#24292e',
    bgColor: 'rgba(36, 41, 46, 0.15)',
    borderColor: 'rgba(36, 41, 46, 0.3)',
    label: 'GitHub',
    textColor: 'text-gray-900 dark:text-gray-100',
  },
  netease: {
    icon: <SiNeteasecloudmusic />,
    color: '#e60026',
    bgColor: 'rgba(230, 0, 38, 0.15)',
    borderColor: 'rgba(230, 0, 38, 0.3)',
    label: 'NetEase',
    textColor: 'text-red-600',
  },
  bangumi: {
    icon: <SiBangumi />,
    color: '#f09199',
    bgColor: 'rgba(240, 145, 153, 0.15)',
    borderColor: 'rgba(240, 145, 153, 0.3)',
    label: 'Bangumi',
    textColor: 'text-rose-500',
  },
  mal: {
    icon: <SiMyanimelist />,
    color: '#2e51a2',
    bgColor: 'rgba(46, 81, 162, 0.15)',
    borderColor: 'rgba(46, 81, 162, 0.3)',
    label: 'MyAnimeList',
    textColor: 'text-blue-600 dark:text-blue-400',
  },
  x: {
    icon: <FaXTwitter />,
    color: '#000000',
    bgColor: 'rgba(0, 0, 0, 0.12)',
    borderColor: 'rgba(0, 0, 0, 0.25)',
    label: 'X',
    textColor: 'text-gray-900 dark:text-gray-100',
  },
  xbox: {
    icon: <FaXbox />,
    color: '#107C10',
    bgColor: 'rgba(16, 124, 16, 0.15)',
    borderColor: 'rgba(16, 124, 16, 0.3)',
    label: 'Xbox',
    textColor: 'text-[#107C10]',
  },
  psn: {
    icon: <SiPlaystation />,
    color: '#0070D1',
    bgColor: 'rgba(0, 112, 209, 0.15)',
    borderColor: 'rgba(0, 112, 209, 0.3)',
    label: 'PlayStation',
    textColor: 'text-[#0070D1]',
  },
  discord: {
    icon: <SiDiscord />,
    color: '#5865F2',
    bgColor: 'rgba(88, 101, 242, 0.15)',
    borderColor: 'rgba(88, 101, 242, 0.3)',
    label: 'Discord',
    textColor: 'text-[#5865F2]',
  },
}

// X 兴趣圈层构成条配色 — 固定顺序分配（圈层1→蓝 … 圈层4→粉），
// 亮/暗两套均通过 CVD 相邻区分与 3:1 对比度验证，勿随意增删或换序
const X_CIRCLE_COLOR_CLASSES = [
  'bg-[#2563eb] dark:bg-[#3b82f6]',
  'bg-[#d97706]',
  'bg-[#7c3aed] dark:bg-[#8b5cf6]',
  'bg-[#db2777] dark:bg-[#ec4899]',
]

const XWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const { t } = useI18n()
  const stats = data?.stats || {}
  // 说明：推文轮播已移除——X 卡聚焦关注图谱，top_posts 仅保留在数据层
  const followingSample = useMemo(
    () => (Array.isArray(data?.following_sample) ? data.following_sample : []),
    [data?.following_sample],
  )
  // 关注亮点：AI 点名的账号，从 following_sample 补齐头像/简介/粉丝数；
  // AI 未产出亮点时（旧报告），直接用关注样本前几位兜底
  const highlights = useMemo(() => {
    const list = Array.isArray(data?.following_highlights)
      ? data.following_highlights
      : []
    const sampleMap = new Map(
      followingSample.map((f: any) => [
        String(f.username || '').toLowerCase(),
        f,
      ]),
    )
    const enriched = list
      .filter((h: any) => h?.username || h?.name)
      .slice(0, 5)
      .map((h: any) => {
        const sample: any =
          sampleMap.get(String(h.username || '').toLowerCase()) || {}
        return {
          ...h,
          avatar: sample.avatar,
          description: sample.description,
          follower_count: sample.follower_count,
        }
      })
    if (enriched.length > 0) return enriched
    // 兜底（旧报告无 AI 亮点时）：样本按粉丝数降序，直接取头部会全是
    // NHK/连锁品牌这类无个性信号的大众官号——反向取有简介的小众账号
    return followingSample
      .filter((f: any) => String(f.description || '').trim())
      .sort(
        (a: any, b: any) =>
          (Number(a.follower_count) || 0) - (Number(b.follower_count) || 0),
      )
      .slice(0, 5)
      .map((f: any) => ({
        username: f.username,
        name: f.name,
        avatar: f.avatar,
        description: f.description,
        follower_count: f.follower_count,
      }))
  }, [data?.following_highlights, followingSample])

  // 兴趣圈层构成条：AI 从关注列表聚类，最多 4 段
  const circles = useMemo(() => {
    const list = Array.isArray(data?.interest_circles)
      ? data.interest_circles
      : []
    const cleaned = list
      .filter((c: any) => c?.name && Number(c?.count) > 0)
      .slice(0, X_CIRCLE_COLOR_CLASSES.length)
    const total = cleaned.reduce(
      (sum: number, c: any) => sum + Number(c.count),
      0,
    )
    if (total === 0) return []
    return cleaned.map((c: any, i: number) => {
      // AI 偶尔无视 ≤6 字约束，超长圈层名截断，保证图例不超两行
      const rawName = String(c.name)
      return {
        name: rawName.length > 7 ? `${rawName.slice(0, 6)}…` : rawName,
        count: Number(c.count),
        pct: (Number(c.count) / total) * 100,
        colorClass: X_CIRCLE_COLOR_CLASSES[i],
      }
    })
  }, [data?.interest_circles])

  // 概览态右侧头像墙素材（最多 7 个）：
  // 优先 AI 点名的品味账号（亮点 + 圈层代表），大众官号（粉丝数最大）不再天然霸榜
  const wallAvatars = useMemo(() => {
    const withAvatar = followingSample.filter((f: any) => f.avatar)
    const rank = new Map<string, number>()
    const addPreferred = (username: unknown) => {
      const key = String(username || '').toLowerCase()
      if (key && !rank.has(key)) rank.set(key, rank.size)
    }
    if (Array.isArray(data?.following_highlights)) {
      for (const h of data.following_highlights) addPreferred(h?.username)
    }
    if (Array.isArray(data?.interest_circles)) {
      for (const c of data.interest_circles) {
        if (Array.isArray(c?.accounts)) c.accounts.forEach(addPreferred)
      }
    }
    const keyOf = (f: any) => String(f.username || '').toLowerCase()
    const curated = withAvatar
      .filter((f: any) => rank.has(keyOf(f)))
      .sort((a: any, b: any) => rank.get(keyOf(a))! - rank.get(keyOf(b))!)
    const rest = withAvatar.filter((f: any) => !rank.has(keyOf(f)))
    return [...curated, ...rest].slice(0, 7)
  }, [followingSample, data?.following_highlights, data?.interest_circles])

  const [slideIndex, setSlideIndex] = useState(0)
  // 头像主色缓存（username → hex），用于详情面的氛围光
  const [tints, setTints] = useState<Record<string, string>>({})
  // 翻面只轮播关注亮点（标题由左下角 logo 药丸承载，卡片内不再放标题）
  const flipItems = highlights

  useEffect(() => {
    if (!showOverview && flipItems.length > 1) {
      const timer = setInterval(() => {
        setSlideIndex((i) => (i + 1) % flipItems.length)
      }, 4000)
      return () => clearInterval(timer)
    }
  }, [showOverview, flipItems.length])

  // 概览态药丸保持纯图标（与其他卡片一致）；详情态由药丸承载账号名/@username
  useEffect(() => {
    if (!showOverview && flipItems[slideIndex % flipItems.length]) {
      const item = flipItems[slideIndex % flipItems.length]
      const titles = [String(item.name || item.username || '')]
      if (item.username) titles.push(`@${item.username}`)
      onContentChange?.({ titles })
    } else {
      onContentChange?.(null)
    }
  }, [showOverview, slideIndex, flipItems, onContentChange])

  if (showOverview) {
    const profile = data?.profile || {}
    // 数字降级为一行小统计（重点是评价与画像）；数值与标签分层渲染
    const statsParts = (
      [
        [stats.following, t.reportCardWidget.xFollowing],
        [stats.followers, t.reportCardWidget.xFollowers],
        [stats.posts, t.reportCardWidget.xPosts],
      ] as [number | null, string][]
    ).filter(([value]) => value != null)
    return (
      <div className="relative h-full w-full overflow-hidden">
        <div className="absolute inset-0 bg-linear-to-br from-gray-200/50 to-transparent dark:from-white/[0.06] dark:to-transparent" />
        {/* 右侧背景：关注头像墙，向左渐隐 */}
        {wallAvatars.length > 0 && (
          <div
            className="absolute inset-y-0 right-0 w-[55%] opacity-80 dark:opacity-60"
            style={{
              maskImage:
                'linear-gradient(to left, rgba(0,0,0,1) 35%, transparent 88%)',
              WebkitMaskImage:
                'linear-gradient(to left, rgba(0,0,0,1) 35%, transparent 88%)',
            }}
          >
            <div className="absolute inset-y-0 left-0 right-0 flex items-start pt-[44px] justify-end pr-3 rotate-6">
              {wallAvatars.map((f: any, i: number) => (
                <motion.div
                  key={f.username || i}
                  className="w-9 h-9 shrink-0 -ml-2 rounded-full overflow-hidden shadow-md ring-2 ring-white/80 dark:ring-black/60"
                  style={{ y: i % 2 === 0 ? -8 : 10 }}
                  initial={{ x: 40, opacity: 0 }}
                  animate={{ x: 0, opacity: 1 }}
                  transition={{
                    duration: 0.45,
                    delay: 0.15 + i * 0.06,
                    ease: 'easeOut',
                  }}
                >
                  <img
                    src={f.avatar}
                    alt={f.name || f.username}
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
        <div className="relative z-10 h-full flex flex-col p-3 pb-9">
          {/* header：账号本人头像 + 用户名 */}
          {(profile.avatar || profile.name || profile.username) && (
            <motion.div
              className="flex items-center gap-2 min-w-0 max-w-[70%]"
              initial={{ y: 8, opacity: 0 }}
              animate={{ y: 0, opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.1 }}
            >
              {profile.avatar && (
                <img
                  src={profile.avatar}
                  alt={profile.name || profile.username}
                  className="w-8 h-8 rounded-full object-cover ring-2 ring-white/80 dark:ring-black/50 shadow-sm shrink-0"
                  loading="lazy"
                  referrerPolicy="no-referrer"
                />
              )}
              <div className="min-w-0">
                <div className="text-[11px] font-bold text-gray-900 dark:text-gray-100 leading-tight truncate">
                  {profile.name || profile.username}
                </div>
                {profile.username && (
                  <div className="text-[9px] font-mono text-gray-500 dark:text-gray-400 truncate">
                    @{profile.username}
                  </div>
                )}
              </div>
            </motion.div>
          )}
          {/* 主角：AI 评价，左侧垂直居中 */}
          {(data?.vibe || data?.engagement_level) && (
            <div className="flex-1 min-h-0 flex items-center pt-2 pb-4">
              <motion.div
                className="max-w-[68%]"
                initial={{ y: 8, opacity: 0 }}
                animate={{ y: 0, opacity: 1 }}
                transition={{ duration: 0.4, delay: 0.2 }}
              >
                <span className="block text-[17px] font-black text-gray-900 dark:text-gray-100 leading-snug line-clamp-2 text-balance">
                  {data?.vibe || data?.engagement_level}
                </span>
              </motion.div>
            </div>
          )}
          {/* 右上角：一行小统计（数值黑体大字，标签小字灰阶） */}
          {statsParts.length > 0 && (
            <motion.div
              className="absolute top-3 right-3 flex items-baseline gap-2"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.3 }}
            >
              {statsParts.map(([value, label]) => (
                <span key={label} className="flex items-baseline gap-0.5">
                  <span className="text-[10px] font-black tabular-nums text-gray-900 dark:text-gray-100">
                    {formatCompactNumber(value)}
                  </span>
                  <span className="text-[9px] font-medium text-gray-500 dark:text-gray-400">
                    {label}
                  </span>
                </span>
              ))}
            </motion.div>
          )}
          {/* 右下角：用户画像（圈层图例 + 构成条）；无圈层数据时退回话题词 */}
          {circles.length > 0 ? (
            <div className="absolute bottom-3 right-3 w-[48%] flex flex-col items-end gap-1">
              <div className="flex flex-wrap justify-end gap-x-2.5 gap-y-0.5 max-h-[26px] overflow-hidden">
                {circles.map((circle: any, i: number) => (
                  <motion.span
                    key={circle.name}
                    className="flex items-center gap-1 text-[8px] font-bold text-gray-600 dark:text-gray-300"
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    transition={{ duration: 0.3, delay: 0.35 + i * 0.12 }}
                  >
                    <span
                      className={`w-1.5 h-1.5 rounded-full ${circle.colorClass}`}
                    />
                    {circle.name}
                    <span className="font-mono text-gray-500 dark:text-gray-400">
                      {circle.count}
                    </span>
                  </motion.span>
                ))}
              </div>
              <div className="flex gap-[2px] h-1.5 w-full rounded-full overflow-hidden bg-gray-200/80 dark:bg-white/10 ring-1 ring-black/5 dark:ring-white/10">
                {circles.map((circle: any, i: number) => (
                  <motion.div
                    key={circle.name}
                    className={`h-full rounded-[2px] ${circle.colorClass}`}
                    initial={{ width: 0 }}
                    animate={{ width: `${circle.pct}%` }}
                    transition={{
                      duration: 0.35,
                      delay: 0.35 + i * 0.12,
                      ease: 'easeOut',
                    }}
                  />
                ))}
              </div>
            </div>
          ) : (
            Array.isArray(data?.signature_topics) &&
            data.signature_topics.length > 0 && (
              <div className="absolute bottom-3 right-3 max-w-[55%] flex flex-wrap justify-end gap-1">
                {data.signature_topics.slice(0, 4).map((topic: string) => (
                  <span
                    key={topic}
                    className="text-[9px] px-1.5 py-0.5 rounded-full bg-black/5 dark:bg-white/10 text-gray-700 dark:text-gray-300"
                  >
                    {topic}
                  </span>
                ))}
              </div>
            )
          )}
        </div>
      </div>
    )
  }

  const item =
    flipItems.length > 0 ? flipItems[slideIndex % flipItems.length] : null
  if (!item) {
    return (
      <div className="h-full w-full flex items-center justify-center text-gray-400 text-xs">
        <FaXTwitter />
      </div>
    )
  }

  // 关注亮点轮播：账号名/@username 由左下角 logo 药丸展示；
  // 标签（AI 评语）是主角，头像缩小为径向渐隐的背景图，主色氛围光衔接卡片背景
  const tint = tints[String(item.username || '')]
  return (
    <AnimatePresence mode="wait">
      <motion.div
        key={`hl-${slideIndex % flipItems.length}`}
        initial={CONTENT_FADE_INITIAL}
        animate={CONTENT_FADE_ANIMATE}
        exit={CONTENT_FADE_EXIT}
        transition={CONTENT_FADE_TRANSITION}
        className="relative h-full w-full overflow-hidden"
      >
        {/* 主色氛围层：取色算法从头像提主色，向卡片背景弥散 */}
        {tint && (
          <div
            className="absolute inset-0"
            style={{
              background: `radial-gradient(circle at 74% 50%, ${tint}30, transparent 75%)`,
            }}
          />
        )}
        {/* 头像：贴住右缘完整显示，向卡片内部径向渐隐（模糊半圆） */}
        {item.avatar && (
          <motion.div
            className="absolute inset-y-0 right-0 w-[58%]"
            style={{
              maskImage:
                'radial-gradient(circle at 100% 50%, rgba(0,0,0,1) 42%, transparent 74%)',
              WebkitMaskImage:
                'radial-gradient(circle at 100% 50%, rgba(0,0,0,1) 42%, transparent 74%)',
            }}
            initial={{ opacity: 0, x: 14 }}
            animate={{ opacity: 1, x: 0 }}
            transition={{ duration: 0.6, ease: 'easeOut' }}
          >
            <img
              src={item.avatar.replace(/_(normal|bigger)\./, '_400x400.')}
              alt={item.name || item.username}
              className="w-full h-full object-cover translate-x-[6%] scale-110"
              loading="lazy"
              referrerPolicy="no-referrer"
              onLoad={(e) => {
                const username = String(item.username || '')
                if (!username || tints[username]) return
                try {
                  const palette = extractColorsFromLoadedImage(e.currentTarget)
                  if (palette?.primary) {
                    setTints((prev) => ({
                      ...prev,
                      [username]: palette.primary,
                    }))
                  }
                } catch {
                  // 取色失败忽略，氛围层缺省即可
                }
              }}
            />
          </motion.div>
        )}
        {/* 前景：标签是主角，粉丝量降为统计行 */}
        <div className="relative z-10 h-full p-3 pb-12 flex flex-col">
          {item.tag && (
            <motion.div
              className="min-w-0 max-w-[70%]"
              initial={{ y: 8, opacity: 0 }}
              animate={{ y: 0, opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.1 }}
            >
              <span className="block text-base font-black text-gray-900 dark:text-gray-100 leading-tight truncate">
                {item.tag}
              </span>
            </motion.div>
          )}
          {item.follower_count != null && (
            <motion.div
              className="mt-0.5 text-[10px] font-medium text-gray-500 dark:text-gray-400"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.2 }}
            >
              {formatCompactNumber(item.follower_count)}{' '}
              {t.reportCardWidget.xFollowers}
            </motion.div>
          )}
          {/* 简介最多两行，收在 overflow-hidden 容器里，不侵入底部药丸区 */}
          {item.description && (
            <div className="mt-1.5 flex-1 min-h-0 overflow-hidden max-w-[58%]">
              <p className="text-[9px] leading-relaxed text-gray-600 dark:text-gray-400 line-clamp-2">
                {item.description}
              </p>
            </div>
          )}
        </div>
        {/* 轮播指示点 */}
        {flipItems.length > 1 && (
          <div className="absolute bottom-3 right-3 z-10 flex gap-1">
            {flipItems.map((_: any, i: number) => (
              <span
                key={i}
                className={`w-1 h-1 rounded-full transition-colors ${
                  i === slideIndex % flipItems.length
                    ? 'bg-gray-800 dark:bg-white/90'
                    : 'bg-gray-400/60 dark:bg-white/30'
                }`}
              />
            ))}
          </div>
        )}
      </motion.div>
    </AnimatePresence>
  )
})

function formatCompactNumber(n: number | undefined | null): string {
  const num = Number(n) || 0
  if (num >= 1_000_000) return `${(num / 1_000_000).toFixed(1)}M`
  if (num >= 1_000) return `${(num / 1_000).toFixed(1)}K`
  return String(num)
}

// ==================== Discord 社区身份卡 ====================
// 概览面：账号画像 + 服务器图标墙 + 社区触达 / 角色定位
// 详情面：代表服务器轮播（规模、角色、认证特性）
const DISCORD_BLURPLE = '#5865F2'

// 服务器无图标时的字母兜底底色（按名称 hash 取一组柔和 blurple 邻近色）
const DISCORD_TILE_COLORS = [
  '#5865F2',
  '#7289DA',
  '#4752C4',
  '#949CF7',
  '#3C45A5',
]
function discordTileColor(name: string): string {
  let hash = 0
  for (let i = 0; i < name.length; i++) {
    hash = (hash * 31 + name.charCodeAt(i)) & 0xFFFFFFFF
  }
  return DISCORD_TILE_COLORS[Math.abs(hash) % DISCORD_TILE_COLORS.length]
}

// 从服务器权限 / 特性推出一个角色徽章
function discordGuildBadge(
  g: any,
  t: any,
): { label: string; color: string } | null {
  const perms: string[] = Array.isArray(g?.permissions) ? g.permissions : []
  const features: string[] = Array.isArray(g?.features) ? g.features : []
  if (g?.owner) return { label: t.reportCardWidget.discordRoleOwner, color: '#F0B232' }
  if (perms.includes('ADMINISTRATOR'))
    return { label: t.reportCardWidget.discordRoleAdmin, color: '#5865F2' }
  if (perms.includes('MANAGE_GUILD'))
    return { label: t.reportCardWidget.discordRoleMod, color: '#3BA55D' }
  if (features.includes('PARTNERED'))
    return { label: t.reportCardWidget.discordFeaturePartner, color: '#5865F2' }
  if (features.includes('VERIFIED'))
    return { label: t.reportCardWidget.discordFeatureVerified, color: '#3BA55D' }
  if (features.includes('COMMUNITY'))
    return { label: t.reportCardWidget.discordFeatureCommunity, color: '#949CF7' }
  return null
}

function DiscordGuildIcon({
  icon,
  name,
  size,
}: {
  icon?: string | null
  name: string
  size: number
}) {
  const [failed, setFailed] = useState(false)
  if (icon && !failed) {
    return (
      <img
        src={icon}
        alt={name}
        className="w-full h-full object-cover"
        loading="lazy"
        referrerPolicy="no-referrer"
        onError={() => setFailed(true)}
      />
    )
  }
  // 兜底：取名称首字（含 emoji）铺底色
  const letter = Array.from(name.trim())[0] || '#'
  return (
    <div
      className="w-full h-full flex items-center justify-center font-bold text-white"
      style={{
        background: discordTileColor(name),
        fontSize: Math.round(size * 0.42),
      }}
    >
      {letter}
    </div>
  )
}

const DiscordWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const { t } = useI18n()
  const profile = data?.profile || {}
  const stats = data?.stats || {}
  const guilds = useMemo(
    () => (Array.isArray(data?.library_items) ? data.library_items : []),
    [data?.library_items],
  )
  const badges: string[] = useMemo(
    () => (Array.isArray(profile.badges) ? profile.badges.slice(0, 3) : []),
    [profile.badges],
  )
  // 社区标签：优先 AI 的 community_tags，缺席时退回绑定平台
  const tags: string[] = useMemo(() => {
    const ct = Array.isArray(data?.community_tags) ? data.community_tags : []
    if (ct.length > 0) return ct.slice(0, 4)
    const lp = Array.isArray(data?.linked_platforms) ? data.linked_platforms : []
    return lp.slice(0, 4)
  }, [data?.community_tags, data?.linked_platforms])

  // 图标墙素材（最多 7 个，后端已按 服主/管理/规模 排序）
  const iconWall = useMemo(() => guilds.slice(0, 7), [guilds])
  // 详情轮播只取前 8 个代表服务器
  const flipItems = useMemo(() => guilds.slice(0, 8), [guilds])

  const [slideIndex, setSlideIndex] = useState(0)
  useEffect(() => {
    if (!showOverview && flipItems.length > 1) {
      const timer = setInterval(() => {
        setSlideIndex((i) => (i + 1) % flipItems.length)
      }, 4000)
      return () => clearInterval(timer)
    }
  }, [showOverview, flipItems.length])

  // 头部第二行：@用户名与账号徽章共用一行，定时轮换（徽章太少不值得单占一行）
  const hasHandle = Boolean(profile.username || profile.nitro)
  const headerSlides = (hasHandle ? 1 : 0) + badges.length
  const [headerIdx, setHeaderIdx] = useState(0)
  useEffect(() => {
    if (showOverview && headerSlides > 1) {
      const timer = setInterval(() => {
        setHeaderIdx((i) => (i + 1) % headerSlides)
      }, 3200)
      return () => clearInterval(timer)
    }
  }, [showOverview, headerSlides])

  // 详情态：左下角药丸承载当前服务器名；概览态药丸保持纯图标
  useEffect(() => {
    if (!showOverview && flipItems[slideIndex % flipItems.length]) {
      const g = flipItems[slideIndex % flipItems.length]
      onContentChange?.({ title: String(g.name || g.title || '') })
    } else {
      onContentChange?.(null)
    }
  }, [showOverview, slideIndex, flipItems, onContentChange])

  const memberReach = Number(stats.member_reach) || 0

  if (showOverview) {
    const displayName =
      profile.display_name || profile.username || 'Discord'
    const statsParts = (
      [
        [Number(stats.guilds) || 0, t.reportCardWidget.discordGuilds],
        memberReach > 0
          ? [memberReach, t.reportCardWidget.discordReach]
          : null,
        [Number(stats.connections) || 0, t.reportCardWidget.discordConnections],
      ].filter(Boolean) as [number, string][]
    ).filter(([value]) => value != null)

    return (
      <div className="relative h-full w-full overflow-hidden">
        <div className="absolute inset-0 bg-linear-to-br from-[#5865F2]/10 to-transparent dark:from-[#5865F2]/[0.14] dark:to-transparent" />
        {/* 右侧背景：服务器图标墙，向左渐隐 */}
        {iconWall.length > 0 && (
          <div
            className="absolute inset-y-0 right-0 w-[55%] opacity-80 dark:opacity-70"
            style={{
              maskImage:
                'linear-gradient(to left, rgba(0,0,0,1) 35%, transparent 88%)',
              WebkitMaskImage:
                'linear-gradient(to left, rgba(0,0,0,1) 35%, transparent 88%)',
            }}
          >
            <div className="absolute inset-y-0 left-0 right-0 flex items-center justify-end pr-3 -rotate-6">
              {iconWall.map((g: any, i: number) => (
                <motion.div
                  key={g.id || i}
                  className="w-10 h-10 shrink-0 -ml-2 rounded-2xl overflow-hidden shadow-md ring-2 ring-white/80 dark:ring-black/60"
                  style={{ y: i % 2 === 0 ? -9 : 11 }}
                  initial={{ x: 40, opacity: 0 }}
                  animate={{ x: 0, opacity: 1 }}
                  transition={{
                    duration: 0.45,
                    delay: 0.15 + i * 0.06,
                    ease: 'easeOut',
                  }}
                >
                  <DiscordGuildIcon
                    icon={g.icon}
                    name={String(g.name || g.title || '?')}
                    size={40}
                  />
                </motion.div>
              ))}
            </div>
          </div>
        )}
        {/* 前景 */}
        <div className="relative z-10 h-full flex flex-col p-3 pb-9">
          {/* header：账号头像 + 名称 + 徽章 */}
          <motion.div
            className="flex items-center gap-2 min-w-0 max-w-[72%]"
            initial={{ y: 8, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            transition={{ duration: 0.4, delay: 0.1 }}
          >
            {profile.avatar_url ? (
              <img
                src={profile.avatar_url}
                alt={displayName}
                className="w-8 h-8 rounded-full object-cover ring-2 ring-white/80 dark:ring-black/50 shadow-sm shrink-0"
                loading="lazy"
                referrerPolicy="no-referrer"
              />
            ) : (
              <div
                className="w-8 h-8 rounded-full flex items-center justify-center text-white text-sm font-bold shrink-0 ring-2 ring-white/80 dark:ring-black/50"
                style={{ background: DISCORD_BLURPLE }}
              >
                {Array.from(String(displayName).trim())[0] || '#'}
              </div>
            )}
            <div className="min-w-0">
              <div className="text-[11px] font-bold text-gray-900 dark:text-gray-100 leading-tight truncate">
                {displayName}
              </div>
              {headerSlides > 0 && (
                <div className="relative h-[15px] overflow-hidden">
                  <AnimatePresence mode="wait" initial={false}>
                    <motion.div
                      key={headerIdx % headerSlides}
                      className="flex items-center gap-1 min-w-0"
                      initial={{ y: 8, opacity: 0 }}
                      animate={{ y: 0, opacity: 1 }}
                      exit={{ y: -8, opacity: 0 }}
                      transition={{ duration: 0.28, ease: 'easeOut' }}
                    >
                      {hasHandle && headerIdx % headerSlides === 0 ? (
                        <>
                          {profile.username && (
                            <span className="text-[9px] font-mono text-gray-500 dark:text-gray-400 truncate">
                              @{profile.username}
                            </span>
                          )}
                          {profile.nitro && (
                            <span className="text-[8px] px-1 rounded bg-[#5865F2]/15 text-[#5865F2] font-bold shrink-0">
                              {profile.nitro}
                            </span>
                          )}
                        </>
                      ) : (
                        <span className="text-[9px] font-medium text-[#5865F2] dark:text-[#949CF7] truncate">
                          {
                            badges[
                              (headerIdx % headerSlides) - (hasHandle ? 1 : 0)
                            ]
                          }
                        </span>
                      )}
                    </motion.div>
                  </AnimatePresence>
                </div>
              )}
            </div>
          </motion.div>

          {/* 主角：AI 社区人格 */}
          {(data?.vibe || data?.role_profile) && (
            <div className="flex-1 min-h-0 flex items-center pt-1.5 pb-4">
              <motion.div
                className="max-w-[68%]"
                initial={{ y: 8, opacity: 0 }}
                animate={{ y: 0, opacity: 1 }}
                transition={{ duration: 0.4, delay: 0.2 }}
              >
                <span className="block text-[16px] font-black text-gray-900 dark:text-gray-100 leading-snug line-clamp-2 text-balance">
                  {data?.vibe || data?.role_profile}
                </span>
              </motion.div>
            </div>
          )}

          {/* 右上角：一行小统计 */}
          {statsParts.length > 0 && (
            <motion.div
              className="absolute top-3 right-3 flex items-baseline gap-2"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.3 }}
            >
              {statsParts.map(([value, label]) => (
                <span key={label} className="flex items-baseline gap-0.5">
                  <span className="text-[10px] font-black tabular-nums text-gray-900 dark:text-gray-100">
                    {formatCompactNumber(value)}
                  </span>
                  <span className="text-[9px] font-medium text-gray-500 dark:text-gray-400">
                    {label}
                  </span>
                </span>
              ))}
            </motion.div>
          )}

          {/* 右下角：社区标签 */}
          {tags.length > 0 && (
            <div className="absolute bottom-3 right-3 max-w-[55%] flex flex-wrap justify-end gap-1">
              {tags.map((tag) => (
                <span
                  key={tag}
                  className="text-[9px] px-1.5 py-0.5 rounded-full bg-[#5865F2]/12 dark:bg-[#5865F2]/20 text-[#4752C4] dark:text-[#949CF7] font-medium"
                >
                  {tag}
                </span>
              ))}
            </div>
          )}
        </div>
      </div>
    )
  }

  // 详情面：代表服务器轮播
  const item =
    flipItems.length > 0 ? flipItems[slideIndex % flipItems.length] : null
  if (!item) {
    return (
      <div className="h-full w-full flex items-center justify-center text-gray-400 text-xl">
        <SiDiscord />
      </div>
    )
  }
  const badge = discordGuildBadge(item, t)
  const guildName = String(item.name || item.title || '')

  return (
    <AnimatePresence mode="wait">
      <motion.div
        key={`dg-${slideIndex % flipItems.length}`}
        initial={CONTENT_FADE_INITIAL}
        animate={CONTENT_FADE_ANIMATE}
        exit={CONTENT_FADE_EXIT}
        transition={CONTENT_FADE_TRANSITION}
        className="relative h-full w-full overflow-hidden"
      >
        {/* 服务器图标：右侧直接展示 */}
        <motion.div
          className="absolute inset-y-0 right-0 flex items-center pr-4"
          initial={{ opacity: 0, x: 14 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ duration: 0.6, ease: 'easeOut' }}
        >
          <div className="h-[52%] max-h-28 aspect-square rounded-3xl overflow-hidden shadow-xl">
            <DiscordGuildIcon icon={item.icon} name={guildName} size={96} />
          </div>
        </motion.div>
        {/* 前景：角色徽章 + 规模，整块垂直居中与右侧图标平衡 */}
        <div className="relative z-10 h-full p-3 pb-12 flex flex-col justify-center">
          {badge && (
            <motion.span
              className="self-start text-[10px] font-bold px-2 py-0.5 rounded-full text-white shadow-sm"
              style={{ background: badge.color }}
              initial={{ y: 8, opacity: 0 }}
              animate={{ y: 0, opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.1 }}
            >
              {badge.label}
            </motion.span>
          )}
          <motion.div
            className="mt-2 min-w-0 max-w-[62%]"
            initial={{ y: 8, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            transition={{ duration: 0.4, delay: 0.15 }}
          >
            <span className="block text-base font-black text-gray-900 dark:text-gray-100 leading-tight line-clamp-2">
              {guildName}
            </span>
          </motion.div>
          {Number(item.member_count) > 0 && (
            <motion.div
              className="mt-1 flex items-baseline gap-2 text-[10px] text-gray-500 dark:text-gray-400"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: 0.4, delay: 0.22 }}
            >
              <span className="font-bold text-gray-700 dark:text-gray-200 tabular-nums">
                {formatCompactNumber(Number(item.member_count))}
              </span>
              <span>{t.reportCardWidget.discordMembers}</span>
            </motion.div>
          )}
        </div>
        {/* 轮播指示点 */}
        {flipItems.length > 1 && (
          <div className="absolute bottom-3 right-3 z-10 flex gap-1">
            {flipItems.map((_: any, i: number) => (
              <span
                key={i}
                className={`w-1 h-1 rounded-full transition-colors ${
                  i === slideIndex % flipItems.length
                    ? 'bg-[#5865F2] dark:bg-[#949CF7]'
                    : 'bg-gray-400/60 dark:bg-white/30'
                }`}
              />
            ))}
          </div>
        )}
      </motion.div>
    </AnimatePresence>
  )
})

// Bangumi 类型构成条配色（动画/书/游戏/音乐/剧集）
const BANGUMI_TYPE_COLORS: Record<string, string> = {
  anime: '#fb7185',
  book: '#a78bfa',
  game: '#60a5fa',
  music: '#34d399',
  real: '#fbbf24',
}

// MAL 类型构成条配色（动画 / 漫画）
const MAL_TYPE_COLORS: Record<string, string> = {
  anime: '#2e51a2',
  manga: '#60a5fa',
}

const BangumiWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const { t } = useI18n()
  const libraryItems = useMemo(
    () => data?.library_items || [],
    [data?.library_items],
  )
  const statusCounts =
    data?.status_counts || data?.collection_type_distribution || {}
  const done = statusCounts.done || 0
  const doing = statusCounts.doing || 0
  const wish = statusCounts.wish || 0
  const subjectTypeLabels: Record<string, string> = {
    book: t.library.book,
    anime: t.library.anime,
    game: t.library.game,
    music: t.library.music,
    real: t.library.tvSeries,
  }
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
            <div className="absolute inset-0 bg-linear-to-br from-rose-50/50 to-transparent dark:from-rose-900/20 dark:to-transparent" />
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
                      className="w-14 shrink-0 aspect-[3/4] rounded-md overflow-hidden shadow-md ring-1 ring-black/10 dark:ring-white/10"
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
                className="w-fit max-w-[70%] px-2 py-0.5 rounded-md text-[9px] font-bold flex items-center gap-1 shadow-sm bg-rose-400/15 text-rose-500 border border-rose-400/25 backdrop-blur-sm"
                initial={{ scale: 0.8, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                transition={{ duration: 0.3, delay: 0.2 }}
              >
                <span className="text-[7px] shrink-0">●</span>
                <span className="truncate">
                  {data?.taste_profile || t.widgets.reportBangumi}
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
                      {t.reportsPage.bangumiDone}
                    </span>
                  </motion.div>
                  <div className="flex gap-3">
                    {[
                      [doing, t.reportsPage.bangumiDoing] as const,
                      [wish, t.reportsPage.bangumiWish] as const,
                    ].map(([count, label], i) => (
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
                              BANGUMI_TYPE_COLORS[segment.type] || '#f09199',
                          }}
                        />
                        {subjectTypeLabels[segment.type] || segment.type}
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
                            BANGUMI_TYPE_COLORS[segment.type] || '#f09199',
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
                <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
                  <div className="absolute inset-0">
                    {item.cover ? (
                      <img
                        src={item.cover}
                        alt={item.title}
                        className="w-full h-full object-cover"
                        loading="lazy"
                      />
                    ) : (
                      <div className="w-full h-full flex items-center justify-center text-4xl text-rose-400 bg-rose-50 dark:bg-rose-950/30">
                        <SiBangumi />
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
})

// MyAnimeList 报告卡 — 布局对齐 Bangumi（概览数字 + 类型条 + 详情双封面）
const MalWidget = memo(({ data, showOverview, onContentChange }: any) => {
  const { t } = useI18n()
  const libraryItems = useMemo(
    () => data?.library_items || [],
    [data?.library_items],
  )
  const statusCounts =
    data?.status_counts || data?.collection_type_distribution || {}
  const done = statusCounts.done || 0
  const doing = statusCounts.doing || 0
  const wish = statusCounts.wish || 0
  const subjectTypeLabels: Record<string, string> = {
    anime: t.library.anime,
    manga: t.library.book,
  }
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
  const wallCovers = useMemo(
    () => libraryItems.filter((item: any) => item.cover).slice(0, 5),
    [libraryItems],
  )
  const [currentIndex, setCurrentIndex] = useState(0)
  const prevShowOverviewRef = useRef(showOverview)

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
            <div className="absolute inset-0 bg-linear-to-br from-blue-50/50 to-transparent dark:from-blue-900/20 dark:to-transparent" />
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
                      className="w-14 shrink-0 aspect-[3/4] rounded-md overflow-hidden shadow-md ring-1 ring-black/10 dark:ring-white/10"
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
                      />
                    </motion.div>
                  ))}
                </div>
              </div>
            )}
            <div className="relative z-10 h-full flex flex-col p-2.5">
              <motion.div
                className="w-fit max-w-[70%] px-2 py-0.5 rounded-md text-[9px] font-bold flex items-center gap-1 shadow-sm bg-blue-500/15 text-blue-600 dark:text-blue-400 border border-blue-500/25 backdrop-blur-sm"
                initial={{ scale: 0.8, opacity: 0 }}
                animate={{ scale: 1, opacity: 1 }}
                transition={{ duration: 0.3, delay: 0.2 }}
              >
                <span className="text-[7px] shrink-0">●</span>
                <span className="truncate">
                  {data?.taste_profile || t.widgets.reportMal}
                </span>
              </motion.div>
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
                      {t.reportsPage.malDone}
                    </span>
                  </motion.div>
                  <div className="flex gap-3">
                    {[
                      [doing, t.reportsPage.malDoing] as const,
                      [wish, t.reportsPage.malWish] as const,
                    ].map(([count, label], i) => (
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
                              MAL_TYPE_COLORS[segment.type] || '#2e51a2',
                          }}
                        />
                        {subjectTypeLabels[segment.type] || segment.type}
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
                            MAL_TYPE_COLORS[segment.type] || '#2e51a2',
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
          <div className="h-full w-full flex gap-1.5">
            {currentItems.map((item: any, idx: number) => (
              <div key={idx} className="flex-1 h-full">
                <div className="relative h-full w-full rounded-xl overflow-hidden shadow-lg bg-white dark:bg-black/90">
                  <div className="absolute inset-0">
                    {item.cover ? (
                      <img
                        src={item.cover}
                        alt={item.title}
                        className="w-full h-full object-cover"
                        loading="lazy"
                      />
                    ) : (
                      <div className="w-full h-full flex items-center justify-center text-4xl text-blue-400 bg-blue-50 dark:bg-blue-950/30">
                        <SiMyanimelist />
                      </div>
                    )}
                  </div>
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
})

// ==================== 主组件 ====================
export const ReportCardWidget = memo(
  ({
    config,
    isEditMode,
    isPreview,
    data: externalData,
    bare = false,
    showOverview: controlledShowOverview,
    onConfigChange,
  }: ReportCardWidgetProps) => {
    const animLevel = useAnimationLevel()
    const { t } = useI18n()
    const navigate = useNavigate()
    const localRef = useRef<HTMLDivElement | null>(null)
    const platformId = resolveReportPlatformId(config)
    const [reportData, setReportData] = useState<any>(null)
    const [loading, setLoading] = useState(true)
    const isOverviewControlled = controlledShowOverview !== undefined
    const [internalShowOverview, setInternalShowOverview] = useState(true)
    const showOverview = isOverviewControlled
      ? controlledShowOverview
      : internalShowOverview
    const [cardContent, setCardContent] = useState<{
      title: string
      type?: string
      titles?: string[]
    } | null>(null)

    useEffect(() => {
      if (isPreview) {
        // SVG data URI：预览态免外网依赖，头像墙 / 海报墙 / 详情面都能亮起来
        const previewAvatar = (letter: string, bg: string) => {
          const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect width="96" height="96" rx="48" fill="${bg}"/><text x="48" y="58" text-anchor="middle" fill="#fff" font-size="36" font-family="system-ui,sans-serif" font-weight="700">${letter}</text></svg>`
          return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
        }
        const previewCover = (letter: string, bg: string, w = 160, h = 200) => {
          const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0%" stop-color="${bg}"/><stop offset="100%" stop-color="#111827"/></linearGradient></defs><rect width="${w}" height="${h}" fill="url(#g)"/><text x="${w / 2}" y="${h / 2 + 12}" text-anchor="middle" fill="#fff" font-size="42" font-family="system-ui,sans-serif" font-weight="700" opacity="0.92">${letter}</text></svg>`
          return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
        }

        // Discord 卡片字段结构与其他平台差异较大（profile/stats/library_items
        // 形状不同），单独给一份预览数据，避免与通用预览字段互相污染。
        if (platformId === 'discord') {
          const guildTile = (letter: string, bg: string) => {
            const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96"><rect width="96" height="96" rx="24" fill="${bg}"/><text x="48" y="62" text-anchor="middle" fill="#fff" font-size="44" font-family="system-ui,sans-serif" font-weight="700">${letter}</text></svg>`
            return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
          }
          setReportData({
            vibe: t.reportCardWidget.discordVibeDefault,
            role_profile: t.reportCardWidget.discordRoleDefault,
            community_tags: [
              t.reportCardWidget.discordTagOpenSource,
              t.reportCardWidget.discordTagIndieGame,
              t.reportCardWidget.discordTagAcg,
            ],
            profile: {
              display_name: 'PreviewUser',
              username: 'preview',
              avatar_url: previewAvatar('D', '#5865F2'),
              nitro: 'Nitro',
              account_age_years: 7,
              badges: [
                'Active Developer',
                'HypeSquad Balance',
                'Early Supporter',
              ],
              mfa_enabled: true,
            },
            stats: {
              guilds: 42,
              owned_guilds: 2,
              admin_guilds: 5,
              manage_guilds: 8,
              connections: 4,
              member_reach: 128000,
              online_reach: 21000,
            },
            linked_platforms: ['github', 'steam', 'spotify', 'youtube'],
            connections: [
              { type: 'github', name: 'octocat', verified: true },
              { type: 'steam', name: 'PreviewGamer', verified: true },
              { type: 'spotify', name: 'preview', verified: false },
            ],
            library_items: [
              {
                id: 'g1',
                name: 'Open Source Guild',
                title: 'Open Source Guild',
                icon: guildTile('O', '#5865F2'),
                owner: true,
                permissions: ['ADMINISTRATOR'],
                member_count: 8200,
                presence_count: 1400,
                features: ['COMMUNITY'],
              },
              {
                id: 'g2',
                name: 'Indie Devs',
                title: 'Indie Devs',
                icon: guildTile('I', '#4752C4'),
                owner: false,
                permissions: ['MANAGE_GUILD'],
                member_count: 25000,
                presence_count: 3800,
                features: ['PARTNERED'],
              },
              {
                id: 'g3',
                name: 'ACG Lounge',
                title: 'ACG Lounge',
                icon: guildTile('A', '#7289DA'),
                owner: false,
                permissions: [],
                member_count: 61000,
                presence_count: 9200,
                features: ['VERIFIED'],
              },
              {
                id: 'g4',
                name: 'Pixel Art',
                title: 'Pixel Art',
                icon: guildTile('P', '#949CF7'),
                owner: false,
                permissions: [],
                member_count: 4300,
                presence_count: 700,
                features: [],
              },
              {
                id: 'g5',
                name: 'Rust Nomads',
                title: 'Rust Nomads',
                icon: guildTile('R', '#3C45A5'),
                owner: false,
                permissions: [],
                member_count: 12000,
                presence_count: 1900,
                features: ['COMMUNITY'],
              },
            ],
          })
          setLoading(false)
          return
        }

        const sampleGame = t.reportCardWidget.sampleGame
        const sampleAnime = t.reportCardWidget.sampleAnime
        const samplePlaylist = t.reportCardWidget.samplePlaylist
        const sampleProject = t.reportCardWidget.sampleProject
        const sampleManga = t.reportCardWidget.sampleManga
        const sampleGame2 = t.reportCardWidget.sampleGame2
        const sampleAnime2 = t.reportCardWidget.sampleAnime2

        setReportData({
          // —— 通用评分 / 身份标签 ——
          hardcore_score: 85,
          player_type: t.reportCardWidget.hardcorePlayer,
          gamer_type: t.reportCardWidget.xboxGamerDefault,
          hunter_type: t.reportCardWidget.psnHunterDefault,
          contribution_level: t.reportCardWidget.seniorDev,
          taste_profile: t.reportCardWidget.bangumiTasteDefault,

          // —— Steam ——
          games_count: 120,
          total_playtime: 2500,
          personaname: 'PreviewGamer',
          personastate_label: 'online',
          is_online: true,
          is_in_game: false,
          avatar: previewAvatar('S', '#1b2838'),
          recent_2weeks_minutes: 840,

          // —— Xbox ——
          gamertag: 'PreviewGamer',
          gamerscore: 12500,
          total_achievements: 340,
          completion_rate: 42,
          completed_games: 8,

          // —— PSN ——
          online_id: 'PreviewPSN',
          trophy_level: 245,
          platinum_count: 18,
          total_trophies: 1260,

          // —— GitHub ——
          total_contributions: 1200,
          repos_count: 45,
          total_stars: 890,
          languages: [
            { name: 'TypeScript', percentage: 42 },
            { name: 'Rust', percentage: 28 },
            { name: 'Python', percentage: 18 },
            { name: 'Go', percentage: 12 },
          ],

          // —— 网易云 ——
          follower_count: 1200,
          playlist_count: 15,
          level: 8,
          mood_keywords: [
            t.reportCardWidget.happyMood,
            t.reportCardWidget.sadMood,
            t.reportCardWidget.passionateMood,
            t.reportCardWidget.calmMood,
            t.reportCardWidget.nightMood,
          ],

          // —— Bilibili 弹幕（无则组件有默认） ——
          danmaku: t.reportCard.danmakuDefault as unknown as string[],

          // —— Bangumi / MAL 收藏结构 ——
          status_counts: { done: 128, doing: 12, wish: 45 },
          subject_type_distribution: {
            anime: 80,
            book: 28,
            manga: 28,
            game: 22,
            music: 12,
            real: 8,
          },

          // —— 详情轮播 / 海报墙（多平台共用，字段取并集） ——
          library_items: [
            {
              title: sampleProject,
              type: 'repo',
              language: 'TypeScript',
              stars: 120,
              forks: 30,
              description: t.reportCardWidget.sampleProjectDesc,
            },
            {
              title: sampleGame,
              type: 'game',
              cover: previewCover('G', '#1b2838', 320, 150),
              progress: 100,
              achievements_earned: 48,
              achievements_total: 48,
              gamerscore: 1000,
              platinum: true,
            },
            {
              title: sampleGame2,
              type: 'game',
              cover: previewCover('H', '#107C10', 320, 150),
              progress: 72,
              achievements_earned: 36,
              achievements_total: 50,
              gamerscore: 640,
              platinum: false,
            },
            {
              title: sampleAnime,
              type: 'anime',
              cover: previewCover('A', '#f09199'),
              rate: 9,
            },
            {
              title: sampleAnime2,
              type: 'anime',
              cover: previewCover('B', '#2e51a2'),
              rate: 8,
            },
            {
              title: sampleManga,
              type: 'book',
              cover: previewCover('M', '#e11d48'),
              rate: 10,
            },
            {
              title: samplePlaylist,
              type: 'music',
              cover: previewCover('♪', '#e60026', 200, 200),
            },
            {
              title: t.reportCardWidget.samplePlaylist2,
              type: 'music',
              cover: previewCover('♫', '#7B68EE', 200, 200),
            },
          ],
          // Xbox / PSN 无 library 封面时的回退列表
          top_titles: [
            {
              name: sampleGame,
              title: sampleGame,
              progress: 100,
              platinum: true,
              cover: previewCover('G', '#1b2838', 320, 150),
              achievements_earned: 48,
              achievements_total: 48,
              gamerscore: 1000,
            },
            {
              name: sampleGame2,
              title: sampleGame2,
              progress: 72,
              platinum: false,
              cover: previewCover('H', '#107C10', 320, 150),
              achievements_earned: 36,
              achievements_total: 50,
              gamerscore: 640,
            },
            {
              name: t.reportCardWidget.sampleGame3,
              title: t.reportCardWidget.sampleGame3,
              progress: 45,
              platinum: false,
              cover: previewCover('J', '#0070D1', 320, 150),
              achievements_earned: 18,
              achievements_total: 40,
              gamerscore: 280,
            },
          ],

          // —— X 关注图谱 + 人设 ——
          vibe: t.reportCardWidget.xVibeDefault,
          engagement_level: t.reportCardWidget.xEngagementDefault,
          signature_topics: [
            t.reportCardWidget.xCircleIndie,
            t.reportCardWidget.xCircleOpenSource,
            t.reportCardWidget.xCircleArt,
          ],
          profile: {
            username: 'preview',
            name: 'Preview',
            avatar: previewAvatar('P', '#111827'),
          },
          stats: {
            followers: 1280,
            following: 420,
            posts: 86,
          },
          interest_circles: [
            {
              name: t.reportCardWidget.xCircleIndie,
              count: 22,
              accounts: ['pixelcraft', 'roguelike'],
            },
            {
              name: t.reportCardWidget.xCircleOpenSource,
              count: 15,
              accounts: ['octocat_lab'],
            },
            {
              name: t.reportCardWidget.xCircleArt,
              count: 9,
              accounts: ['inkwave'],
            },
          ],
          following_highlights: [
            {
              username: 'pixelcraft',
              name: 'PixelCraft',
              tag: t.reportCardWidget.xTagIndie,
            },
            {
              username: 'octocat_lab',
              name: 'Octocat Lab',
              tag: t.reportCardWidget.xTagTech,
            },
            {
              username: 'inkwave',
              name: 'Ink Wave',
              tag: t.reportCardWidget.xTagArt,
            },
          ],
          following_sample: [
            {
              username: 'pixelcraft',
              name: 'PixelCraft',
              description: t.reportCardWidget.xPreviewDescIndie,
              follower_count: 18200,
              avatar: previewAvatar('P', '#2563eb'),
            },
            {
              username: 'octocat_lab',
              name: 'Octocat Lab',
              description: t.reportCardWidget.xPreviewDescTech,
              follower_count: 9400,
              avatar: previewAvatar('O', '#7c3aed'),
            },
            {
              username: 'inkwave',
              name: 'Ink Wave',
              description: t.reportCardWidget.xPreviewDescArt,
              follower_count: 5600,
              avatar: previewAvatar('I', '#db2777'),
            },
            {
              username: 'roguelike',
              name: 'RogueLike',
              description: t.reportCardWidget.xPreviewDescIndie,
              follower_count: 3100,
              avatar: previewAvatar('R', '#d97706'),
            },
            {
              username: 'synthwave',
              name: 'SynthWave',
              description: t.reportCardWidget.xPreviewDescArt,
              follower_count: 2200,
              avatar: previewAvatar('S', '#0891b2'),
            },
            {
              username: 'typecraft',
              name: 'TypeCraft',
              description: t.reportCardWidget.xPreviewDescTech,
              follower_count: 4800,
              avatar: previewAvatar('T', '#059669'),
            },
            {
              username: 'loomstudio',
              name: 'Loom Studio',
              description: t.reportCardWidget.xPreviewDescArt,
              follower_count: 1700,
              avatar: previewAvatar('L', '#e11d48'),
            },
          ],
        })
        setLoading(false)
        return
      }

      // 外部直接提供数据（报告页复用）：不再自行请求，跟随 prop 更新
      if (externalData !== undefined) {
        // Accept raw card_visuals or a full platform report envelope
        const visuals =
          extractCardVisuals(externalData) ??
          (externalData &&
          typeof externalData === 'object' &&
          !Array.isArray(externalData)
            ? (externalData as Record<string, unknown>)
            : null)
        setReportData(
          hasRenderableCardVisuals(visuals) ? visuals : null,
        )
        setLoading(false)
        return
      }

      const fetchReport = async () => {
        try {
          // 使用去重机制避免多个 ReportCardWidget 同时请求
          const data = await getLatestReportDeduped()
          if (!data?.success && !Array.isArray(data?.platform_reports)) {
            setReportData(null)
            return
          }
          const report = findPlatformReport(data?.platform_reports, platformId)
          const visuals = extractCardVisuals(report)
          // Only accept non-empty visuals so owner home shows real stats,
          // not a blank shell after a field-mapping miss.
          setReportData(hasRenderableCardVisuals(visuals) ? visuals : null)
        } catch (err) {
          console.error(`${t.reportCardWidget.fetchReportFailed}:`, err)
          setReportData(null)
        } finally {
          setLoading(false)
        }
      }
      fetchReport()

      // 5分钟刷新一次 - timeout 链 + 可见性暂停
      let cancelled = false
      let timeoutId: number | null = null
      const schedule = () => {
        if (cancelled || document.hidden) return
        fetchReport()
        timeoutId = window.setTimeout(schedule, 5 * 60 * 1000)
      }
      timeoutId = window.setTimeout(schedule, 5 * 60 * 1000)

      const onVisibility = () => {
        if (document.hidden && timeoutId) {
          clearTimeout(timeoutId)
          timeoutId = null
        } else if (!document.hidden && !cancelled && !timeoutId) {
          schedule()
        }
      }
      document.addEventListener('visibilitychange', onVisibility)

      return () => {
        cancelled = true
        if (timeoutId) clearTimeout(timeoutId)
        document.removeEventListener('visibilitychange', onVisibility)
      }
    }, [platformId, isPreview, externalData])

    useEffect(() => {
      // 预览态 / 外部控制概览态时不启用内部自动轮播
      if (isPreview || isOverviewControlled) return

      // 10秒切换概览/详情 - timeout 链 + 可见性暂停
      let cancelled = false
      let timeoutId: number | null = null
      const tick = () => {
        if (cancelled || document.hidden) return
        setInternalShowOverview((prev) => !prev)
        timeoutId = window.setTimeout(tick, 10000)
      }
      timeoutId = window.setTimeout(tick, 10000)

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
    }, [isPreview, isOverviewControlled])

    const handleContentChange = useCallback((content: any) => {
      setCardContent(content)
    }, [])

    // ===== 长按点击行为设置（参考社交组件：编辑模式下按住 500ms 打开设置）=====
    // 仅作为仪表盘小组件时启用（报告页 bare / 预览态不干预）
    const interactive = !bare && !isPreview
    const clickAction: ReportCardClickAction =
      config.config?.clickAction === 'social' ? 'social' : 'report'

    const [socialUserId, setSocialUserId] = useState<string | undefined>(
      undefined,
    )
    useEffect(() => {
      if (!interactive || clickAction !== 'social') return
      let alive = true
      fetchPlatformUserIds().then((m) => {
        if (alive) setSocialUserId(m[platformId])
      })
      return () => {
        alive = false
      }
    }, [interactive, clickAction, platformId])

    const isLongPressRef = useRef(false)
    const longPressTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

    const applyClickAction = useCallback(
      (action: ReportCardClickAction) => {
        const nextConfig = { ...config.config, platformId, clickAction: action }
        if (typeof onConfigChange === 'function') {
          onConfigChange(nextConfig)
        } else {
          window.dispatchEvent(
            new CustomEvent('widget-config-update', {
              detail: {
                widgetId: config.id,
                config: nextConfig,
              },
            }),
          )
        }
        isLongPressRef.current = false
      },
      [config.id, config.config, onConfigChange, platformId],
    )

    const openSettings = useCallback(() => {
      if (!localRef.current) return
      openReportCardSettingsModal(
        clickAction,
        localRef.current.getBoundingClientRect(),
        applyClickAction,
        () => {
          isLongPressRef.current = false
        },
      )
    }, [applyClickAction, clickAction])

    const handlePressStart = useCallback(() => {
      if (!interactive || !isEditMode) return
      if (longPressTimerRef.current) {
        clearTimeout(longPressTimerRef.current)
      }
      isLongPressRef.current = false
      longPressTimerRef.current = setTimeout(() => {
        longPressTimerRef.current = null
        isLongPressRef.current = true
        openSettings()
      }, 500)
    }, [interactive, isEditMode, openSettings])

    const handlePressEnd = useCallback(() => {
      if (longPressTimerRef.current) {
        clearTimeout(longPressTimerRef.current)
        longPressTimerRef.current = null
      }
    }, [])

    useEffect(() => {
      return () => {
        if (longPressTimerRef.current) {
          clearTimeout(longPressTimerRef.current)
        }
        isLongPressRef.current = false
      }
    }, [])

    const handleCardClick = useCallback(() => {
      // 长按触发的设置不当作点击
      if (isLongPressRef.current) {
        isLongPressRef.current = false
        return
      }
      if (!interactive || isEditMode) return
      if (clickAction === 'social' && socialUserId) {
        window.open(
          PLATFORM_SOCIAL[platformId]?.getUserUrl(socialUserId) || '#',
          '_blank',
          'noopener,noreferrer',
        )
        return
      }
      // report 模式，或社交模式下未配置用户ID的兜底
      navigate('/reports')
    }, [
      interactive,
      isEditMode,
      clickAction,
      socialUserId,
      platformId,
      navigate,
    ])

    const handleMouseLeave = useCallback(() => {
      handlePressEnd()
    }, [handlePressEnd])

    if (loading) {
      return (
        <div className="h-full w-full flex items-center justify-center">
          <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500" />
        </div>
      )
    }
    if (!reportData) {
      return (
        <div className="h-full w-full flex items-center justify-center text-gray-400 text-sm">
          <span>{t.reportCard.noReportData}</span>
        </div>
      )
    }

    const platformConfig =
      PLATFORM_CONFIG[platformId] || PLATFORM_CONFIG.bilibili

    return (
      <WidgetShell
        containerRef={localRef}
        padding={0}
        glass={!bare}
        className={interactive && !isEditMode ? 'cursor-pointer' : ''}
        rootProps={{
          onClick: interactive ? handleCardClick : undefined,
          onMouseDown: interactive ? handlePressStart : undefined,
          onMouseUp: interactive ? handlePressEnd : undefined,
          onMouseLeave: interactive ? handleMouseLeave : undefined,
          onTouchStart: interactive ? handlePressStart : undefined,
          onTouchEnd: interactive ? handlePressEnd : undefined,
          onTouchCancel: interactive ? handlePressEnd : undefined,
        }}
        background={
          /* 动态背景光效（bare 模式下由外层容器负责，避免重复叠加） */
          !bare && (
            <GlowBackground
              color={platformConfig.color}
              animLevel={animLevel.level}
              shouldAnimate={animLevel.loop}
              variant="single"
              size="lg"
            />
          )
        }
      >
        {/* 主内容区 */}
        <div className="absolute inset-0 flex flex-col z-10">
          {platformId === 'bilibili' && (
            <BilibiliWidget
              data={reportData}
              showOverview={showOverview}
              onContentChange={handleContentChange}
              allowLoop={animLevel.loop}
            />
          )}
          {platformId === 'steam' && (
            <SteamWidget
              data={reportData}
              showOverview={showOverview}
              onContentChange={handleContentChange}
            />
          )}
          {platformId === 'github' && (
            <GithubWidget
              data={reportData}
              showOverview={showOverview}
              onContentChange={handleContentChange}
            />
          )}
          {platformId === 'netease' && (
            <NeteaseWidget
              data={reportData}
              showOverview={showOverview}
              onContentChange={handleContentChange}
              allowLoop={animLevel.loop}
            />
          )}
          {platformId === 'bangumi' && (
            <BangumiWidget
              data={reportData}
              showOverview={showOverview}
              onContentChange={handleContentChange}
            />
          )}
          {platformId === 'mal' && (
            <MalWidget
              data={reportData}
              showOverview={showOverview}
              onContentChange={handleContentChange}
            />
          )}
          {platformId === 'xbox' && (
            <XboxWidget
              data={reportData}
              showOverview={showOverview}
              onContentChange={handleContentChange}
            />
          )}
          {platformId === 'psn' && (
            <PsnWidget
              data={reportData}
              showOverview={showOverview}
              onContentChange={handleContentChange}
            />
          )}
          {platformId === 'x' && (
            <XWidget
              data={reportData}
              showOverview={showOverview}
              onContentChange={handleContentChange}
            />
          )}
          {platformId === 'discord' && (
            <DiscordWidget
              data={reportData}
              showOverview={showOverview}
              onContentChange={handleContentChange}
            />
          )}
        </div>

        {/* 左下角浮动Logo */}
        <motion.div
          className="absolute bottom-3 left-3 z-20"
          initial={false}
          animate={{ width: cardContent ? 'auto' : '32px' }}
          transition={{ duration: 0.3, ease: 'easeOut' }}
        >
          <div
            className={`rounded-lg flex items-center gap-2 ${platformConfig.textColor} backdrop-blur-sm shadow-lg transition-all overflow-hidden ${
              cardContent ? 'bg-white/95 dark:bg-black/95' : ''
            }`}
            style={{
              background: cardContent ? undefined : platformConfig.bgColor,
              border: `1px solid ${platformConfig.borderColor}`,
              padding: cardContent?.titles ? '4px 8px' : '0 8px',
              height: cardContent?.titles ? 'auto' : '32px',
            }}
          >
            <div className={`text-base shrink-0 ${platformConfig.textColor}`}>
              {platformConfig.icon}
            </div>
            <AnimatePresence>
              {cardContent && (
                <motion.div
                  initial={{ opacity: 0, width: 0 }}
                  animate={{ opacity: 1, width: 'auto' }}
                  exit={{ opacity: 0, width: 0 }}
                  transition={{ duration: 0.3 }}
                  className="flex items-center gap-2 whitespace-nowrap overflow-hidden"
                >
                  {cardContent.titles ? (
                    <div className="flex flex-col gap-0.5">
                      {cardContent.titles.map((title: string, idx: number) => (
                        <div
                          key={idx}
                          className="text-[10px] font-bold text-gray-900 dark:text-gray-100 max-w-30 truncate leading-tight"
                        >
                          {title}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <span className="text-[11px] font-bold text-gray-900 dark:text-gray-100 max-w-30 truncate">
                      {cardContent.title}
                    </span>
                  )}
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        </motion.div>

        {/* 长按设置提示（编辑模式）- 与社交网络小组件保持一致 */}
        {interactive && isEditMode && (
          <motion.div
            className="absolute top-1.5 right-1.5 z-30 w-5 h-5 rounded-md flex items-center justify-center bg-black/15 dark:bg-white/15 backdrop-blur-sm pointer-events-none"
            initial={{ opacity: 0, scale: 0.8 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={{ type: 'spring', stiffness: 400, damping: 20 }}
            title={t.platformCard.longPressHint}
          >
            <svg
              className="w-3 h-3 text-gray-700 dark:text-gray-200"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"
              />
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"
              />
            </svg>
          </motion.div>
        )}
      </WidgetShell>
    )
  },
)

ReportCardWidget.displayName = 'ReportCardWidget'

export { ReportCardSettingsModal }
