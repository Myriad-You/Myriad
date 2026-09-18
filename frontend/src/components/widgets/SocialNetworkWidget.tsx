import type { WidgetComponentProps } from '../widgetGridTypes'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'
import {
  FaGithub,
  FaSteam,
  FaXTwitter,
  SiBangumi,
  SiBilibili,
  SiMyanimelist,
  SiNeteasecloudmusic,
  SiYoutube,
} from '@lib/platformBrandIcons'

import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { createPortal } from 'react-dom'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { useLoopAnimation } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { currentCopy } from '../../i18n/localeCopy'
import {
  namedIconVersion,
  peekNamedIcon,
  requestNamedIcon,
  subscribeNamedIcons,
} from '../../lib/namedIconCatalog'
import { armWidgetSettingsHost } from '../../lib/widgetSettingsHost'
import { getCSRFToken } from '../../utils/csrf'
import {
  clearDedupCache,
  getPublicConfigDeduped,
  getUIConfigDeduped,
} from '../../utils/requestDedup'
import { useThemeMode } from '../../utils/themeSubscriber'
import { showError } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import { Spinner } from '../Spinner'
import { parseCustomPlatforms } from './parseCustomPlatforms'
import { GlowBackground } from './shared/GlowBackground'
import { WidgetLongPressHint } from './shared/WidgetLongPressHint'
import { WidgetSettingsSection, WidgetSettingsTip } from './shared/WidgetSettingsTip'
import { WidgetShell } from './shared/WidgetShell'

// 内联 SVG，避免 react-icons 全量导入。

const HTML_TAG_REGEX = /<[^>]*>/g
const DANGEROUS_CHARS_REGEX = /[<>"'`\\]/g
const DANGEROUS_CHARS_WITH_AMP_REGEX = /[<>"'`\\&]/g
const VALID_PROTOCOLS_REGEX = /^(https?:\/\/|mailto:)/i
const DANGEROUS_PROTOCOLS_REGEX = /^(javascript:|data:|vbscript:|file:)/i
const SUSPICIOUS_CHARS_REGEX = /[<>"'`\\]/

function escapeHtml(text: string): string {
  const div = document.createElement('div')
  div.textContent = text
  return div.innerHTML
}

function isValidUrlPattern(pattern: string): boolean {
  if (!pattern) return true // 空 URL 允许。

  // 只允许 http(s)/mailto。
  if (!VALID_PROTOCOLS_REGEX.test(pattern)) {
    return false
  }

  if (DANGEROUS_PROTOCOLS_REGEX.test(pattern)) {
    return false
  }

  if (SUSPICIOUS_CHARS_REGEX.test(pattern.replaceAll('{username}', ''))) {
    return false
  }

  return true
}

function sanitizePlatformName(name: string): string {
  return name
    .replaceAll(HTML_TAG_REGEX, '')
    .replaceAll(DANGEROUS_CHARS_REGEX, '')
    .trim()
    .slice(0, 50)
}

function sanitizeUsername(username: string): string {
  return username
    .replaceAll(DANGEROUS_CHARS_WITH_AMP_REGEX, '')
    .trim()
    .slice(0, 100)
}

function sanitizePopupText(text: string): string {
  return escapeHtml(text.trim().slice(0, 500))
}

function sanitizeUrlPattern(pattern: string): string {
  if (!pattern) return ''

  let cleaned = pattern.trim()

  if (cleaned && !VALID_PROTOCOLS_REGEX.test(cleaned)) {
    cleaned = `https://${cleaned}`
  }

  return cleaned.slice(0, 500)
}

interface PlatformInfo {
  id: string
  name: string
  icon: React.ReactNode
  color: string
  darkColor: string
  getUserUrl: (userId: string) => string
  configKey: string
  isCustom?: boolean
}

interface CustomPlatformData {
  id: string
  name: string
  username: string
  iconType?: 'react-icons' | 'url'
  iconLibrary?: string
  iconName?: string
  iconUrl?: string
  color: string
  darkColor: string
  linkType: 'url' | 'popup'
  linkPattern?: string
  popupData?: CustomPlatformPopupData
}

interface CustomPlatformPopupData {
  type: 'text' | 'qrcode' | 'both'
  text?: string
  qrcodeUrl?: string
}

const PLATFORMS: readonly PlatformInfo[] = Object.freeze([
  {
    id: 'bilibili',
    name: 'Bilibili',
    icon: <SiBilibili aria-hidden="true" />,
    color: '#00A1D6',
    darkColor: '#00A1D6',
    getUserUrl: (uid: string) => `https://space.bilibili.com/${uid}`,
    configKey: 'bilibili_uid',
  },
  {
    id: 'steam',
    name: 'Steam',
    icon: <FaSteam aria-hidden="true" />,
    color: '#1B2838',
    darkColor: '#c7d5e0',
    getUserUrl: (steamId: string) =>
      `https://steamcommunity.com/profiles/${steamId}`,
    configKey: 'steam_id',
  },
  {
    id: 'github',
    name: 'GitHub',
    icon: <FaGithub aria-hidden="true" />,
    color: '#24292E',
    darkColor: '#e6edf3',
    getUserUrl: (username: string) => `https://github.com/${username}`,
    configKey: 'github_username',
  },
  {
    id: 'youtube',
    name: 'YouTube',
    icon: <SiYoutube aria-hidden="true" />,
    color: '#FF0000',
    darkColor: '#ff4d4d',
    getUserUrl: (channelId: string) => {
      const id = String(channelId).trim()
      if (id.startsWith('UC') && id.length >= 20) {
        return `https://www.youtube.com/channel/${id}`
      }
      return `https://www.youtube.com/@${id.replaceAll(/^@/g, '')}`
    },
    configKey: 'youtube_channel_id',
  },
  {
    id: 'netease',
    name: 'NetEase Music',
    icon: <SiNeteasecloudmusic aria-hidden="true" />,
    color: '#E60026',
    darkColor: '#E60026',
    getUserUrl: (userId: string) =>
      `https://music.163.com/#/user/home?id=${userId}`,
    configKey: 'netease_user_id',
  },
  {
    id: 'bangumi',
    name: 'Bangumi',
    icon: <SiBangumi aria-hidden="true" />,
    color: '#F09199',
    darkColor: '#F09199',
    getUserUrl: (username: string) => `https://bgm.tv/user/${username}`,
    configKey: 'bangumi_username',
  },
  {
    id: 'mal',
    name: 'MyAnimeList',
    icon: <SiMyanimelist aria-hidden="true" />,
    color: '#2E51A2',
    darkColor: '#2E51A2',
    getUserUrl: (username: string) =>
      `https://myanimelist.net/profile/${username}`,
    configKey: 'mal_username',
  },
  {
    id: 'x',
    name: 'X',
    icon: <FaXTwitter aria-hidden="true" />,
    color: '#000000',
    darkColor: '#e7e9ea',
    getUserUrl: (username: string) =>
      `https://x.com/${username.replaceAll(/^@/g, '')}`,
    configKey: 'x_username',
  },
])

const PLATFORM_INDEX_MAP: Record<string, number> = {
  bilibili: 0,
  steam: 1,
  github: 2,
  youtube: 3,
  netease: 4,
  bangumi: 5,
  mal: 6,
  x: 7,
}

const ICON_HOVER_ANIMATION = {
  scale: [1, 1.08, 1],
  rotate: [0, 5, -5, 0],
}

const ICON_STATIC_ANIMATION = {
  scale: 1,
  rotate: 0,
}

const ICON_STATIC_TRANSITION = { duration: 0.3 }

// 循环动画用有限次数，避免永久运行。
const LOOP_TRANSITION_FAST = {
  duration: 0.5,
  repeat: 6,
  ease: 'easeInOut' as const,
}

const LOOP_TRANSITION_NORMAL = {
  duration: 0.6,
  repeat: 5,
  ease: 'easeInOut' as const,
}

const NO_LOOP_TRANSITION_FAST = {
  duration: 0.5,
  ease: 'easeInOut' as const,
}

const NO_LOOP_TRANSITION_NORMAL = {
  duration: 0.6,
  ease: 'easeInOut' as const,
}

const ICON_LARGE_HOVER_ANIMATION = {
  scale: [1, 1.1, 1],
  rotate: [0, 8, -8, 0],
}

const HINT_ANIMATION = {
  initial: { opacity: 0, y: 5 },
  animate: { opacity: 1, y: 0 },
  transition: { delay: 0.2 },
}
const HINT_ARROW_ANIMATION = { x: [0, 2, 0] }
const HINT_LOOP_TRANSITION = { duration: 1, repeat: 3 }
const HINT_NO_LOOP_TRANSITION = { duration: 0.3 }

const platformInfoCache = new Map<
  string,
  { info: PlatformInfo; timestamp: number }
>()
const PLATFORM_INFO_CACHE_TTL = 60 * 1000
const MAX_PLATFORM_INFO_CACHE = 50

let customPlatformsData: CustomPlatformData[] = []
let customPlatformsLoaded = false
let customPlatformsLoadPromise: Promise<CustomPlatformData[]> | null = null

function getCustomPlatformsData(): CustomPlatformData[] {
  return Array.isArray(customPlatformsData) ? customPlatformsData : []
}

function loadCustomPlatforms(): CustomPlatformData[] {
  if (customPlatformsLoaded) {
    return getCustomPlatformsData()
  }

  if (!customPlatformsLoadPromise) {
    customPlatformsLoadPromise = loadCustomPlatformsAsync()
  }

  return getCustomPlatformsData()
}

async function loadCustomPlatformsAsync(): Promise<CustomPlatformData[]> {
  if (customPlatformsLoaded) {
    return getCustomPlatformsData()
  }

  try {
    const data = await getUIConfigDeduped()
    customPlatformsData = parseCustomPlatforms(
      data?.custom_platforms,
    ) as CustomPlatformData[]
  } catch (e) {
    console.error('Failed to load custom platforms from API:', e)
  }

  customPlatformsLoaded = true
  customPlatformsLoadPromise = null
  return getCustomPlatformsData()
}

// 必须 POST 并校验 ok；只 dispatch 的话刷新会丢。
async function saveCustomPlatforms(platforms: CustomPlatformData[]) {
  const previous = getCustomPlatformsData()
  customPlatformsData = platforms
  customPlatformsLoaded = true
  platformInfoCache.clear()

  try {
    const csrfToken = await getCSRFToken(true)
    if (!csrfToken) {
      throw new Error(currentCopy().errors.csrfUnavailable)
    }
    const response = await fetch(`${API_URL}/api/config/dashboard`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-CSRF-Token': csrfToken,
      },
      credentials: 'include',
      body: JSON.stringify({
        custom_platforms: JSON.stringify(platforms),
      }),
    })
    if (!response.ok) {
      throw new Error(`Failed to save custom platforms: HTTP ${response.status}`)
    }
    // 清 30s UI 配置缓存，刷新和其他小组件才能看到新列表。
    clearDedupCache(`${API_URL}/api/config/ui`)
    window.dispatchEvent(
      new CustomEvent('custom-platforms-update', {
        detail: { platforms, persisted: true },
      }),
    )
  } catch (err) {
    // 失败则回滚内存，与服务器一致。
    customPlatformsData = previous
    platformInfoCache.clear()
    console.error('Failed to persist custom platforms:', err)
    throw err
  }
}

async function addCustomPlatform(platform: CustomPlatformData) {
  const updated = [...getCustomPlatformsData(), platform]
  await saveCustomPlatforms(updated)
  return platform.id
}

async function removeCustomPlatform(platformId: string) {
  const updated = getCustomPlatformsData().filter((p) => p.id !== platformId)
  await saveCustomPlatforms(updated)
}

const DEFAULT_ICON = (
  <svg
    className="w-full h-full"
    viewBox="0 0 24 24"
    fill="currentColor"
    aria-hidden="true"
  >
    <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm-1-13h2v6h-2zm0 8h2v2h-2z" />
  </svg>
)

function customPlatformToPlatformInfo(
  custom: CustomPlatformData,
): PlatformInfo {
  const cached = platformInfoCache.get(custom.id)
  const now = Date.now()
  if (cached && now - cached.timestamp < PLATFORM_INFO_CACHE_TTL) {
    return cached.info
  }
  if (cached) platformInfoCache.delete(custom.id)

  let icon: React.ReactNode
  let iconPending = false

  if (custom.iconName) {
    requestNamedIcon(custom.iconName)
    const IconComponent = peekNamedIcon(custom.iconName)
    if (IconComponent) {
      icon = <IconComponent className="w-full h-full" aria-hidden="true" />
    } else if (IconComponent === undefined) {
      iconPending = true
    }
  }

  if (!icon && custom.iconUrl) {
    icon = (
      <img
        src={custom.iconUrl}
        alt={custom.name}
        className="w-full h-full object-contain"
        loading="lazy"
      />
    )
  }

  if (!icon) {
    icon = DEFAULT_ICON
  }

  const linkPattern = custom.linkPattern
  const linkType = custom.linkType
  const hasUsername = custom.username && custom.username.trim() !== ''
  const getUserUrl =
    linkType === 'url' && linkPattern
      ? hasUsername
        ? (userId: string) => linkPattern.replaceAll('{username}', userId)
        : () => linkPattern
      : () => '#'

  const info: PlatformInfo = {
    id: custom.id,
    name: custom.name,
    icon,
    color: custom.color,
    darkColor: custom.darkColor,
    getUserUrl,
    configKey: `custom_${custom.id}`,
    isCustom: true,
  }

  if (!iconPending) {
    platformInfoCache.delete(custom.id)
    platformInfoCache.set(custom.id, { info, timestamp: now })
    while (platformInfoCache.size > MAX_PLATFORM_INFO_CACHE) {
      const oldest = platformInfoCache.keys().next().value
      if (oldest === undefined) break
      platformInfoCache.delete(oldest)
    }
  }

  return info
}

function getAllPlatforms(): PlatformInfo[] {
  const customPlatforms = getCustomPlatformsData().map(
    customPlatformToPlatformInfo,
  )
  return [...PLATFORMS, ...customPlatforms]
}

// 全局只允许一个设置弹窗。
interface SettingsModalState {
  isOpen: boolean
  selectedPlatformId: string
  anchorRect?: DOMRect
  onSelect?: (platformId: string) => void
}

let globalModalState: SettingsModalState = {
  isOpen: false,
  selectedPlatformId: 'bilibili',
}

const modalStateListeners: Set<() => void> = new Set()

function openSettingsModal(
  selectedPlatformId: string,
  anchorRect: DOMRect,
  onSelect: (platformId: string) => void,
) {
  armWidgetSettingsHost()
  globalModalState = {
    isOpen: true,
    selectedPlatformId,
    anchorRect,
    onSelect,
  }
  modalStateListeners.forEach((listener) => listener())
}

function closeSettingsModal() {
  globalModalState = {
    ...globalModalState,
    isOpen: false,
  }
  modalStateListeners.forEach((listener) => listener())
}

function subscribeToModalState(listener: () => void) {
  modalStateListeners.add(listener)
  return () => {
    modalStateListeners.delete(listener)
  }
}

interface PlatformUserIds {
  bilibili_uid?: string
  steam_id?: string
  github_username?: string
  youtube_channel_id?: string
  netease_user_id?: string
  bangumi_username?: string
  mal_username?: string
  x_username?: string
}

let cachedPlatformUserIds: PlatformUserIds | null = null
let fetchPromise: Promise<PlatformUserIds> | null = null
let cacheTimestamp = 0
const CACHE_TTL = 5 * 60 * 1000

async function fetchPlatformUserIds(): Promise<PlatformUserIds> {
  const now = Date.now()

  if (cachedPlatformUserIds && now - cacheTimestamp < CACHE_TTL) {
    return cachedPlatformUserIds
  }

  if (fetchPromise) return fetchPromise

  fetchPromise = (async () => {
    try {
      // 公开端点；与报告卡共享去重缓存。
      const data = await getPublicConfigDeduped()
      const result: PlatformUserIds = {}

      if (data.platforms && Array.isArray(data.platforms)) {
        for (const platform of data.platforms) {
          for (const field of platform.config_fields || []) {
            if (
              platform.name === 'GitHub' &&
              field.key === 'username' &&
              field.value
            ) {
              result.github_username = field.value
            } else if (
              platform.name === 'Bilibili' &&
              field.key === 'uid' &&
              field.value
            ) {
              result.bilibili_uid = field.value
            } else if (
              platform.name === 'Steam' &&
              field.key === 'steam_id' &&
              field.value
            ) {
              result.steam_id = field.value
            } else if (
              platform.name === 'YouTube' &&
              field.key === 'channel_id' &&
              field.value
            ) {
              result.youtube_channel_id = field.value
            } else if (
              platform.name === 'Netease Music' &&
              field.key === 'user_id' &&
              field.value
            ) {
              result.netease_user_id = field.value
            } else if (
              platform.name === 'Bangumi' &&
              field.key === 'username' &&
              field.value
            ) {
              result.bangumi_username = field.value
            } else if (
              platform.name === 'MyAnimeList' &&
              field.key === 'username' &&
              field.value
            ) {
              result.mal_username = field.value
            } else if (
              platform.name === 'X' &&
              field.key === 'username' &&
              field.value
            ) {
              result.x_username = field.value
            }
          }
        }
      }

      cachedPlatformUserIds = result
      cacheTimestamp = now
      return result
    } catch {
      return cachedPlatformUserIds || {}
    } finally {
      fetchPromise = null
    }
  })()

  return fetchPromise
}

const CustomPlatformForm = memo(
  ({
    formData,
    onChange,
    onSubmit,
    onCancel,
    isGenerating,
  }: {
    formData: {
      name: string
      username: string
      linkType: 'url' | 'popup'
      linkPattern: string
      popupText: string
    }
    onChange: (data: any) => void
    onSubmit: () => void
    onCancel: () => void
    isGenerating: boolean
  }) => {
    const { t } = useI18n()
    return (
      <div className="space-y-4 max-h-96 overflow-y-auto">
        <div>
          <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1.5">
            {t.socialNetwork.platformName} *
          </label>
          <input
            type="text"
            value={formData.name}
            onChange={(e) => onChange({ ...formData, name: e.target.value })}
            placeholder={t.socialNetwork.platformNameHint}
            className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-gray-900 dark:text-gray-100 text-sm focus:ring-2 focus:ring-[var(--color-primary)] focus:border-transparent outline-none"
          />
        </div>

        <div>
          <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1.5">
            {t.socialNetwork.linkType}
          </label>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={() => onChange({ ...formData, linkType: 'url' })}
              className={`flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-all ${
                formData.linkType === 'url'
                  ? 'bg-[var(--color-primary)] text-white'
                  : 'bg-gray-100 dark:bg-neutral-800 text-gray-700 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-neutral-700'
              }`}
            >
              {t.socialNetwork.urlLink}
            </button>
            <button
              type="button"
              onClick={() => onChange({ ...formData, linkType: 'popup' })}
              className={`flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-all ${
                formData.linkType === 'popup'
                  ? 'bg-[var(--color-primary)] text-white'
                  : 'bg-gray-100 dark:bg-neutral-800 text-gray-700 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-neutral-700'
              }`}
            >
              {t.socialNetwork.infoPopup}
            </button>
          </div>
        </div>

        {formData.linkType === 'url' ? (
          <>
            <div>
              <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1.5">
                {t.socialNetwork.usernameId}{' '}
                <span className="text-gray-400 dark:text-gray-500">
                  ({t.socialNetwork.optional})
                </span>
              </label>
              <input
                type="text"
                value={formData.username}
                onChange={(e) =>
                  onChange({ ...formData, username: e.target.value })
                }
                placeholder={t.socialNetwork.usernameHint}
                className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-gray-900 dark:text-gray-100 text-sm focus:ring-2 focus:ring-[var(--color-primary)] focus:border-transparent outline-none"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1.5">
                {t.socialNetwork.urlPattern}{' '}
                {!formData.username && (
                  <span className="text-amber-500">*</span>
                )}
              </label>
              <input
                type="text"
                value={formData.linkPattern}
                onChange={(e) =>
                  onChange({ ...formData, linkPattern: e.target.value })
                }
                placeholder={
                  formData.username
                    ? t.socialNetwork.linkAutoGenerate
                    : t.socialNetwork.linkManualInput
                }
                className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-gray-900 dark:text-gray-100 text-sm focus:ring-2 focus:ring-[var(--color-primary)] focus:border-transparent outline-none"
              />
              <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                {formData.username
                  ? t.socialNetwork.usePlaceholder
                  : t.socialNetwork.noUsernameHint}
              </p>
            </div>
          </>
        ) : (
          <div>
            <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1.5">
              {t.socialNetwork.popupContent}
            </label>
            <textarea
              value={formData.popupText}
              onChange={(e) =>
                onChange({ ...formData, popupText: e.target.value })
              }
              placeholder={t.socialNetwork.popupHint}
              rows={4}
              className="w-full px-3 py-2 rounded-lg border border-gray-300 dark:border-neutral-700 bg-white dark:bg-neutral-900 text-gray-900 dark:text-gray-100 text-sm focus:ring-2 focus:ring-[var(--color-primary)] focus:border-transparent outline-none resize-none"
            />
          </div>
        )}

        <div className="flex gap-2 pt-2">
          <button
            type="button"
            onClick={onCancel}
            className="flex-1 px-4 py-2.5 rounded-lg bg-gray-100 dark:bg-neutral-800 text-gray-700 dark:text-gray-300 font-medium hover:bg-gray-200 dark:hover:bg-neutral-700 transition-colors"
          >
            {t.common.cancel}
          </button>
          <button
            type="button"
            onClick={onSubmit}
            disabled={
              isGenerating ||
              !formData.name ||
              (formData.linkType === 'url' &&
                !formData.username &&
                !formData.linkPattern) ||
              (formData.linkType === 'popup' && !formData.popupText)
            }
            className="flex-1 px-4 py-2.5 rounded-lg bg-[var(--color-primary)] text-white font-medium hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed transition-all"
          >
            {isGenerating ? (
              <span className="inline-flex justify-center">
                <Spinner size="xs" color="white" />
              </span>
            ) : (
              t.socialNetwork.create
            )}
          </button>
        </div>
      </div>
    )
  },
)

CustomPlatformForm.displayName = 'CustomPlatformForm'

function isValidImageUrl(url: string): boolean {
  try {
    const parsed = new URL(url)
    // 图片 URL 只允许 http(s)。
    if (!['http:', 'https:'].includes(parsed.protocol)) {
      return false
    }
    if (
      parsed.pathname.includes('..') ||
      parsed.search.includes('<') ||
      parsed.search.includes('>')
    ) {
      return false
    }
    return true
  } catch {
    return false
  }
}

function parsePopupContent(text?: string): {
  textContent: string
  imageUrls: string[]
} {
  if (!text) return { textContent: '', imageUrls: [] }

  const imageUrlRegex = /(https?:\/\/\S+\.(?:jpg|jpeg|png|gif|webp|bmp|svg))/gi
  const rawImageUrls = text.match(imageUrlRegex) || []

  const imageUrls = rawImageUrls.filter(isValidImageUrl)

  const textContent = text.replaceAll(imageUrlRegex, '').trim()

  return { textContent, imageUrls }
}

const TOOLTIP_ANIMATION = {
  initial: { opacity: 0, y: -4, x: '-50%', scale: 0.96 },
  animate: { opacity: 1, y: 0, x: '-50%', scale: 1 },
  exit: { opacity: 0, y: -4, x: '-50%', scale: 0.96 },
  transition: { duration: 0.12, ease: 'easeOut' as const },
}

const InfoTooltip = memo(
  ({
    isVisible,
    popupData,
    anchorRef,
  }: {
    isVisible: boolean
    popupData: CustomPlatformPopupData
    anchorRef: React.RefObject<HTMLDivElement | null>
  }) => {
    const { t } = useI18n()
    const [_copied, setCopied] = useState(false)

    const { textContent, imageUrls } = useMemo(
      () => parsePopupContent(popupData.text),
      [popupData.text],
    )

    const position = useMemo(() => {
      if (!isVisible || !anchorRef.current) return { top: 0, left: 0 }
      const rect = anchorRef.current.getBoundingClientRect()
      return {
        top: rect.bottom + 6,
        left: rect.left + rect.width / 2,
      }
    }, [isVisible, anchorRef])

    const handleCopy = useCallback(async () => {
      if (!textContent) return
      try {
        await navigator.clipboard.writeText(textContent)
        setCopied(true)
        setTimeout(setCopied, 1500, false)
      } catch (e) {
        console.error('Failed to copy:', e)
        showError(userFacingError(e, currentCopy().errors.clipboardFailed))
      }
    }, [textContent])

    if (!isVisible) return null

    const hasContent =
      textContent || imageUrls.length > 0 || popupData.qrcodeUrl

    return createPortal(
      <motion.div
        {...TOOLTIP_ANIMATION}
        className="fixed z-10001 pointer-events-auto w-fit h-fit"
        style={{ top: position.top, left: position.left }}
      >
        <div
          className="glass rounded-xl shadow-lg overflow-hidden border border-white/20 dark:border-white/10 w-fit h-fit max-w-xs mx-auto"
          style={{ minWidth: '120px' }}
          onClick={handleCopy}
        >
          {hasContent ? (
            <div className="p-3 space-y-2">
              {textContent && (
                <div className="cursor-pointer text-center">
                  <p className="text-sm font-medium text-gray-800 dark:text-gray-100 wrap-break-word whitespace-pre-wrap leading-relaxed">
                    {textContent}
                  </p>
                </div>
              )}

              {imageUrls.length > 0 && (
                <div className="flex flex-wrap justify-center gap-1.5">
                  {imageUrls.map((url, index) => (
                    <img
                      key={index}
                      src={url}
                      alt=""
                      className="max-h-24 rounded-lg object-contain"
                      loading="lazy"
                      onError={(e) => {
                        ;(e.target as HTMLImageElement).style.display = 'none'
                      }}
                    />
                  ))}
                </div>
              )}

              {popupData.qrcodeUrl && (
                <img
                  src={popupData.qrcodeUrl}
                  alt="QR"
                  className="w-32 h-32 rounded-lg object-contain mx-auto"
                  loading="lazy"
                />
              )}
            </div>
          ) : (
            <div className="p-3 text-center">
              <span className="text-xs text-gray-400">
                {t.socialNetwork.noContent}
              </span>
            </div>
          )}
        </div>
      </motion.div>,
      document.body,
    )
  },
)

InfoTooltip.displayName = 'InfoTooltip'

const GlobalSettingsModal = memo(() => {
  const [, forceUpdate] = useState({})
  const { t } = useI18n()

  const [allPlatforms, setAllPlatforms] = useState<PlatformInfo[]>(() => {
    return getAllPlatforms()
  })

  useEffect(() => {
    loadCustomPlatformsAsync().then(() => {
      setAllPlatforms(getAllPlatforms())
    })
  }, [])

  const [showCustomForm, setShowCustomForm] = useState(false)
  const [customFormData, setCustomFormData] = useState({
    name: '',
    username: '',
    linkType: 'url' as 'url' | 'popup',
    linkPattern: '',
    popupText: '',
  })
  const [isGeneratingIcon, setIsGeneratingIcon] = useState(false)

  useEffect(() => {
    return subscribeToModalState(() => {
      forceUpdate({})
    })
  }, [])

  const { isOpen, selectedPlatformId, anchorRect, onSelect } = globalModalState

  const handleSelect = useCallback(
    (platformId: string) => {
      if (onSelect) {
        onSelect(platformId)
      }
      closeSettingsModal()
    },
    [onSelect],
  )

  const handleCustomFormSubmit = useCallback(async () => {
    if (!customFormData.name) {
      showError(t.socialNetworkWidget.fillPlatformName)
      return
    }

    if (
      customFormData.linkType === 'url' &&
      !customFormData.username &&
      !customFormData.linkPattern
    ) {
      showError(t.socialNetworkWidget.fillUsernameOrUrl)
      return
    }
    if (customFormData.linkType === 'popup' && !customFormData.popupText) {
      showError(t.socialNetworkWidget.fillPopupContent)
      return
    }

    const urlPattern = customFormData.linkPattern || ''
    if (
      customFormData.linkType === 'url' &&
      urlPattern &&
      !isValidUrlPattern(urlPattern)
    ) {
      showError(t.socialNetworkWidget.invalidUrlPattern)
      return
    }

    setIsGeneratingIcon(true)

    try {
      const customId = `custom_${Date.now()}_${Math.random().toString(36).slice(2, 11)}`

      let iconData: {
        iconType?: 'react-icons' | 'url'
        iconLibrary?: string
        iconName?: string
        iconUrl?: string
        recommendedColor?: string
        urlPattern?: string
      } = {}

      try {
        const response = await fetch(`${API_URL}/api/ai/recommend-icon`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
          },
          body: JSON.stringify({
            platform_name: customFormData.name,
          }),
        })

        if (response.ok) {
          const data = await response.json()
          iconData = {
            iconType: data.icon_type as 'react-icons' | 'url',
            iconLibrary: data.icon_library,
            iconName: data.icon_name,
            iconUrl: data.icon_url,
            recommendedColor: data.color_suggestion,
            urlPattern: data.url_pattern,
          }
        }
      } catch (error) {
        console.error('Failed to get AI icon recommendation:', error)
      }

      const rawLinkPattern =
        customFormData.linkPattern || iconData.urlPattern || ''
      const finalLinkPattern = sanitizeUrlPattern(rawLinkPattern)

      // AI 推荐的 URL 也要再验一次。
      if (
        customFormData.linkType === 'url' &&
        finalLinkPattern &&
        !isValidUrlPattern(finalLinkPattern)
      ) {
        console.warn(
          'AI recommended URL pattern is invalid, using empty pattern',
        )
      }

      const newPlatform: CustomPlatformData = {
        id: customId,
        name: sanitizePlatformName(customFormData.name),
        username:
          customFormData.linkType === 'url'
            ? sanitizeUsername(customFormData.username)
            : '',
        iconType: iconData.iconType || 'react-icons',
        iconLibrary: iconData.iconLibrary,
        iconName: iconData.iconName,
        iconUrl: iconData.iconUrl,
        color: iconData.recommendedColor || '#6366f1',
        darkColor: iconData.recommendedColor || '#6366f1',
        linkType: customFormData.linkType,
        linkPattern:
          customFormData.linkType === 'url' &&
          isValidUrlPattern(finalLinkPattern)
            ? finalLinkPattern
            : undefined,
        popupData:
          customFormData.linkType === 'popup'
            ? {
                type: 'text',
                text: sanitizePopupText(customFormData.popupText),
              }
            : undefined,
      }

      await addCustomPlatform(newPlatform)

      setAllPlatforms(getAllPlatforms())

      setCustomFormData({
        name: '',
        username: '',
        linkType: 'url',
        linkPattern: '',
        popupText: '',
      })
      setShowCustomForm(false)

      if (onSelect) {
        onSelect(customId)
      }
      closeSettingsModal()
    } catch (error) {
      console.error('Failed to create custom platform:', error)
      showError(
        userFacingError(error, t.socialNetworkWidget.createCustomPlatformFailed),
      )
    } finally {
      setIsGeneratingIcon(false)
    }
  }, [customFormData, onSelect, t])

  const handleDeleteCustomPlatform = useCallback(
    async (platformId: string) => {
      if (confirm(t.socialNetworkWidget.confirmDeleteCustomPlatform)) {
        try {
          await removeCustomPlatform(platformId)
          setAllPlatforms(getAllPlatforms())
        } catch (error) {
          showError(
            userFacingError(
              error,
              t.socialNetworkWidget.deleteCustomPlatformFailed,
            ),
          )
        }
      }
    },
    [t],
  )

  return (
    <WidgetSettingsTip
      open={isOpen}
      anchor={anchorRect ?? null}
      title={t.widgets.socialNetwork}
      width={300}
      height={420}
      onClose={closeSettingsModal}
    >
      {!showCustomForm ? (
        <>
          <WidgetSettingsSection label={t.socialNetwork.selectPlatform}>
            <div className="widget-settings-tip__body space-y-1.5">
              {allPlatforms.map((platform) => (
                <PlatformButton
                  key={platform.id}
                  platform={platform}
                  isSelected={selectedPlatformId === platform.id}
                  onSelect={handleSelect}
                  onDelete={
                    platform.isCustom ? handleDeleteCustomPlatform : undefined
                  }
                  deleteLabel={t.socialNetworkWidget.delete}
                />
              ))}
            </div>
          </WidgetSettingsSection>
          <WidgetSettingsSection>
            <button
              type="button"
              onClick={() => setShowCustomForm(true)}
              className="widget-settings-tip__save"
            >
              {t.socialNetwork.create}
            </button>
          </WidgetSettingsSection>
        </>
      ) : (
        <CustomPlatformForm
          formData={customFormData}
          onChange={setCustomFormData}
          onSubmit={handleCustomFormSubmit}
          onCancel={() => {
            setShowCustomForm(false)
            setCustomFormData({
              name: '',
              username: '',
              linkType: 'url',
              linkPattern: '',
              popupText: '',
            })
          }}
          isGenerating={isGeneratingIcon}
        />
      )}
    </WidgetSettingsTip>
  )
})

GlobalSettingsModal.displayName = 'GlobalSettingsModal'

const PlatformButton = memo(
  ({
    platform,
    isSelected,
    onSelect,
    onDelete,
    deleteLabel,
  }: {
    platform: PlatformInfo
    isSelected: boolean
    onSelect: (platformId: string) => void
    onDelete?: (platformId: string) => void
    deleteLabel?: string
  }) => {
    const handleClick = useCallback(() => {
      onSelect(platform.id)
    }, [onSelect, platform.id])

    const handleDelete = useCallback(
      (e: React.MouseEvent) => {
        e.stopPropagation()
        if (onDelete) {
          onDelete(platform.id)
        }
      },
      [onDelete, platform.id],
    )

    return (
      <button
        type="button"
        onClick={handleClick}
        className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-xl transition-all duration-200 ${
          isSelected
            ? 'bg-black/5 dark:bg-white/10 ring-1 ring-black/10 dark:ring-white/20 hover:bg-black/10 dark:hover:bg-white/15'
            : 'hover:bg-black/5 dark:hover:bg-white/8 hover:shadow-sm active:scale-[0.98]'
        }`}
      >
        <div
          className={`w-8 h-8 rounded-lg flex items-center justify-center text-white shrink-0 ${platform.isCustom ? 'p-1.5' : 'text-lg'}`}
          style={{ backgroundColor: platform.color }}
        >
          {platform.icon}
        </div>
        <span className="font-medium text-sm text-gray-800 dark:text-gray-200 truncate">
          {platform.name}
        </span>
        {isSelected && !onDelete && (
          <div
            className="ml-auto w-5 h-5 rounded-full flex items-center justify-center shrink-0"
            style={{ backgroundColor: platform.color }}
          >
            <svg
              className="w-3 h-3 text-white"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={3}
                d="M5 13l4 4L19 7"
              />
            </svg>
          </div>
        )}
        {onDelete && (
          <div
            role="button"
            tabIndex={0}
            onClick={handleDelete}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault()
                handleDelete(e as any)
              }
            }}
            className="ml-auto w-6 h-6 rounded-full flex items-center justify-center hover:bg-red-500/10 text-red-500 transition-colors shrink-0 cursor-pointer"
            title={deleteLabel || 'Delete'}
          >
            <svg
              className="w-3.5 h-3.5"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
              />
            </svg>
          </div>
        )}
      </button>
    )
  },
)

PlatformButton.displayName = 'PlatformButton'

export const SocialNetworkWidget = memo(
  ({ config, isEditMode, isPreview, onConfigChange }: WidgetComponentProps) => {
    const { containerRef, fontScale } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const anim = useAnimationLevel()
    const { t } = useI18n()
    useSyncExternalStore(
      subscribeNamedIcons,
      namedIconVersion,
      namedIconVersion,
    )

    const { isAnimating } = useLoopAnimation({
      duration: 600,
      trigger: 'mount',
      enabled: anim.loop,
    })

    const localRef = useRef<HTMLDivElement | null>(null)

    const isDark = useThemeMode()

    const [selectedPlatformId, setSelectedPlatformId] = useState<string>(
      config.config?.platformId || 'bilibili',
    )
    const [platformUserIds, setPlatformUserIds] = useState<PlatformUserIds>({})
    const [isHovered, setIsHovered] = useState(false)

    const [showInfoTooltip, setShowInfoTooltip] = useState(false)

    const [customPlatformsReady, setCustomPlatformsReady] = useState(
      customPlatformsLoaded,
    )

    const longPressTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
    const isLongPressRef = useRef(false)

    const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

    useEffect(() => {
      if (!customPlatformsLoaded) {
        loadCustomPlatformsAsync().then(() => {
          setCustomPlatformsReady(true)
        })
      }
    }, [])

    const selectedPlatform = useMemo(() => {
      const index = PLATFORM_INDEX_MAP[selectedPlatformId]
      if (index !== undefined) {
        return PLATFORMS[index]
      }

      if (customPlatformsReady) {
        const customPlatform = getCustomPlatformsData().find(
          (p) => p.id === selectedPlatformId,
        )
        if (customPlatform) {
          return customPlatformToPlatformInfo(customPlatform)
        }
      }

      return PLATFORMS[0]
    }, [selectedPlatformId, customPlatformsReady])

    const userId = useMemo(() => {
      switch (selectedPlatformId) {
        case 'bilibili':
          return platformUserIds.bilibili_uid
        case 'steam':
          return platformUserIds.steam_id
        case 'github':
          return platformUserIds.github_username
        case 'youtube':
          return platformUserIds.youtube_channel_id
        case 'netease':
          return platformUserIds.netease_user_id
        case 'bangumi':
          return platformUserIds.bangumi_username
        case 'mal':
          return platformUserIds.mal_username
        case 'x':
          return platformUserIds.x_username
        default: {
          if (customPlatformsReady) {
            const customPlatform = getCustomPlatformsData().find(
              (p) => p.id === selectedPlatformId,
            )
            if (customPlatform) {
              if (customPlatform.linkType === 'popup') {
                return '__popup__'
              }
              if (
                customPlatform.username &&
                customPlatform.username.trim() !== ''
              ) {
                return customPlatform.username
              }
              if (customPlatform.linkPattern) {
                return '__direct_url__'
              }
            }
          }
          return undefined
        }
      }
    }, [selectedPlatformId, platformUserIds, customPlatformsReady])

    useEffect(() => {
      if (isPreview) return
      fetchPlatformUserIds().then(setPlatformUserIds)
    }, [isPreview])

    useEffect(() => {
      if (
        config.config?.platformId &&
        config.config.platformId !== selectedPlatformId
      ) {
        setSelectedPlatformId(config.config.platformId)
      }
    }, [config.config?.platformId])

    const handleSelectPlatform = useCallback(
      (platformId: string) => {
        setSelectedPlatformId(platformId)
        if (typeof onConfigChange === 'function') {
          onConfigChange({ ...config.config, platformId })
        } else {
          window.dispatchEvent(
            new CustomEvent('widget-config-update', {
              detail: {
                widgetId: config.id,
                config: { ...config.config, platformId },
              },
            }),
          )
        }
      },
      [config.id, config.config, onConfigChange],
    )

    const openSettings = useCallback(() => {
      if (!localRef.current) return

      openSettingsModal(
        selectedPlatformId,
        localRef.current.getBoundingClientRect(),
        handleSelectPlatform,
      )
    }, [selectedPlatformId, handleSelectPlatform])

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

    const popupPlatformData = useMemo(() => {
      loadCustomPlatforms()
      const customPlatform = getCustomPlatformsData().find(
        (p) => p.id === selectedPlatformId,
      )
      if (customPlatform?.linkType === 'popup') {
        return {
          popupData: customPlatform.popupData || {
            type: 'text' as const,
            text: '',
          },
          color: customPlatform.color,
        }
      }
      return null
    }, [selectedPlatformId])

    const isPopupType = popupPlatformData !== null
    const hasInteraction = !isEditMode && !!userId

    const handleClick = useCallback(async () => {
      // 长按打开设置时不要当点击跳转。
      if (isLongPressRef.current) {
        isLongPressRef.current = false
        return
      }

      if (isEditMode) return

      if (popupPlatformData?.popupData.text) {
        try {
          const { textContent } = parsePopupContent(
            popupPlatformData.popupData.text,
          )
          if (textContent) {
            await navigator.clipboard.writeText(textContent)
          }
        } catch (err) {
          console.error('Failed to copy:', err)
          showError(
            userFacingError(err, currentCopy().errors.clipboardFailed),
          )
        }
        return
      }

      if (userId && userId !== '__popup__') {
        const url = selectedPlatform.getUserUrl(
          userId === '__direct_url__' ? '' : userId,
        )
        if (url !== '#') {
          window.open(url, '_blank', 'noopener,noreferrer')
        }
      }
    }, [isEditMode, userId, selectedPlatform, popupPlatformData])

    const handleKeyDown = useCallback(
      (event: React.KeyboardEvent<HTMLDivElement>) => {
        if (
          hasInteraction &&
          (event.key === 'Enter' || event.key === ' ')
        ) {
          event.preventDefault()
          void handleClick()
        }
      },
      [handleClick, hasInteraction],
    )

    useEffect(() => {
      return () => {
        if (longPressTimerRef.current) {
          clearTimeout(longPressTimerRef.current)
        }
        if (hoverTimerRef.current) {
          clearTimeout(hoverTimerRef.current)
        }
        if (tooltipHideTimerRef.current) {
          clearTimeout(tooltipHideTimerRef.current)
        }
      }
    }, [])

    const iconColor = useMemo(
      () => (isDark ? selectedPlatform.darkColor : selectedPlatform.color),
      [isDark, selectedPlatform.darkColor, selectedPlatform.color],
    )

    const canLoopAnimate = anim.loop && isAnimating
    const loopTransitionFast = canLoopAnimate
      ? LOOP_TRANSITION_FAST
      : NO_LOOP_TRANSITION_FAST
    const loopTransitionNormal = canLoopAnimate
      ? LOOP_TRANSITION_NORMAL
      : NO_LOOP_TRANSITION_NORMAL
    const hintLoopTransition = canLoopAnimate
      ? HINT_LOOP_TRANSITION
      : HINT_NO_LOOP_TRANSITION

    const content = useMemo(() => {
      if (config.size === '1x1') {
        return (
          <div className="h-full w-full flex items-center justify-center">
            <motion.div
              className="text-3xl"
              layout={false}
              style={{ color: iconColor }}
              animate={isHovered ? ICON_HOVER_ANIMATION : ICON_STATIC_ANIMATION}
              transition={
                isHovered ? loopTransitionNormal : ICON_STATIC_TRANSITION
              }
            >
              {selectedPlatform.icon}
            </motion.div>
          </div>
        )
      }

      if (config.size === '2x1') {
        return (
          <div className="h-full w-full flex items-center justify-center gap-3 px-4">
            <motion.div
              className="text-2xl shrink-0"
              layout={false}
              style={{ color: iconColor }}
              animate={
                isHovered ? ICON_LARGE_HOVER_ANIMATION : ICON_STATIC_ANIMATION
              }
              transition={
                isHovered ? loopTransitionFast : ICON_STATIC_TRANSITION
              }
            >
              {selectedPlatform.icon}
            </motion.div>
            <span
              className="font-bold text-gray-800 dark:text-gray-100 truncate"
              style={{ fontSize: `${18 * fontScale}px` }}
            >
              {selectedPlatform.name}
            </span>
          </div>
        )
      }

      const popupContent = popupPlatformData
        ? parsePopupContent(popupPlatformData.popupData.text)
        : null
      const is2x2Popup =
        userId === '__popup__' && popupContent && popupPlatformData

      if (is2x2Popup) {
        const hasOnlyImages =
          !popupContent.textContent &&
          (popupContent.imageUrls.length > 0 ||
            popupPlatformData.popupData.qrcodeUrl)

        return (
          <div className="h-full w-full relative flex flex-col p-3 cursor-pointer group">
            {popupContent.textContent && (
              <div className="absolute top-2 right-2 text-[10px] text-gray-400 dark:text-gray-500 opacity-0 group-hover:opacity-100 transition-opacity flex items-center gap-0.5">
                <span>{t.socialNetwork.copy}</span>
                <svg
                  className="w-3 h-3"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z"
                  />
                </svg>
              </div>
            )}

            <div className="flex items-center gap-2 mb-1">
              <div className="text-lg shrink-0" style={{ color: iconColor }}>
                {selectedPlatform.icon}
              </div>
              <span className="text-xs font-medium text-gray-600 dark:text-gray-400 truncate">
                {selectedPlatform.name}
              </span>
            </div>

            <div className="flex-1 flex flex-col justify-center overflow-hidden">
              {popupContent.textContent && (
                <p className="text-base font-medium text-gray-800 dark:text-gray-100 wrap-break-word whitespace-pre-wrap leading-snug line-clamp-4 text-center">
                  {popupContent.textContent}
                </p>
              )}
              {popupContent.imageUrls.length > 0 && (
                <div
                  className={`flex justify-center gap-2 ${popupContent.textContent ? 'mt-2' : ''}`}
                >
                  {popupContent.imageUrls.slice(0, 2).map((url, index) => (
                    <img
                      key={index}
                      src={url}
                      alt=""
                      className={`rounded-lg object-contain ${hasOnlyImages ? 'max-h-24 max-w-[48%]' : 'max-h-20 max-w-[48%]'}`}
                      loading="lazy"
                    />
                  ))}
                </div>
              )}
              {popupPlatformData.popupData.qrcodeUrl && (
                <div
                  className={`flex justify-center ${popupContent.textContent || popupContent.imageUrls.length > 0 ? 'mt-2' : ''}`}
                >
                  <img
                    src={popupPlatformData.popupData.qrcodeUrl}
                    alt="QR"
                    className={`rounded-lg object-contain ${hasOnlyImages ? 'w-24 h-24' : 'w-20 h-20'}`}
                    loading="lazy"
                  />
                </div>
              )}
            </div>
          </div>
        )
      }

      return (
        <div className="h-full w-full flex flex-col items-center justify-center gap-2 p-4">
          <motion.div
            className="text-4xl"
            layout={false}
            style={{ color: iconColor }}
            animate={
              isHovered ? ICON_LARGE_HOVER_ANIMATION : ICON_STATIC_ANIMATION
            }
            transition={isHovered ? loopTransitionFast : ICON_STATIC_TRANSITION}
          >
            {selectedPlatform.icon}
          </motion.div>
          <div className="text-center w-full">
            <div
              className="font-bold text-gray-800 dark:text-gray-100"
              style={{ fontSize: `${16 * fontScale}px` }}
            >
              {selectedPlatform.name}
            </div>
            <motion.div
              className={`mt-1 flex items-center justify-center gap-1 ${userId ? 'text-gray-500 dark:text-gray-400' : 'text-amber-500 dark:text-amber-400'}`}
              style={{ fontSize: `${10 * fontScale}px` }}
              layout={false}
              {...HINT_ANIMATION}
            >
              {userId ? (
                <>
                  <span>{t.socialNetwork.clickToVisit}</span>
                  <motion.span
                    layout={false}
                    animate={anim.loop ? HINT_ARROW_ANIMATION : { x: 0 }}
                    transition={hintLoopTransition}
                  >
                    →
                  </motion.span>
                </>
              ) : (
                <>
                  <svg
                    className="w-3 h-3"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth={2}
                      d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
                    />
                  </svg>
                  <span>{t.socialNetwork.notConfigured}</span>
                </>
              )}
            </motion.div>
          </div>
        </div>
      )
    }, [
      config.size,
      iconColor,
      isHovered,
      selectedPlatform,
      fontScale,
      userId,
      popupPlatformData,
      loopTransitionFast,
      hintLoopTransition,
      anim.loop,
    ])

    const containerHoverProps = useMemo(
      () =>
        hasInteraction
          ? {
              whileHover: { filter: 'brightness(1.03)' },
              whileTap: { scale: 0.98 },
            }
          : {},
      [hasInteraction],
    )

    const shouldAnimateGlow = anim.loop

    const tooltipHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
      null,
    )

    const handleMouseLeave = useCallback(() => {
      handlePressEnd()
      setIsHovered(false)
      if (hoverTimerRef.current) {
        clearTimeout(hoverTimerRef.current)
        hoverTimerRef.current = null
      }
      if (tooltipHideTimerRef.current) {
        clearTimeout(tooltipHideTimerRef.current)
      }
      tooltipHideTimerRef.current = setTimeout(() => {
        setShowInfoTooltip(false)
        tooltipHideTimerRef.current = null
      }, 100)
    }, [handlePressEnd])

    const handleMouseEnter = useCallback(() => {
      if (!isEditMode && userId) {
        setIsHovered(true)
      }
      if (
        !isEditMode &&
        isPopupType &&
        popupPlatformData &&
        config.size !== '2x2'
      ) {
        if (hoverTimerRef.current) {
          clearTimeout(hoverTimerRef.current)
        }
        hoverTimerRef.current = setTimeout(() => {
          setShowInfoTooltip(true)
        }, 300)
      }
    }, [isEditMode, userId, isPopupType, popupPlatformData, config.size])

    const mergedRef = useCallback(
      (node: HTMLDivElement | null) => {
        localRef.current = node
        if (typeof containerRef === 'function') {
          containerRef(node)
        }
      },
      [containerRef],
    )

    return (
      <>
        <WidgetShell
          as={motion.div}
          containerRef={mergedRef}
          padding={0}
          className={`${
            hasInteraction ? 'cursor-pointer' : ''
          } ${isEditMode ? 'cursor-grab' : ''}`}
          style={{
            // 用 filter 替代 box-shadow，避免影响布局。
            transition: 'filter 0.3s ease, border-color 0.3s ease',
          }}
          background={
            <GlowBackground
              color={selectedPlatform.color}
              animLevel={anim.level}
              shouldAnimate={shouldAnimateGlow}
            />
          }
          rootProps={{
            ...containerHoverProps,
            role: hasInteraction ? 'button' : undefined,
            tabIndex: hasInteraction ? 0 : undefined,
            'aria-label': hasInteraction
              ? `${selectedPlatform.name}: ${t.socialNetwork.clickToVisit}`
              : undefined,
            onClick: handleClick,
            onKeyDown: handleKeyDown,
            onMouseDown: handlePressStart,
            onMouseUp: handlePressEnd,
            onMouseLeave: handleMouseLeave,
            onMouseEnter: handleMouseEnter,
            onTouchStart: handlePressStart,
            onTouchEnd: handlePressEnd,
            onTouchCancel: handlePressEnd,
          }}
        >
          {content}
          <WidgetLongPressHint
            visible={isEditMode}
            title={t.socialNetwork.longPressToEdit}
            onClick={openSettings}
          />
        </WidgetShell>

        <AnimatePresence>
          {showInfoTooltip && popupPlatformData && (
            <InfoTooltip
              isVisible={showInfoTooltip}
              popupData={popupPlatformData.popupData}
              anchorRef={localRef}
            />
          )}
        </AnimatePresence>
      </>
    )
  },
)

SocialNetworkWidget.displayName = 'SocialNetworkWidget'

export { GlobalSettingsModal as SocialNetworkSettingsModal }
