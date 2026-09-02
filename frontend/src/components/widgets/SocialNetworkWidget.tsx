/**
 * 社交网络小组件 - 显示平台链接入口
 * 支持 1x1, 2x1, 2x2 三种尺寸
 * glass风格 + 微动态效果
 * 长按设置面板选择平台
 *
 * 核心特性:
 * - 全局单例弹窗管理（发布/订阅模式）
 * - 自定义平台支持（后端数据库持久化，随 ui_config.custom_platforms 保存）
 * - XSS 安全防护（URL/文本消毒）
 * - 动画级别自适应
 */

import type { WidgetComponentProps } from '../WidgetGrid'
import {
  FaGithub,
  FaSteam,
  FaTimes,
  FaXTwitter,
  getIconByName,
  SiBangumi,
  SiBilibili,
  SiMyanimelist,
  SiNeteasecloudmusic,
  SiYoutube,
} from '@lib/icons'
import {
  AnimatePresenceShim as AnimatePresence,
  motionShim as motion,
} from '@lib/motionShim'

import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { API_URL } from '../../config'
import { useI18n } from '../../contexts/I18nContext'
import { useLoopAnimation } from '../../hooks/animation'
import { useAnimationLevel } from '../../hooks/useAnimationLevel'
import { useWidgetSize } from '../../hooks/useWidgetSize'
import { currentCopy } from '../../i18n/localeCopy'
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
import { WidgetShell } from './shared/WidgetShell'

// 使用内联 SVG 图标，避免 react-icons 全量导入

// 安全验证工具函数

// 预编译正则表达式（避免每次调用时重新创建）
const HTML_TAG_REGEX = /<[^>]*>/g
const DANGEROUS_CHARS_REGEX = /[<>"'`\\]/g
const DANGEROUS_CHARS_WITH_AMP_REGEX = /[<>"'`\\&]/g
const VALID_PROTOCOLS_REGEX = /^(https?:\/\/|mailto:)/i
const DANGEROUS_PROTOCOLS_REGEX = /^(javascript:|data:|vbscript:|file:)/i
const SUSPICIOUS_CHARS_REGEX = /[<>"'`\\]/

// HTML 实体转义，防止 XSS
function escapeHtml(text: string): string {
  const div = document.createElement('div')
  div.textContent = text
  return div.innerHTML
}

// 验证 URL 模式是否安全
function isValidUrlPattern(pattern: string): boolean {
  if (!pattern) return true // 空字符串允许

  // 必须以 http:// 或 https:// 或 mailto: 开头
  if (!VALID_PROTOCOLS_REGEX.test(pattern)) {
    return false
  }

  // 禁止危险协议
  if (DANGEROUS_PROTOCOLS_REGEX.test(pattern)) {
    return false
  }

  // 禁止包含可疑字符（防止注入）
  if (SUSPICIOUS_CHARS_REGEX.test(pattern.replace('{username}', ''))) {
    return false
  }

  return true
}

// 消毒平台名称
function sanitizePlatformName(name: string): string {
  // 移除 HTML 标签和危险字符，限制长度
  return name
    .replace(HTML_TAG_REGEX, '') // 移除 HTML 标签
    .replace(DANGEROUS_CHARS_REGEX, '') // 移除危险字符
    .trim()
    .slice(0, 50) // 限制长度
}

// 消毒用户名
function sanitizeUsername(username: string): string {
  // 移除危险字符，限制长度
  return username
    .replace(DANGEROUS_CHARS_WITH_AMP_REGEX, '') // 移除危险字符
    .trim()
    .slice(0, 100) // 限制长度
}

// 消毒弹窗文本
function sanitizePopupText(text: string): string {
  // 转义 HTML 实体，限制长度
  return escapeHtml(text.trim().slice(0, 500))
}

// 消毒 URL 模式
function sanitizeUrlPattern(pattern: string): string {
  if (!pattern) return ''

  // 移除空白字符
  let cleaned = pattern.trim()

  // 如果没有协议，添加 https://
  if (cleaned && !VALID_PROTOCOLS_REGEX.test(cleaned)) {
    cleaned = `https://${cleaned}`
  }

  return cleaned.slice(0, 500) // 限制长度
}

// 结束安全验证工具函数

// 平台配置定义 - 移到组件外部避免重复创建
interface PlatformInfo {
  id: string
  name: string
  icon: React.ReactNode
  color: string
  darkColor: string
  getUserUrl: (userId: string) => string
  configKey: string
  isCustom?: boolean // 标识是否为自定义平台
}

// 自定义平台扩展数据
interface CustomPlatformData {
  id: string // 唯一标识
  name: string // 平台名称
  username: string // 用户名
  iconType?: 'react-icons' | 'url' // 图标类型
  iconLibrary?: string // react-icons库名称（fa/si/fa6）
  iconName?: string // react-icons图标名称
  iconUrl?: string // 外部图标URL
  color: string // 主题色
  darkColor: string // 暗色主题色
  linkType: 'url' | 'popup' // 链接类型：URL或信息弹窗
  linkPattern?: string // URL模式，例如: https://example.com/{username}
  popupData?: CustomPlatformPopupData // 信息弹窗数据
}

// 信息弹窗数据
interface CustomPlatformPopupData {
  type: 'text' | 'qrcode' | 'both' // 显示类型：纯文本、二维码或两者
  text?: string // 显示的文本（如ID、数字等）
  qrcodeUrl?: string // 二维码图片URL
}

// 静态平台配置 - 使用 Object.freeze 防止意外修改
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
      return `https://www.youtube.com/@${id.replace(/^@/, '')}`
    },
    configKey: 'youtube_channel_id',
  },
  {
    id: 'netease',
    name: 'NetEase Music', // Will be translated in component
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
      `https://x.com/${username.replace(/^@/, '')}`,
    configKey: 'x_username',
  },
])

// 平台ID到索引的映射 - 避免重复查找
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

// 静态动画配置常量 - 避免每次渲染创建新对象
const ICON_HOVER_ANIMATION = {
  scale: [1, 1.08, 1],
  rotate: [0, 5, -5, 0],
}

const ICON_STATIC_ANIMATION = {
  scale: 1,
  rotate: 0,
}

const ICON_STATIC_TRANSITION = { duration: 0.3 }

// 循环动画过渡配置 - 有限次数（视觉上与无限循环等价但不会永久运行）
const LOOP_TRANSITION_FAST = {
  duration: 0.5,
  repeat: 6, // ~3s
  ease: 'easeInOut' as const,
}

const LOOP_TRANSITION_NORMAL = {
  duration: 0.6,
  repeat: 5, // ~3s
  ease: 'easeInOut' as const,
}

// 非循环版本 - 用于低端设备
const NO_LOOP_TRANSITION_FAST = {
  duration: 0.5,
  ease: 'easeInOut' as const,
}

const NO_LOOP_TRANSITION_NORMAL = {
  duration: 0.6,
  ease: 'easeInOut' as const,
}

// 2x1/2x2 尺寸图标动画配置 - 更大的动画幅度
const ICON_LARGE_HOVER_ANIMATION = {
  scale: [1, 1.1, 1],
  rotate: [0, 8, -8, 0],
}

// 提示文字动画
const HINT_ANIMATION = {
  initial: { opacity: 0, y: 5 },
  animate: { opacity: 1, y: 0 },
  transition: { delay: 0.2 },
}
const HINT_ARROW_ANIMATION = { x: [0, 2, 0] }
const HINT_LOOP_TRANSITION = { duration: 1, repeat: 3 } // ~3s
const HINT_NO_LOOP_TRANSITION = { duration: 0.3 }

// 自定义平台 PlatformInfo 缓存
// 注意：需要在 saveCustomPlatforms 之前声明
const platformInfoCache = new Map<
  string,
  { info: PlatformInfo; timestamp: number }
>()
const PLATFORM_INFO_CACHE_TTL = 60 * 1000 // 1分钟缓存

// 自定义平台存储管理
let customPlatformsData: CustomPlatformData[] = []
let customPlatformsLoaded = false // 标记是否已加载
let customPlatformsLoadPromise: Promise<CustomPlatformData[]> | null = null

function getCustomPlatformsData(): CustomPlatformData[] {
  return Array.isArray(customPlatformsData) ? customPlatformsData : []
}

// 从后端API加载自定义平台
function loadCustomPlatforms(): CustomPlatformData[] {
  // 如果已加载，直接返回缓存的数据
  if (customPlatformsLoaded) {
    return getCustomPlatformsData()
  }

  // 触发异步加载（如果还没有）
  if (!customPlatformsLoadPromise) {
    customPlatformsLoadPromise = loadCustomPlatformsAsync()
  }

  return getCustomPlatformsData()
}

// 异步从后端加载自定义平台（使用去重机制）
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

// 保存自定义平台：必须真正 POST 到后端并校验 response.ok。
// 历史上只 dispatchEvent + Home 空 CSRF 头 → 内存有、刷新丢。
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
    // Drop 30s UI config cache so refresh / other widgets see new list
    clearDedupCache(`${API_URL}/api/config/ui`)
    window.dispatchEvent(
      new CustomEvent('custom-platforms-update', {
        detail: { platforms, persisted: true },
      }),
    )
  } catch (err) {
    // Roll back in-memory state so UI matches server
    customPlatformsData = previous
    platformInfoCache.clear()
    console.error('Failed to persist custom platforms:', err)
    throw err
  }
}

// 添加自定义平台
async function addCustomPlatform(platform: CustomPlatformData) {
  const updated = [...getCustomPlatformsData(), platform]
  await saveCustomPlatforms(updated)
  return platform.id
}

// 删除自定义平台
async function removeCustomPlatform(platformId: string) {
  const updated = getCustomPlatformsData().filter((p) => p.id !== platformId)
  await saveCustomPlatforms(updated)
}

// 默认图标 - 预先创建避免重复
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

// 将自定义平台数据转换为PlatformInfo - 带缓存
function customPlatformToPlatformInfo(
  custom: CustomPlatformData,
): PlatformInfo {
  // 检查缓存
  const cached = platformInfoCache.get(custom.id)
  const now = Date.now()
  if (cached && now - cached.timestamp < PLATFORM_INFO_CACHE_TTL) {
    return cached.info
  }

  // 创建图标元素 - 支持 react-icons 名称映射和 URL
  let icon: React.ReactNode

  // 优先使用 iconName（从图标库映射）
  if (custom.iconName) {
    const IconComponent = getIconByName(custom.iconName)
    if (IconComponent) {
      icon = <IconComponent className="w-full h-full" aria-hidden="true" />
    }
  }

  // 如果没有找到图标组件，使用外部URL
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

  // 如果没有图标，使用默认图标
  if (!icon) {
    icon = DEFAULT_ICON
  }

  // 预先创建 getUserUrl 函数，避免每次调用创建闭包
  const linkPattern = custom.linkPattern
  const linkType = custom.linkType
  const hasUsername = custom.username && custom.username.trim() !== ''
  const getUserUrl =
    linkType === 'url' && linkPattern
      ? hasUsername
        ? (userId: string) => linkPattern.replace('{username}', userId)
        : () => linkPattern // 无用户名时直接返回URL模式
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

  // 存入缓存
  platformInfoCache.set(custom.id, { info, timestamp: now })

  return info
}

// 获取所有平台（包括预设和自定义）
function getAllPlatforms(): PlatformInfo[] {
  const customPlatforms = getCustomPlatformsData().map(
    customPlatformToPlatformInfo,
  )
  return [...PLATFORMS, ...customPlatforms]
}

// 全局设置弹窗管理器 - 确保只有一个弹窗实例
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

// 缓存平台配置数据
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

// 全局缓存 - 避免重复请求
let cachedPlatformUserIds: PlatformUserIds | null = null
let fetchPromise: Promise<PlatformUserIds> | null = null
let cacheTimestamp = 0
const CACHE_TTL = 5 * 60 * 1000 // 5分钟缓存

async function fetchPlatformUserIds(): Promise<PlatformUserIds> {
  const now = Date.now()

  // 检查缓存是否有效
  if (cachedPlatformUserIds && now - cacheTimestamp < CACHE_TTL) {
    return cachedPlatformUserIds
  }

  // 复用进行中的请求
  if (fetchPromise) return fetchPromise

  fetchPromise = (async () => {
    try {
      // 使用公开端点，不需要登录认证；去重缓存与报告卡片共享同一次请求
      const data = await getPublicConfigDeduped()
      const result: PlatformUserIds = {}

      // 从 platforms 数组提取用户ID配置
      // 只要字段已填即可（与报告页 enabled 开关无关；enabled 仅控制报告卡片展示）
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

// 自定义平台表单组件
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
      <div className="p-4 space-y-4 max-h-96 overflow-y-auto">
        {/* 平台名称 */}
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

        {/* 链接类型 */}
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

        {/* URL类型显示用户名和URL模式 */}
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

        {/* 按钮 */}
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

// 解析弹窗内容 - 自动识别图片URL
// 安全验证图片 URL
function isValidImageUrl(url: string): boolean {
  try {
    const parsed = new URL(url)
    // 只允许 http 和 https 协议
    if (!['http:', 'https:'].includes(parsed.protocol)) {
      return false
    }
    // 检查是否有可疑的路径
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

  // 匹配图片URL的正则表达式
  const imageUrlRegex = /(https?:\/\/\S+\.(?:jpg|jpeg|png|gif|webp|bmp|svg))/gi
  const rawImageUrls = text.match(imageUrlRegex) || []

  // 过滤不安全的图片 URL
  const imageUrls = rawImageUrls.filter(isValidImageUrl)

  // 移除图片URL后的纯文本
  const textContent = text.replace(imageUrlRegex, '').trim()

  return { textContent, imageUrls }
}

// Tooltip 动画配置常量
const TOOLTIP_ANIMATION = {
  initial: { opacity: 0, y: -4, x: '-50%', scale: 0.96 },
  animate: { opacity: 1, y: 0, x: '-50%', scale: 1 },
  exit: { opacity: 0, y: -4, x: '-50%', scale: 0.96 },
  transition: { duration: 0.12, ease: 'easeOut' as const },
}

// 信息提示组件 - hover 时在小组件下方显示，点击复制
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

    // 解析内容
    const { textContent, imageUrls } = useMemo(
      () => parsePopupContent(popupData.text),
      [popupData.text],
    )

    // 计算位置 - 基于小组件中心对齐
    const position = useMemo(() => {
      if (!isVisible || !anchorRef.current) return { top: 0, left: 0 }
      const rect = anchorRef.current.getBoundingClientRect()
      return {
        top: rect.bottom + 6,
        left: rect.left + rect.width / 2,
      }
    }, [isVisible, anchorRef])

    // 复制文本
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
              {/* 文本内容 - 点击可复制 */}
              {textContent && (
                <div className="cursor-pointer text-center">
                  <p className="text-sm font-medium text-gray-800 dark:text-gray-100 wrap-break-word whitespace-pre-wrap leading-relaxed">
                    {textContent}
                  </p>
                </div>
              )}

              {/* 图片 - 紧凑显示 */}
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

              {/* 二维码 - 紧凑显示 */}
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

// 全局设置弹窗组件 - 单例模式
const GlobalSettingsModal = memo(() => {
  // 订阅全局状态
  const [, forceUpdate] = useState({})
  const { t } = useI18n()

  // 加载自定义平台
  const [allPlatforms, setAllPlatforms] = useState<PlatformInfo[]>(() => {
    return getAllPlatforms()
  })

  // 异步加载自定义平台
  useEffect(() => {
    loadCustomPlatformsAsync().then(() => {
      setAllPlatforms(getAllPlatforms())
    })
  }, [])

  // 自定义平台表单状态
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
  const modalRef = useRef<HTMLDivElement>(null)

  // 使用 useMemo 计算位置，避免重复计算
  const position = useMemo(() => {
    if (!anchorRect) return { top: 0, left: 0 }

    const modalWidth = 280
    const modalHeight = 280
    const padding = 16

    let top = anchorRect.bottom + 8
    let left = anchorRect.left + (anchorRect.width - modalWidth) / 2

    if (left + modalWidth > window.innerWidth - padding) {
      left = window.innerWidth - modalWidth - padding
    }
    if (left < padding) left = padding
    if (top + modalHeight > window.innerHeight - padding) {
      top = anchorRect.top - modalHeight - 8
    }
    if (top < padding) top = padding

    return { top, left }
  }, [anchorRect])

  // 优化事件监听器 - 分离 mousedown 和 keydown 处理
  useEffect(() => {
    if (!isOpen) return

    const handleClickOutside = (e: MouseEvent) => {
      if (modalRef.current && !modalRef.current.contains(e.target as Node)) {
        closeSettingsModal()
      }
    }

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        closeSettingsModal()
      }
    }

    // 延迟添加事件监听，避免立即触发
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

  // 处理平台选择
  const handleSelect = useCallback(
    (platformId: string) => {
      if (onSelect) {
        onSelect(platformId)
      }
      closeSettingsModal()
    },
    [onSelect],
  )

  // 处理自定义平台表单提交
  const handleCustomFormSubmit = useCallback(async () => {
    if (!customFormData.name) {
      alert(t.socialNetworkWidget.fillPlatformName)
      return
    }

    // URL类型需要用户名或URL模式其一，弹窗类型需要显示内容
    if (
      customFormData.linkType === 'url' &&
      !customFormData.username &&
      !customFormData.linkPattern
    ) {
      alert(t.socialNetworkWidget.fillUsernameOrUrl)
      return
    }
    if (customFormData.linkType === 'popup' && !customFormData.popupText) {
      alert(t.socialNetworkWidget.fillPopupContent)
      return
    }

    // 安全验证：检查 URL 模式
    const urlPattern = customFormData.linkPattern || ''
    if (
      customFormData.linkType === 'url' &&
      urlPattern &&
      !isValidUrlPattern(urlPattern)
    ) {
      alert(t.socialNetworkWidget.invalidUrlPattern)
      return
    }

    setIsGeneratingIcon(true)

    try {
      // 生成唯一ID
      const customId = `custom_${Date.now()}_${Math.random().toString(36).substring(2, 11)}`

      // 调用AI推荐图标API
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
            urlPattern: data.url_pattern, // AI 生成的 URL 模式
          }
        }
      } catch (error) {
        console.error('Failed to get AI icon recommendation:', error)
        // 继续使用用户选择的颜色
      }

      // 确定最终使用的 URL 模式：优先使用用户填写的，否则使用 AI 推荐的
      const rawLinkPattern =
        customFormData.linkPattern || iconData.urlPattern || ''
      // 安全处理：消毒 URL 模式
      const finalLinkPattern = sanitizeUrlPattern(rawLinkPattern)

      // 再次验证消毒后的 URL（AI 推荐的也需要验证）
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
        name: sanitizePlatformName(customFormData.name), // 消毒平台名称
        username:
          customFormData.linkType === 'url'
            ? sanitizeUsername(customFormData.username)
            : '', // 消毒用户名
        iconType: iconData.iconType || 'react-icons',
        iconLibrary: iconData.iconLibrary,
        iconName: iconData.iconName,
        iconUrl: iconData.iconUrl,
        color: iconData.recommendedColor || '#6366f1', // 使用AI推荐的颜色或默认颜色
        darkColor: iconData.recommendedColor || '#6366f1',
        linkType: customFormData.linkType,
        linkPattern:
          customFormData.linkType === 'url' &&
          isValidUrlPattern(finalLinkPattern)
            ? finalLinkPattern
            : undefined, // 验证后存储
        popupData:
          customFormData.linkType === 'popup'
            ? {
                type: 'text',
                text: sanitizePopupText(customFormData.popupText), // 消毒弹窗文本
              }
            : undefined,
      }

      await addCustomPlatform(newPlatform)

      // 刷新平台列表
      setAllPlatforms(getAllPlatforms())

      // 重置表单
      setCustomFormData({
        name: '',
        username: '',
        linkType: 'url',
        linkPattern: '',
        popupText: '',
      })
      setShowCustomForm(false)

      // 自动选择新建的平台
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

  // 删除自定义平台
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

  // 提前返回，不渲染任何东西
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
        className="absolute glass rounded-2xl shadow-2xl overflow-hidden border border-white/20 dark:border-white/10"
        style={{
          top: position.top,
          left: position.left,
          width: 280,
          pointerEvents: 'auto',
        }}
      >
        {/* 标题栏 */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200/50 dark:border-white/10">
          <span className="font-bold text-sm text-gray-800 dark:text-gray-200">
            {t.socialNetwork.selectPlatform}
          </span>
          <button
            type="button"
            onClick={closeSettingsModal}
            className="w-6 h-6 flex items-center justify-center rounded-full hover:bg-black/5 dark:hover:bg-white/10 transition-colors"
            title={t.socialNetworkWidget.close}
            aria-label={t.socialNetworkWidget.close}
          >
            <FaTimes size={12} className="text-gray-500 dark:text-gray-400" />
          </button>
        </div>

        {/* 平台列表或自定义表单 */}
        {!showCustomForm ? (
          <>
            {/* 平台列表 - 使用优化后的 PlatformButton */}
            <div className="p-3 space-y-1.5 max-h-80 overflow-y-auto">
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

            {/* 添加自定义平台按钮 */}
            <div className="p-3 pt-0">
              <button
                type="button"
                onClick={() => setShowCustomForm(true)}
                className="w-full flex items-center justify-center gap-2 px-3 py-2.5 rounded-xl bg-[var(--color-primary)] hover:opacity-90 text-white font-medium transition-all duration-200 shadow-sm hover:shadow-md active:scale-[0.98]"
              >
                <svg
                  className="w-4 h-4"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={2}
                    d="M12 4v16m8-8H4"
                  />
                </svg>
                {t.socialNetwork.create}
              </button>
            </div>
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
      </motion.div>
    </motion.div>,
    document.body,
  )
})

GlobalSettingsModal.displayName = 'GlobalSettingsModal'

// 平台按钮组件 - 单独 memo 避免列表重渲染
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

// 主组件
export const SocialNetworkWidget = memo(
  ({ config, isEditMode, isPreview, onConfigChange }: WidgetComponentProps) => {
    const { containerRef, fontScale } = useWidgetSize(
      config.size,
      isPreview ? 1 : undefined,
    )
    const anim = useAnimationLevel()
    const { t } = useI18n()

    // 🆕 使用触发式动画 - 组件挂载时播放一次hover动画
    const { isAnimating } = useLoopAnimation({
      duration: 600, // hover动画约0.6秒
      trigger: 'mount', // 固定值，组件首次渲染时触发一次
      enabled: anim.loop, // 低端设备禁用
    })

    // 本地 ref 用于获取 DOM 元素引用（用于定位弹窗）
    const localRef = useRef<HTMLDivElement | null>(null)

    // 深色模式检测 - 使用共享主题订阅器，避免每个组件都创建 MutationObserver
    const isDark = useThemeMode()

    // 从配置获取选中的平台ID
    const [selectedPlatformId, setSelectedPlatformId] = useState<string>(
      config.config?.platformId || 'bilibili',
    )
    const [platformUserIds, setPlatformUserIds] = useState<PlatformUserIds>({})
    const [isHovered, setIsHovered] = useState(false)

    // 信息提示状态 - hover 触发
    const [showInfoTooltip, setShowInfoTooltip] = useState(false)

    // 自定义平台加载状态
    const [customPlatformsReady, setCustomPlatformsReady] = useState(
      customPlatformsLoaded,
    )

    // 长按检测 - 使用 ReturnType<typeof setTimeout> 兼容浏览器和 Node 环境
    const longPressTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
    const isLongPressRef = useRef(false)

    // hover 延迟计时器
    const hoverTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

    // 异步加载自定义平台数据
    useEffect(() => {
      if (!customPlatformsLoaded) {
        loadCustomPlatformsAsync().then(() => {
          setCustomPlatformsReady(true)
        })
      }
    }, [])

    // 获取当前选中的平台信息 - 支持自定义平台
    const selectedPlatform = useMemo(() => {
      // 先检查是否是预设平台
      const index = PLATFORM_INDEX_MAP[selectedPlatformId]
      if (index !== undefined) {
        return PLATFORMS[index]
      }

      // 检查是否是自定义平台（确保数据已加载）
      if (customPlatformsReady) {
        const customPlatform = getCustomPlatformsData().find(
          (p) => p.id === selectedPlatformId,
        )
        if (customPlatform) {
          return customPlatformToPlatformInfo(customPlatform)
        }
      }

      // 默认返回第一个平台
      return PLATFORMS[0]
    }, [selectedPlatformId, customPlatformsReady])

    // 获取用户ID - 支持自定义平台
    const userId = useMemo(() => {
      // 预设平台
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
          // 自定义平台（确保数据已加载）
          if (customPlatformsReady) {
            const customPlatform = getCustomPlatformsData().find(
              (p) => p.id === selectedPlatformId,
            )
            if (customPlatform) {
              // 信息弹窗类型返回特殊标识，表示已配置
              if (customPlatform.linkType === 'popup') {
                return '__popup__'
              }
              // 有用户名则返回用户名，否则如果有linkPattern则返回特殊标识表示直接访问
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

    // 加载平台用户ID
    useEffect(() => {
      if (isPreview) return
      fetchPlatformUserIds().then(setPlatformUserIds)
    }, [isPreview])

    // 同步配置中的 platformId
    useEffect(() => {
      if (
        config.config?.platformId &&
        config.config.platformId !== selectedPlatformId
      ) {
        setSelectedPlatformId(config.config.platformId)
      }
    }, [config.config?.platformId])

    // 处理平台选择变化
    const handleSelectPlatform = useCallback(
      (platformId: string) => {
        setSelectedPlatformId(platformId)
        // 优先通过 props.onConfigChange 上报
        if (typeof onConfigChange === 'function') {
          onConfigChange({ ...config.config, platformId })
        } else {
          // 兼容旧逻辑
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

    // 打开设置弹窗
    const openSettings = useCallback(() => {
      if (!localRef.current) return

      openSettingsModal(
        selectedPlatformId,
        localRef.current.getBoundingClientRect(),
        handleSelectPlatform,
      )
    }, [selectedPlatformId, handleSelectPlatform])

    // 长按开始
    const handlePressStart = useCallback(() => {
      if (!isEditMode) return

      isLongPressRef.current = false
      longPressTimerRef.current = setTimeout(() => {
        isLongPressRef.current = true
        openSettings()
      }, 500)
    }, [isEditMode, openSettings])

    // 长按取消
    const handlePressEnd = useCallback(() => {
      if (longPressTimerRef.current) {
        clearTimeout(longPressTimerRef.current)
        longPressTimerRef.current = null
      }
    }, [])

    // 获取弹窗类型平台数据（合并查找逻辑避免重复）
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

    // 是否为弹窗类型（直接从 popupPlatformData 派生）
    const isPopupType = popupPlatformData !== null
    const hasInteraction = !isEditMode && !!userId

    // 点击处理（跳转到用户页面或复制信息）
    const handleClick = useCallback(async () => {
      // 如果是长按触发的设置弹窗，不处理点击
      if (isLongPressRef.current) {
        isLongPressRef.current = false
        return
      }

      // 编辑模式下不跳转
      if (isEditMode) return

      // 信息弹窗类型：点击复制文本
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

      // 如果有用户ID，则跳转
      if (userId && userId !== '__popup__') {
        // __direct_url__ 表示直接访问URL模式（无用户名占位符）
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

    // 清理定时器
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

    // 计算图标颜色 - 使用 useMemo 避免重复计算
    const iconColor = useMemo(
      () => (isDark ? selectedPlatform.darkColor : selectedPlatform.color),
      [isDark, selectedPlatform.darkColor, selectedPlatform.color],
    )

    // 根据动画级别和调度器状态选择过渡配置
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

    // 使用 useMemo 渲染内容区域，避免不必要的重渲染
    const content = useMemo(() => {
      // 1x1 尺寸 - 仅图标
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

      // 2x1 尺寸 - 图标 + 平台名称
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

      // 2x2 尺寸 - popup 类型重视内容显示
      const popupContent = popupPlatformData
        ? parsePopupContent(popupPlatformData.popupData.text)
        : null
      const is2x2Popup =
        userId === '__popup__' && popupContent && popupPlatformData

      // 2x2 popup 类型：内容为主的布局
      if (is2x2Popup) {
        // 判断是否只有图片内容（没有文本）
        const hasOnlyImages =
          !popupContent.textContent &&
          (popupContent.imageUrls.length > 0 ||
            popupPlatformData.popupData.qrcodeUrl)

        return (
          <div className="h-full w-full relative flex flex-col p-3 cursor-pointer group">
            {/* 右上角：复制提示 - 仅在有文本内容时显示 */}
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

            {/* 顶部：小图标 + 平台名 */}
            <div className="flex items-center gap-2 mb-1">
              <div className="text-lg shrink-0" style={{ color: iconColor }}>
                {selectedPlatform.icon}
              </div>
              <span className="text-xs font-medium text-gray-600 dark:text-gray-400 truncate">
                {selectedPlatform.name}
              </span>
            </div>

            {/* 中间：主要内容区域 - 扩大显示 */}
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

      // 2x2 尺寸 - 普通类型：图标 + 平台名称 + 描述
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

    // 缓存容器的 hover/tap 动画配置 - 条件判断移到外部避免创建空对象
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

    // 背景光晕动画 - 直接使用条件渲染，无需 useMemo
    const shouldAnimateGlow = anim.loop

    // tooltip 隐藏延迟计时器
    const tooltipHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
      null,
    )

    // 缓存 onMouseLeave 和 onMouseEnter 回调
    const handleMouseLeave = useCallback(() => {
      handlePressEnd()
      setIsHovered(false)
      // 清除 hover 延迟计时器
      if (hoverTimerRef.current) {
        clearTimeout(hoverTimerRef.current)
        hoverTimerRef.current = null
      }
      // 延迟隐藏 tooltip，给用户一点时间移动到 tooltip 上
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
      // 对于 popup 类型，hover 延迟显示 tooltip（仅非 2x2 尺寸）
      // 2x2 尺寸直接在小组件内显示内容
      if (
        !isEditMode &&
        isPopupType &&
        popupPlatformData &&
        config.size !== '2x2'
      ) {
        // 清除之前的计时器
        if (hoverTimerRef.current) {
          clearTimeout(hoverTimerRef.current)
        }
        // 300ms 延迟后显示 tooltip
        hoverTimerRef.current = setTimeout(() => {
          setShowInfoTooltip(true)
        }, 300)
      }
    }, [isEditMode, userId, isPopupType, popupPlatformData, config.size])

    // 合并 ref 回调
    const mergedRef = useCallback(
      (node: HTMLDivElement | null) => {
        // 设置本地 ref
        localRef.current = node
        // 调用 containerRef 回调
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
            // 使用 filter 替代 box-shadow 避免布局影响
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
          {/* 内容区域 */}
          {content}
          <WidgetLongPressHint
            visible={isEditMode}
            title={t.socialNetwork.longPressToEdit}
          />
        </WidgetShell>

        {/* 信息提示 - hover 触发，点击复制 */}
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

// 导出全局弹窗组件供应用层级使用
export { GlobalSettingsModal as SocialNetworkSettingsModal }
