import type {
  NotificationEventDefinition,
  NotificationPreferences,
  NotificationSourceKey,
} from '../services/notificationPreferencesApi'
import type { ModuleVisibilityPreferences } from '../utils/moduleVisibility'
import type { OAuthSettings } from '../utils/oauthSettings'
import type { HitokotoConfig } from '../utils/quote'
import type { ReportSettings } from '../utils/reportSettings'
import type {
  LibrarySourcePreferences,
  PlatformAutoFetchConfig,
} from './config'
import type { PermissionConfigValues } from './config/PermissionsConfigSection'
import type { ToastType } from './Toast'

import {
  FaExclamationTriangle,
  FaSearch,
  FaStar,
  FaTimes,
  LuGripVertical,
} from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { API_URL } from '../config'

import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { useDebounce } from '../hooks/useDebounce'
import {
  checkSpeechStatus,
  fetchConfig,
  fetchPermissionsConfig,
  reloadSystemConfig,
  updateConfig,
  updatePermissionsConfig,
} from '../lib/api'
import apiService from '../services/api'
import notificationPreferencesApi, {
  areNotificationPreferencesEqual,
  cloneNotificationPreferences,
  DEFAULT_NOTIFICATION_CATALOG,
  DEFAULT_NOTIFICATION_PREFERENCES,
} from '../services/notificationPreferencesApi'
import { getCSRFToken } from '../utils/csrf'
import {
  areModuleVisibilityPreferencesEqual,
  DEFAULT_MODULE_VISIBILITY_PREFERENCES,
  dispatchModuleVisibilityPreferencesUpdated,
  fetchModuleVisibilityPreferences,
  normalizeModuleVisibilityPreferences,
  updateModuleVisibilityPreferences,
} from '../utils/moduleVisibility'
import {
  areOAuthSettingsEqual,
  cloneOAuthSettings,
  DEFAULT_OAUTH_SETTINGS,
  fetchOAuthSettings,
  updateOAuthSettings,
} from '../utils/oauthSettings'
import {
  areHitokotoConfigsEqual,
  DEFAULT_HITOKOTO_CONFIG,
  fetchHitokotoConfig,
  updateHitokotoConfig,
} from '../utils/quote'
import {
  areReportSettingsEqual,
  DEFAULT_REPORT_SETTINGS,
  fetchReportSettings,
  updateReportSettings,
} from '../utils/reportSettings'
import { clearDedupCache } from '../utils/requestDedup'
import {
  AboutConfigSection,
  AdvancedConfigSection,
  AiConfigSection,
  areLibrarySourcePreferencesEqual,
  DEFAULT_LIBRARY_SOURCE_PREFERENCES,
  ModuleConfigSection,
  MusicConfigSection,
  NetworkConfigSection,
  normalizeLibraryPreferences,
  NotificationConfigSection,
  OAuthConfigSection,
  PermissionsConfigSection,
  PlatformAutoRefreshSettings,
  UiConfigSection,
} from './config'
import MyriadConfigIcon from './config/MyriadConfigIcon'
import PlatformIcon from './PlatformIcon'
import Toast from './Toast'
import './ConfigForm.css'

// 导入迁移后的配置区块组件

interface ConfigField {
  key: string
  label: string
  field_type: string
  value: string
  placeholder: string
  required: boolean
}

interface PlatformConfig {
  name: string
  enabled: boolean
  has_token: boolean
  config_fields: ConfigField[]
  description: string
  icon: string
}

interface AiConfig {
  provider: string
  model: string
  api_key: string
  enabled: boolean
  // AI 图片生成配置
  image_provider: string
  config_fields: ConfigField[]
}

interface ReportConfig {
  topic_style: string
  config_fields: ConfigField[]
}

interface UiConfig {
  wallpaper_url: string
  wallpaper_blur: number
  theme: string
  primary_color: string
  secondary_color: string
  config_fields: ConfigField[]
}

interface Config {
  platforms: PlatformConfig[]
  auto_fetch: PlatformAutoFetchConfig
  ai_config: AiConfig
  report_config: ReportConfig
  ui_config: UiConfig
}

interface QuickAccessItem {
  id: string
  label: string
  icon: React.ReactNode
  section: string
  subsection?: string
}

interface SaveLibrarySourcePreferencesResponse {
  success: boolean
  preferences?: LibrarySourcePreferences
  message?: string
}

const DEFAULT_PERMISSION_CONFIG: PermissionConfigValues = {
  user_perm_ai_generate: false,
  user_perm_ai_analyze: false,
  user_perm_ai_chat: false,
  user_perm_report_write: false,
  user_perm_network_fetch: false,
  user_perm_media_control: false,
  user_perm_component_theme: false,
  user_perm_shortcut_register: false,
  user_perm_event_publish: false,
  user_perm_ai_image: false,
  user_perm_scheduler_register: false,
  user_perm_speech_tts: false,
  user_perm_speech_asr: false,
  guest_perm_ai_generate: false,
  guest_perm_ai_analyze: false,
  guest_perm_ai_chat: false,
  guest_perm_report_write: false,
  guest_perm_network_fetch: false,
  guest_perm_media_control: false,
  guest_perm_component_theme: false,
  guest_perm_shortcut_register: false,
  guest_perm_event_publish: false,
  guest_perm_ai_image: false,
  guest_perm_scheduler_register: false,
  guest_perm_speech_tts: false,
  guest_perm_speech_asr: false,
  user_ai_daily_calls: 50,
  user_ai_daily_tokens: 20000,
  user_ai_cooldown_seconds: 5,
  guest_ai_daily_calls: 10,
  guest_ai_daily_tokens: 5000,
  guest_ai_cooldown_seconds: 10,
}

const DEFAULT_CONFIG_FAVORITES = ['platforms', 'ai']
const DEFAULT_AUTO_FETCH_CONFIG: PlatformAutoFetchConfig = {
  enabled: false,
  interval_hours: 24,
}

function loadConfigFavorites(): string[] {
  if (typeof window === 'undefined') return DEFAULT_CONFIG_FAVORITES
  try {
    const saved = localStorage.getItem('config_favorites')
    if (!saved) return DEFAULT_CONFIG_FAVORITES
    const parsed: unknown = JSON.parse(saved)
    return Array.isArray(parsed) &&
      parsed.every((item) => typeof item === 'string')
      ? parsed
      : DEFAULT_CONFIG_FAVORITES
  } catch {
    return DEFAULT_CONFIG_FAVORITES
  }
}

function isMaskedValue(value: string) {
  return value.includes('••') || value.includes('**') || value === '********'
}

function hasFieldValue(field?: ConfigField) {
  return Boolean(field && String(field.value).trim().length > 0)
}

function isBangumiPlatform(platform: PlatformConfig) {
  return platform.name.toLowerCase() === 'bangumi'
}

function hasBangumiCredential(platform: PlatformConfig) {
  const username = platform.config_fields.find(
    (field) => field.key === 'username',
  )
  const accessToken = platform.config_fields.find(
    (field) => field.key === 'access_token',
  )

  return hasFieldValue(username) || hasFieldValue(accessToken)
}

// 优化：提取为独立的 memo 组件避免不必要的重渲染
interface QuickAccessCardProps {
  item: QuickAccessItem
  isActive: boolean
  isFavorite: boolean
  onCardClick: (section: string) => void
  onToggleFavorite: (id: string) => void
}

const QuickAccessCard = React.memo<QuickAccessCardProps>(
  ({ item, isActive, isFavorite, onCardClick, onToggleFavorite }) => {
    const handleCardClick = React.useCallback(() => {
      onCardClick(item.section)
    }, [onCardClick, item.section])

    const handleFavoriteClick = React.useCallback(
      (e: React.MouseEvent) => {
        e.stopPropagation()
        onToggleFavorite(item.id)
      },
      [onToggleFavorite, item.id],
    )

    return (
      <div
        onClick={handleCardClick}
        className={`quick-access-card ${isActive ? 'active' : ''}`}
      >
        <span className="card-icon">{item.icon}</span>
        <span className="card-label">{item.label}</span>
        <button
          onClick={handleFavoriteClick}
          className={`favorite-btn ${isFavorite ? 'active' : ''}`}
          aria-label={isFavorite ? 'Remove from favorites' : 'Add to favorites'}
        >
          <FaStar />
        </button>
      </div>
    )
  },
)

QuickAccessCard.displayName = 'QuickAccessCard'

const ModernConfigForm: React.FC = () => {
  const navigate = useNavigate()
  const { t } = useI18n()
  const { user } = useAuth()
  const [config, setConfig] = useState<Config | null>(null)
  const [initialConfig, setInitialConfig] = useState<Config | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState('')
  const [messageType, setMessageType] = useState<ToastType>('info')
  const [activeSection, setActiveSection] = useState<string>('platforms')
  const [platformModalOpen, setPlatformModalOpen] = useState<string | null>(
    null,
  )
  // 🆕 平台拖拽排序状态：仅在按住拖拽手柄时才允许拖动
  const [dragIndex, setDragIndex] = useState<number | null>(null)
  const [dragOverIndex, setDragOverIndex] = useState<number | null>(null)
  const [dragArmedIndex, setDragArmedIndex] = useState<number | null>(null)
  const [searchQuery, setSearchQuery] = useState('')
  const [favorites, setFavorites] = useState<string[]>(loadConfigFavorites)
  const [savedFavorites, setSavedFavorites] =
    useState<string[]>(loadConfigFavorites)

  // Tapp 权限下放配置状态（13 项 elevated 权限 × 2 角色 + AI 限额配置）
  const [permissionConfig, setPermissionConfig] =
    useState<PermissionConfigValues>(DEFAULT_PERMISSION_CONFIG)
  const [savedPermissionConfig, setSavedPermissionConfig] =
    useState<PermissionConfigValues>(DEFAULT_PERMISSION_CONFIG)
  const [permissionLoading, setPermissionLoading] = useState(false)
  const [notificationDraft, setNotificationDraft] =
    useState<NotificationPreferences>(DEFAULT_NOTIFICATION_PREFERENCES)
  const [savedNotificationPreferences, setSavedNotificationPreferences] =
    useState<NotificationPreferences>(DEFAULT_NOTIFICATION_PREFERENCES)
  const [notificationSources, setNotificationSources] = useState<
    NotificationSourceKey[]
  >(DEFAULT_NOTIFICATION_CATALOG.sources)
  const [notificationEvents, setNotificationEvents] = useState<
    NotificationEventDefinition[]
  >(DEFAULT_NOTIFICATION_CATALOG.events)
  const [notificationLoading, setNotificationLoading] = useState(false)
  const [oauthDraft, setOAuthDraft] = useState<OAuthSettings>(
    DEFAULT_OAUTH_SETTINGS,
  )
  const [savedOAuthSettings, setSavedOAuthSettings] = useState<OAuthSettings>(
    DEFAULT_OAUTH_SETTINGS,
  )
  const [oauthLoading, setOAuthLoading] = useState(false)
  const [librarySourceDraft, setLibrarySourceDraft] =
    useState<LibrarySourcePreferences>(DEFAULT_LIBRARY_SOURCE_PREFERENCES)
  const [savedLibrarySourcePreferences, setSavedLibrarySourcePreferences] =
    useState<LibrarySourcePreferences>(DEFAULT_LIBRARY_SOURCE_PREFERENCES)
  const [librarySourceSaveRevision, setLibrarySourceSaveRevision] = useState(0)
  const [moduleVisibilityDraft, setModuleVisibilityDraft] =
    useState<ModuleVisibilityPreferences>(DEFAULT_MODULE_VISIBILITY_PREFERENCES)
  const [
    savedModuleVisibilityPreferences,
    setSavedModuleVisibilityPreferences,
  ] = useState<ModuleVisibilityPreferences>(
    DEFAULT_MODULE_VISIBILITY_PREFERENCES,
  )
  const [hitokotoDraft, setHitokotoDraft] = useState<HitokotoConfig>(
    DEFAULT_HITOKOTO_CONFIG,
  )
  const [savedHitokotoConfig, setSavedHitokotoConfig] =
    useState<HitokotoConfig>(DEFAULT_HITOKOTO_CONFIG)
  const [reportSettingsDraft, setReportSettingsDraft] =
    useState<ReportSettings>(DEFAULT_REPORT_SETTINGS)
  const [savedReportSettings, setSavedReportSettings] =
    useState<ReportSettings>(DEFAULT_REPORT_SETTINGS)

  const showMessage = useCallback(
    (nextMessage: string, nextType: ToastType = 'info', duration = 3000) => {
      setMessageType(nextType)
      setMessage(nextMessage)
      if (duration > 0) {
        window.setTimeout(setMessage, duration, '')
      }
    },
    [],
  )

  // 所有设置项只更新草稿；实际写入统一由 handleSave 完成。
  const updatePermissionConfig = useCallback(
    (
      keyOrPatch: string | Record<string, boolean | number>,
      value?: boolean | number,
    ) => {
      const patch: Record<string, boolean | number> =
        typeof keyOrPatch === 'string'
          ? { [keyOrPatch]: value as boolean | number }
          : keyOrPatch

      setPermissionConfig((prev) => ({ ...prev, ...patch }))
    },
    [],
  )

  // 加载权限配置
  const loadPermissionConfig = useCallback(async () => {
    try {
      setPermissionLoading(true)
      const response = await fetchPermissionsConfig()

      if (response.success && response.config) {
        const { guest, user, user_ai_quota, guest_ai_quota } = response.config
        const loaded: PermissionConfigValues = {
          // 普通用户权限
          user_perm_ai_generate: user.ai_generate,
          user_perm_ai_analyze: user.ai_analyze,
          user_perm_ai_chat: user.ai_chat,
          user_perm_report_write: user.report_write,
          user_perm_network_fetch: user.network_fetch,
          user_perm_media_control: user.media_control,
          user_perm_component_theme: user.component_theme,
          user_perm_shortcut_register: user.shortcut_register,
          user_perm_event_publish: user.event_publish,
          user_perm_ai_image: user.ai_image,
          user_perm_scheduler_register: user.scheduler_register,
          user_perm_speech_tts: user.speech_tts,
          user_perm_speech_asr: user.speech_asr,
          // 游客权限
          guest_perm_ai_generate: guest.ai_generate,
          guest_perm_ai_analyze: guest.ai_analyze,
          guest_perm_ai_chat: guest.ai_chat,
          guest_perm_report_write: guest.report_write,
          guest_perm_network_fetch: guest.network_fetch,
          guest_perm_media_control: guest.media_control,
          guest_perm_component_theme: guest.component_theme,
          guest_perm_shortcut_register: guest.shortcut_register,
          guest_perm_event_publish: guest.event_publish,
          guest_perm_ai_image: guest.ai_image,
          guest_perm_scheduler_register: guest.scheduler_register,
          guest_perm_speech_tts: guest.speech_tts,
          guest_perm_speech_asr: guest.speech_asr,
          // AI 使用限额配置
          user_ai_daily_calls: user_ai_quota?.daily_calls ?? 50,
          user_ai_daily_tokens: user_ai_quota?.daily_tokens ?? 20000,
          user_ai_cooldown_seconds: user_ai_quota?.cooldown_seconds ?? 5,
          guest_ai_daily_calls: guest_ai_quota?.daily_calls ?? 10,
          guest_ai_daily_tokens: guest_ai_quota?.daily_tokens ?? 5000,
          guest_ai_cooldown_seconds: guest_ai_quota?.cooldown_seconds ?? 10,
        }
        setPermissionConfig(loaded)
        setSavedPermissionConfig(loaded)
      }
    } catch (error) {
      console.error('Failed to load permissions:', error)
    } finally {
      setPermissionLoading(false)
    }
  }, [])

  const loadNotificationSettings = useCallback(async () => {
    try {
      setNotificationLoading(true)
      const response = await notificationPreferencesApi.get()
      const loaded = cloneNotificationPreferences(response.preferences)
      setNotificationDraft(loaded)
      setSavedNotificationPreferences(cloneNotificationPreferences(loaded))
      setNotificationSources(response.catalog.sources)
      setNotificationEvents(response.catalog.events)
    } catch (error) {
      console.error('Failed to load notification settings:', error)
      showMessage(t.config.loadConfigFailed, 'error')
    } finally {
      setNotificationLoading(false)
    }
  }, [showMessage, t])

  const loadOAuthSettings = useCallback(async () => {
    try {
      setOAuthLoading(true)
      const loaded = await fetchOAuthSettings()
      setOAuthDraft(cloneOAuthSettings(loaded))
      setSavedOAuthSettings(cloneOAuthSettings(loaded))
    } catch (error) {
      console.error('Failed to load OAuth settings:', error)
      showMessage(t.config.loadConfigFailed, 'error')
    } finally {
      setOAuthLoading(false)
    }
  }, [showMessage, t])

  const getPlatformDescription = useCallback(
    (platform: PlatformConfig) => {
      const descMap: Record<string, string> = {
        github: t.config.platformDescGithub,
        bilibili: t.config.platformDescBilibili,
        bangumi: t.config.platformDescBangumi,
        steam: t.config.platformDescSteam,
        'netease music': t.config.platformDescNetease,
        x: t.config.platformDescX,
        discord: t.config.platformDescDiscord,
        myanimelist: t.config.platformDescMal,
      }
      return descMap[platform.name.toLowerCase()] || platform.description
    },
    [t],
  )

  const isPlatformConfigured = useCallback((platform: PlatformConfig) => {
    if (!platform.config_fields || platform.config_fields.length === 0)
      return true

    if (isBangumiPlatform(platform)) {
      return hasBangumiCredential(platform)
    }

    // Discord 一键授权后 has_token=true；掩码字段也算已配置
    if (platform.name.toLowerCase() === 'discord') {
      if (platform.has_token) return true
    }

    return platform.config_fields.every((field) => {
      if (!field.required) return true
      return field.value && String(field.value).trim().length > 0
    })
  }, [])

  const connectDiscordOAuth = useCallback(() => {
    // 与登录/绑定一致：浏览器导航，携带 auth_token cookie
    window.location.href = `${API_URL}/api/platforms/discord/oauth/start`
  }, [])

  // 获取翻译后的字段标签（覆盖后端返回的标签）
  const getFieldLabel = useCallback(
    (fieldKey: string, originalLabel: string): string => {
      const fieldLabels: Record<string, string> = {
        wallpaper_url: t.config.fieldWallpaperUrl,
        wallpaper_blur: t.config.fieldWallpaperBlur,
        wallpaper_parallax: t.config.fieldWallpaperParallax,
        pet_enabled: t.config.fieldPetEnabled,
        pet_image_url: t.config.fieldPetImageUrl,
        site_title: t.config.fieldSiteTitle,
        site_description: t.config.fieldSiteDescription,
        site_favicon: t.config.fieldSiteFavicon,
        music_enabled: t.config.fieldMusicEnabled,
        music_source: t.config.fieldMusicSource,
        music_playlist_id: t.config.fieldMusicPlaylistId,
      }
      return fieldLabels[fieldKey] || originalLabel
    },
    [t],
  )

  // 获取翻译后的占位符
  const getFieldPlaceholder = useCallback(
    (fieldKey: string, originalPlaceholder: string): string => {
      const placeholders: Record<string, string> = {
        wallpaper_url: t.config.placeholderWallpaperUrl,
        site_title: t.config.placeholderSiteTitle,
        site_description: t.config.placeholderSiteDescription,
        site_favicon: t.config.placeholderSiteFavicon,
        pet_image_url: t.config.placeholderPetImageUrl,
      }
      return placeholders[fieldKey] || originalPlaceholder
    },
    [t],
  )

  const getPlatformFieldLabel = useCallback(
    (platform: PlatformConfig, field: ConfigField): string => {
      if (!isBangumiPlatform(platform)) return field.label

      const labels: Record<string, string> = {
        username: t.config.bangumiUsernameLabel,
        access_token: t.config.bangumiAccessTokenLabel,
        user_agent: t.config.bangumiUserAgentLabel,
      }
      return labels[field.key] || field.label
    },
    [t],
  )

  const getPlatformFieldPlaceholder = useCallback(
    (platform: PlatformConfig, field: ConfigField): string => {
      if (!isBangumiPlatform(platform)) return field.placeholder

      const placeholders: Record<string, string> = {
        username: t.config.bangumiUsernamePlaceholder,
        access_token: t.config.bangumiAccessTokenPlaceholder,
        user_agent: t.config.bangumiUserAgentPlaceholder,
      }
      return placeholders[field.key] || field.placeholder
    },
    [t],
  )

  // 使用防抖优化搜索性能 - 避免频繁搜索
  const debouncedSearchQuery = useDebounce(searchQuery, 300)

  // 快速访问项（使用 useMemo 避免每次渲染重新创建数组）
  const quickAccessItems: QuickAccessItem[] = useMemo(
    () => [
      {
        id: 'platforms',
        label: t.config.platforms,
        icon: <MyriadConfigIcon kind="platforms" />,
        section: 'platforms',
      },
      {
        id: 'data',
        label: t.config.data,
        icon: <MyriadConfigIcon kind="data" />,
        section: 'data',
      },
      {
        id: 'ai',
        label: t.config.ai,
        icon: <MyriadConfigIcon kind="ai" />,
        section: 'ai',
      },
      {
        id: 'ui',
        label: t.config.basic,
        icon: <MyriadConfigIcon kind="ui" />,
        section: 'ui',
      },
      {
        id: 'music',
        label: t.config.music,
        icon: <MyriadConfigIcon kind="music" />,
        section: 'music',
      },
      {
        id: 'oauth',
        label: t.config.oauth,
        icon: <MyriadConfigIcon kind="oauth" />,
        section: 'oauth',
      },
      {
        id: 'network',
        label: t.config.network,
        icon: <MyriadConfigIcon kind="network" />,
        section: 'network',
      },
      {
        id: 'permissions',
        label: t.config.permissions,
        icon: <MyriadConfigIcon kind="permissions" />,
        section: 'permissions',
      },
      {
        id: 'notifications',
        label: t.notificationCenter.title,
        icon: <MyriadConfigIcon kind="notifications" />,
        section: 'notifications',
      },
      {
        id: 'modules',
        label: t.config.moduleSettings,
        icon: <MyriadConfigIcon kind="modules" />,
        section: 'modules',
      },
      {
        id: 'advanced',
        label: t.config.advanced,
        icon: <MyriadConfigIcon kind="advanced" />,
        section: 'advanced',
      },
      {
        id: 'about',
        label: t.config.about,
        icon: <MyriadConfigIcon kind="about" />,
        section: 'about',
      },
    ],
    [t],
  )

  // 搜索功能
  const searchableContent = useMemo(() => {
    if (!config) return []

    const items: Array<{
      type: string
      section: string
      title: string
      description: string
      keywords: string[]
    }> = []

    // 平台配置 - 区块描述
    items.push({
      type: 'section',
      section: 'platforms',
      title: t.config.platforms,
      description: t.config.platformsDesc,
      keywords: [
        '平台',
        '数据源',
        'token',
        'api',
        'github',
        'bilibili',
        'bangumi',
        'steam',
        'netease',
        'myanimelist',
        'mal',
      ],
    })

    // 平台配置 - 各平台
    config.platforms.forEach((platform) => {
      items.push({
        type: 'platform',
        section: 'platforms',
        title: platform.name,
        description: platform.description,
        keywords: [
          platform.name.toLowerCase(),
          '平台',
          '数据源',
          'token',
          'api',
        ],
      })
    })

    // AI配置
    items.push({
      type: 'section',
      section: 'ai',
      title: t.config.ai,
      description: t.config.aiDesc,
      keywords: [
        'ai',
        'gemini',
        'openai',
        'api',
        '模型',
        '智能',
        '图片',
        '生成',
        'image',
      ],
    })

    // UI配置
    items.push({
      type: 'section',
      section: 'ui',
      title: t.config.basic,
      description: t.config.basicDesc,
      keywords: [
        'basic',
        '基础',
        '站点',
        '主题',
        '背景',
        '样式',
        'theme',
        'url',
      ],
    })

    // OAuth配置
    items.push({
      type: 'section',
      section: 'oauth',
      title: t.config.oauth,
      description: t.config.oauthDesc,
      keywords: ['oauth', 'github', '登录', 'auth', '认证'],
    })

    // 音乐播放器
    items.push({
      type: 'section',
      section: 'music',
      title: t.config.music,
      description: t.config.musicDesc,
      keywords: ['音乐', 'music', '歌单', '播放器', '网易云', 'qq音乐'],
    })

    // 网络代理
    items.push({
      type: 'section',
      section: 'network',
      title: t.config.network || '网络代理',
      description: t.config.networkDesc || '配置网络代理以访问外部服务',
      keywords: [
        'proxy',
        '代理',
        '网络',
        'gemini',
        'github',
        'api',
        '镜像',
        'mirror',
        'socks',
      ],
    })

    // 高级配置
    items.push({
      type: 'section',
      section: 'notifications',
      title: t.notificationCenter.title,
      description: t.notificationCenter.settingsDesc,
      keywords: [
        'notification',
        '通知',
        '提醒',
        'toast',
        'browser',
        'arael',
        'brew',
        'tapp',
        'mcp',
        'aro',
      ],
    })

    // 高级配置
    items.push({
      type: 'section',
      section: 'advanced',
      title: t.config.advanced,
      description: t.config.advancedDesc,
      keywords: ['advanced', '高级', 'danger', 'reset', '重置', '危险'],
    })

    // 关于（含 updater 管理内联面板）
    items.push({
      type: 'section',
      section: 'about',
      title: t.config.about,
      description: t.config.aboutDesc,
      keywords: [
        'about',
        '关于',
        '版本',
        'version',
        'logo',
        'myriad',
        // updater 关键字也指向 about section（updater 已合并进关于页）
        'updater',
        '更新',
        'update',
        'upgrade',
        '升级',
        '回滚',
        'rollback',
        'snapshot',
        '快照',
      ],
    })

    // 权限管理
    items.push({
      type: 'section',
      section: 'permissions',
      title: t.config.permissions,
      description: t.config.permissionsDesc,
      keywords: [
        '权限',
        'permission',
        'elevated',
        '下放',
        '配额',
        'quota',
        'ai',
        '游客',
        'guest',
      ],
    })

    // 模块设置
    items.push({
      type: 'section',
      section: 'modules',
      title: t.config.moduleSettings,
      description: t.config.moduleSettingsDesc,
      keywords: [
        '模块',
        'module',
        '资料库',
        'library',
        '来源',
        'source',
        '平台',
        '分类',
        '可见性',
        'visibility',
        '登录用户',
        '管理员',
        '一言',
        'hitokoto',
        'quote',
      ],
    })

    return items
  }, [config, t])

  // 使用防抖后的搜索查询优化性能
  const filteredContent = useMemo(() => {
    if (!debouncedSearchQuery.trim()) return searchableContent

    const query = debouncedSearchQuery.toLowerCase()
    return searchableContent.filter(
      (item) =>
        item.title.toLowerCase().includes(query) ||
        item.description.toLowerCase().includes(query) ||
        item.keywords.some((k) => k.includes(query)),
    )
  }, [debouncedSearchQuery, searchableContent])

  // 切换收藏
  const toggleFavorite = React.useCallback((section: string) => {
    setFavorites((prev) => {
      return prev.includes(section)
        ? prev.filter((s) => s !== section)
        : [...prev, section]
    })
  }, [])

  // 处理节切换
  const handleSectionChange = React.useCallback(
    (section: string) => {
      // 如果是数据管理，直接跳转到专门页面
      if (section === 'data') {
        navigate('/data-management')
        return
      }
      setActiveSection(section)
      setSearchQuery('')
    },
    [navigate],
  )

  const notifyDirtyState = React.useCallback((dirty: boolean) => {
    window.dispatchEvent(
      new CustomEvent('config-dirty-state', {
        detail: { dirty },
      }),
    )
  }, [])

  const handleLibrarySourcePreferencesLoaded = React.useCallback(
    (
      preferences: LibrarySourcePreferences,
      options: { resetDraft?: boolean } = {},
    ) => {
      const normalized = normalizeLibraryPreferences(preferences)
      setSavedLibrarySourcePreferences(normalized)
      if (options.resetDraft) {
        setLibrarySourceDraft(normalized)
      }
    },
    [],
  )

  const saveLibrarySourcePreferences = React.useCallback(async () => {
    const saved = await apiService.put<SaveLibrarySourcePreferencesResponse>(
      '/library/preferences',
      librarySourceDraft,
    )
    if (!saved.success) {
      throw new Error(saved.message || t.config.librarySourceSaveFailed)
    }

    const preferences = normalizeLibraryPreferences(saved.preferences)
    setLibrarySourceDraft(preferences)
    setSavedLibrarySourcePreferences(preferences)
    setLibrarySourceSaveRevision((revision) => revision + 1)
    clearDedupCache(`${API_URL}/api/library`)
  }, [librarySourceDraft, t])

  const loadModuleVisibilityPreferences = React.useCallback(async () => {
    try {
      const preferences = await fetchModuleVisibilityPreferences()
      setSavedModuleVisibilityPreferences(preferences)
      setModuleVisibilityDraft(preferences)
    } catch {
      showMessage(t.config.moduleVisibilityLoadFailed, 'error')
    }
  }, [showMessage, t])

  const saveModuleVisibilityPreferences = React.useCallback(async () => {
    const saved = await updateModuleVisibilityPreferences(moduleVisibilityDraft)
    const preferences = normalizeModuleVisibilityPreferences(saved)
    setModuleVisibilityDraft(preferences)
    setSavedModuleVisibilityPreferences(preferences)
    dispatchModuleVisibilityPreferencesUpdated(preferences)
  }, [moduleVisibilityDraft])

  const handleModuleMessage = React.useCallback(
    (msg: string, type: ToastType = 'info') => showMessage(msg, type),
    [showMessage],
  )

  const loadHitokotoSettings = React.useCallback(async () => {
    try {
      const config = await fetchHitokotoConfig()
      setSavedHitokotoConfig(config)
      setHitokotoDraft(config)
    } catch {
      showMessage(t.config.hitokotoLoadFailed, 'error')
    }
  }, [showMessage, t])

  const saveHitokotoDraft = React.useCallback(async () => {
    const saved = await updateHitokotoConfig(hitokotoDraft)
    setHitokotoDraft(saved)
    setSavedHitokotoConfig(saved)
  }, [hitokotoDraft])

  const loadReportSettings = React.useCallback(async () => {
    try {
      const settings = await fetchReportSettings()
      setSavedReportSettings(settings)
      setReportSettingsDraft(settings)
    } catch {
      showMessage(t.config.reportSettingsLoadFailed, 'error')
    }
  }, [showMessage, t])

  const saveReportSettingsDraft = React.useCallback(async () => {
    const saved = await updateReportSettings(reportSettingsDraft)
    setReportSettingsDraft(saved)
    setSavedReportSettings(saved)
  }, [reportSettingsDraft])

  const handleSave = React.useCallback(async () => {
    if (!config) {
      window.dispatchEvent(
        new CustomEvent('config-save-result', {
          detail: { success: false, message: t.config.configEmpty },
        }),
      )
      return
    }

    const hasConfigChanges =
      Boolean(initialConfig) &&
      JSON.stringify(config) !== JSON.stringify(initialConfig)
    const hasLibrarySourceChanges = !areLibrarySourcePreferencesEqual(
      librarySourceDraft,
      savedLibrarySourcePreferences,
    )
    const hasModuleVisibilityChanges = !areModuleVisibilityPreferencesEqual(
      moduleVisibilityDraft,
      savedModuleVisibilityPreferences,
    )
    const hasHitokotoChanges = !areHitokotoConfigsEqual(
      hitokotoDraft,
      savedHitokotoConfig,
    )
    const hasReportSettingsChanges = !areReportSettingsEqual(
      reportSettingsDraft,
      savedReportSettings,
    )
    const hasPermissionChanges =
      JSON.stringify(permissionConfig) !== JSON.stringify(savedPermissionConfig)
    const hasNotificationChanges = !areNotificationPreferencesEqual(
      notificationDraft,
      savedNotificationPreferences,
    )
    const hasOAuthChanges = !areOAuthSettingsEqual(
      oauthDraft,
      savedOAuthSettings,
    )
    const hasFavoriteChanges =
      JSON.stringify(favorites) !== JSON.stringify(savedFavorites)

    if (
      !hasConfigChanges &&
      !hasLibrarySourceChanges &&
      !hasModuleVisibilityChanges &&
      !hasHitokotoChanges &&
      !hasReportSettingsChanges &&
      !hasPermissionChanges &&
      !hasNotificationChanges &&
      !hasOAuthChanges &&
      !hasFavoriteChanges
    ) {
      notifyDirtyState(false)
      window.dispatchEvent(
        new CustomEvent('config-save-result', {
          detail: { success: true, message: t.config.configSaved },
        }),
      )
      return
    }

    const invalidBangumi = hasConfigChanges
      ? config.platforms.find(
          (platform) =>
            platform.enabled &&
            isBangumiPlatform(platform) &&
            !hasBangumiCredential(platform),
        )
      : undefined
    if (invalidBangumi) {
      showMessage(t.config.bangumiCredentialMissing, 'error', 0)
      window.dispatchEvent(
        new CustomEvent('config-save-result', {
          detail: {
            success: false,
            message: t.config.bangumiCredentialMissing,
          },
        }),
      )
      return
    }

    showMessage(t.config.savingConfig, 'info', 0)
    setSaving(true)

    try {
      let resultMessage = t.config.configSaved

      if (hasConfigChanges) {
        // 获取 CSRF Token（stale sessionStorage / backend restart）
        await getCSRFToken(true)

        // updateConfig throws on HTTP >= 400 or success !== true (incl. CSRF after retry)
        const result = await updateConfig(config)
        if (result?.success === false) {
          throw new Error(result.message || t.config.configSaveFailed)
        }
        resultMessage = result.message || t.config.configSaved
        // Only mark draft clean after a confirmed successful write
        setInitialConfig(JSON.parse(JSON.stringify(config)))
      }

      if (hasLibrarySourceChanges) {
        await saveLibrarySourcePreferences()
        resultMessage = hasConfigChanges
          ? resultMessage
          : t.config.librarySourceSaved
      }

      if (hasModuleVisibilityChanges) {
        await saveModuleVisibilityPreferences()
        resultMessage = hasConfigChanges
          ? resultMessage
          : t.config.moduleVisibilitySaved
      }

      if (hasHitokotoChanges) {
        await saveHitokotoDraft()
        resultMessage = hasConfigChanges
          ? resultMessage
          : t.config.hitokotoSaved
      }

      if (hasReportSettingsChanges) {
        await saveReportSettingsDraft()
        resultMessage = hasConfigChanges
          ? resultMessage
          : t.config.reportSettingsSaved
      }

      if (hasPermissionChanges) {
        await getCSRFToken(true)
        const patch = Object.fromEntries(
          Object.entries(permissionConfig).filter(
            ([key, value]) => savedPermissionConfig[key] !== value,
          ),
        )
        const response = await updatePermissionsConfig(patch)
        if (!response.success) {
          throw new Error(response.message || t.config.permissionsSaveFailed)
        }
        setSavedPermissionConfig({ ...permissionConfig })
        const { TappRuntime } = await import('../tapp/runtime/TappRuntime')
        await TappRuntime.getInstance().refreshPermissionGrants()
        resultMessage = hasConfigChanges
          ? resultMessage
          : t.config.permissionsSaved
      }

      if (hasNotificationChanges) {
        const saved = await notificationPreferencesApi.update(
          notificationDraft,
          user?.id,
        )
        const normalized = cloneNotificationPreferences(saved)
        setNotificationDraft(normalized)
        setSavedNotificationPreferences(
          cloneNotificationPreferences(normalized),
        )
      }

      if (hasOAuthChanges) {
        const saved = await updateOAuthSettings(oauthDraft)
        setOAuthDraft(cloneOAuthSettings(saved))
        setSavedOAuthSettings(cloneOAuthSettings(saved))
      }

      if (hasFavoriteChanges) {
        localStorage.setItem('config_favorites', JSON.stringify(favorites))
        setSavedFavorites([...favorites])
      }

      notifyDirtyState(false)
      window.dispatchEvent(
        new CustomEvent('config-save-result', {
          detail: {
            success: true,
            message: resultMessage,
          },
        }),
      )

      if (!hasConfigChanges) {
        showMessage(resultMessage, 'success', 3000)
        return
      }

      showMessage(
        `${t.config.configSaved} ${t.config.refreshing}`,
        'success',
        0,
      )

      try {
        // reload-config 也需要 CSRF Token
        await getCSRFToken(true)
        await reloadSystemConfig()

        showMessage(t.config.savedSuccess, 'success', 0)

        // 等待后端完成配置保存和环境变量重新加载，然后刷新页面
        setTimeout(() => {
          window.location.reload()
        }, 2000)
      } catch (_restartError) {
        showMessage(t.config.savedSuccess, 'success', 0)
        // 即使刷新配置失败，仍然刷新页面以应用数据库中的新配置
        setTimeout(() => {
          window.location.reload()
        }, 2000)
      }
    } catch (error) {
      const errorMsg = `${t.config.configSaveFailed}: ${error instanceof Error ? error.message : t.errors.networkError}`
      showMessage(errorMsg, 'error', 0)
      window.dispatchEvent(
        new CustomEvent('config-save-result', {
          detail: { success: false, message: errorMsg },
        }),
      )
    } finally {
      setSaving(false)
    }
  }, [
    config,
    initialConfig,
    librarySourceDraft,
    moduleVisibilityDraft,
    hitokotoDraft,
    notifyDirtyState,
    saveLibrarySourcePreferences,
    saveModuleVisibilityPreferences,
    saveHitokotoDraft,
    reportSettingsDraft,
    savedReportSettings,
    saveReportSettingsDraft,
    permissionConfig,
    savedPermissionConfig,
    notificationDraft,
    savedNotificationPreferences,
    oauthDraft,
    savedOAuthSettings,
    favorites,
    savedFavorites,
    user?.id,
    savedLibrarySourcePreferences,
    savedModuleVisibilityPreferences,
    savedHitokotoConfig,
    showMessage,
    t,
  ])

  const handleReset = React.useCallback(async () => {
    showMessage(t.config.resettingConfig, 'info', 0)
    setSaving(true)

    try {
      const data = await fetchConfig()

      const clearedData = {
        ...data,
        auto_fetch: DEFAULT_AUTO_FETCH_CONFIG,
        platforms: data.platforms.map((platform: any) => ({
          ...platform,
          enabled: false,
          has_token: false,
          config_fields: platform.config_fields.map((field: any) => ({
            ...field,
            value: '',
          })),
        })),
        ai_config: {
          ...data.ai_config,
          enabled: false,
          api_key: '',
          config_fields: data.ai_config.config_fields.map((field: any) => {
            let defaultValue = ''
            if (field.key === 'model') defaultValue = 'gemini-3-flash-preview'
            else if (field.key === 'ai_image_provider')
              defaultValue = 'pollinations'
            else if (field.key === 'ai_image_model') defaultValue = 'flux-anime'
            else if (field.key === 'ai_image_width') defaultValue = '512'
            else if (field.key === 'ai_image_height') defaultValue = '768'
            return { ...field, value: defaultValue }
          }),
        },
        ui_config: {
          ...data.ui_config,
          config_fields: data.ui_config.config_fields.map((field: any) => {
            let defaultValue = ''
            if (field.key === 'wallpaper_url') {
              defaultValue =
                'https://images.unsplash.com/photo-1579546929518-9e396f3cc809'
            } else if (field.key === 'wallpaper_blur') {
              defaultValue = '3'
            }
            return { ...field, value: defaultValue }
          }),
        },
      }

      setConfig(clearedData)

      await new Promise((resolve) => setTimeout(resolve, 200))

      showMessage(t.config.savingDefault, 'info', 0)

      // 获取 CSRF Token
      await getCSRFToken(true)

      // updateConfig throws on failure — do not clean dirty / emit success otherwise
      const saveResult = await updateConfig(clearedData)
      if (saveResult?.success === false) {
        throw new Error(saveResult.message || t.config.resetFailed)
      }
      setInitialConfig(JSON.parse(JSON.stringify(clearedData)))

      showMessage(t.config.configReset, 'success', 5000)

      window.dispatchEvent(
        new CustomEvent('config-reset-result', {
          detail: {
            success: true,
            message: saveResult.message || t.config.configReset,
          },
        }),
      )
    } catch (error) {
      const errorMsg = `${t.config.resetFailed}${error instanceof Error ? error.message : t.errors.unknown}`
      showMessage(errorMsg, 'error', 0)
      window.dispatchEvent(
        new CustomEvent('config-reset-result', {
          detail: { success: false, message: errorMsg },
        }),
      )
    } finally {
      setSaving(false)
    }
  }, [showMessage, t])

  const loadConfig = React.useCallback(async () => {
    setLoading(true)
    try {
      const data = await fetchConfig()
      const normalizedData = {
        ...data,
        auto_fetch: data.auto_fetch || DEFAULT_AUTO_FETCH_CONFIG,
      }
      setConfig(normalizedData)
      setInitialConfig(JSON.parse(JSON.stringify(normalizedData)))
      notifyDirtyState(false)

      const event = new CustomEvent('config-loaded', { detail: data })
      window.dispatchEvent(event)
    } catch (_error) {
      showMessage(t.config.loadConfigFailed, 'error')
    } finally {
      setLoading(false)
    }
  }, [notifyDirtyState])

  useEffect(() => {
    loadConfig()
    loadPermissionConfig()
    loadModuleVisibilityPreferences()
    loadHitokotoSettings()
    loadReportSettings()
    loadNotificationSettings()
    loadOAuthSettings()
  }, [
    loadConfig,
    loadPermissionConfig,
    loadModuleVisibilityPreferences,
    loadHitokotoSettings,
    loadReportSettings,
    loadNotificationSettings,
    loadOAuthSettings,
  ])

  // Discord 一键授权回调：/config?section=platforms&discord_oauth=ok|error
  useEffect(() => {
    if (typeof window === 'undefined') return
    const params = new URLSearchParams(window.location.search)
    const oauth = params.get('discord_oauth')
    if (!oauth) return

    if (params.get('section') === 'platforms') {
      setActiveSection('platforms')
    }

    if (oauth === 'ok') {
      showMessage(t.config.discordOAuthSuccess, 'success')
      setPlatformModalOpen('Discord')
      void loadConfig()
    } else {
      const reason = params.get('reason') || 'unknown'
      showMessage(
        `${t.config.discordOAuthFailed}${reason !== 'unknown' ? ` (${reason})` : ''}`,
        'error',
      )
    }

    params.delete('discord_oauth')
    params.delete('reason')
    const qs = params.toString()
    const next = `${window.location.pathname}${qs ? `?${qs}` : ''}${window.location.hash}`
    window.history.replaceState({}, '', next)
  }, [loadConfig, t.config.discordOAuthFailed, t.config.discordOAuthSuccess])

  useEffect(() => {
    const handleSaveEvent = () => handleSave()
    const handleResetEvent = () => handleReset()

    window.addEventListener('request-config-save', handleSaveEvent)
    window.addEventListener('config-reset', handleResetEvent)

    return () => {
      window.removeEventListener('request-config-save', handleSaveEvent)
      window.removeEventListener('config-reset', handleResetEvent)
    }
  }, [handleSave, handleReset])

  // 测试语音服务可用性（返回 Promise 供组件使用）
  const handleSpeechTest = React.useCallback(async (): Promise<{
    success: boolean
    message: string
  }> => {
    if (!config) {
      return { success: false, message: 'Config not loaded' }
    }

    try {
      const result = await checkSpeechStatus()

      return {
        success: result.available === true,
        message: result.available
          ? t.config.speechTestSuccess
          : result.error || t.config.speechTestFailed,
      }
    } catch (_error) {
      return {
        success: false,
        message: t.config.speechTestFailed,
      }
    }
  }, [config, t])

  const updateConfigField = React.useCallback(
    (
      section: 'ai' | 'ui',
      fieldKey: string,
      value: string,
      providerFieldKey?: string,
    ) => {
      if (!config) return

      const sectionKey = `${section}_config` as 'ai_config' | 'ui_config'
      const sectionConfig = config[sectionKey]
      const newFields = [...sectionConfig.config_fields]
      const field = newFields.find((f) => f.key === fieldKey)

      if (field) {
        // 🔒 安全措施：如果新值包含掩码字符，说明用户在掩码上直接输入，需要清除掩码
        if (
          isMaskedValue(value) &&
          value !== '••••••••' &&
          value !== '********'
        ) {
          // 移除所有掩码字符，只保留用户新输入的内容
          field.value = value.replace(/[•*]+/g, '')
        } else {
          field.value = value
        }

        if (providerFieldKey && fieldKey === providerFieldKey) {
          setConfig({
            ...config,
            [sectionKey]: {
              ...sectionConfig,
              provider: value,
              config_fields: newFields,
            },
          })
        } else {
          setConfig({
            ...config,
            [sectionKey]: { ...sectionConfig, config_fields: newFields },
          })
        }
        notifyDirtyState(true)
      }
    },
    [config, notifyDirtyState],
  )

  const updateFieldValue = React.useCallback(
    (platformIndex: number, fieldKey: string, value: string) => {
      if (!config) return

      const newPlatforms = [...config.platforms]
      const field = newPlatforms[platformIndex].config_fields.find(
        (f) => f.key === fieldKey,
      )
      if (field) {
        // 🔒 安全措施：如果新值包含掩码字符，说明用户在掩码上直接输入，需要清除掩码
        // 检测是否在掩码基础上输入（例如 "a••••••••"）
        if (
          isMaskedValue(value) &&
          value !== '••••••••' &&
          value !== '********'
        ) {
          // 移除所有掩码字符，只保留用户新输入的内容
          field.value = value.replace(/[•*]+/g, '')
        } else {
          field.value = value
        }
        setConfig({ ...config, platforms: newPlatforms })
        notifyDirtyState(true)
      }
    },
    [config, notifyDirtyState],
  )

  const updateAiFieldValue = React.useCallback(
    (fieldKey: string, value: string) => {
      updateConfigField('ai', fieldKey, value, 'provider')
    },
    [updateConfigField],
  )

  const updateUiFieldValue = React.useCallback(
    (fieldKey: string, value: string) => {
      updateConfigField('ui', fieldKey, value)
    },
    [updateConfigField],
  )

  const togglePlatform = React.useCallback(
    (platformIndex: number) => {
      if (!config) return

      const newPlatforms = [...config.platforms]
      newPlatforms[platformIndex].enabled = !newPlatforms[platformIndex].enabled
      setConfig({ ...config, platforms: newPlatforms })
      notifyDirtyState(true)
    },
    [config, notifyDirtyState],
  )

  const updateAutoFetchConfig = React.useCallback(
    (auto_fetch: PlatformAutoFetchConfig) => {
      if (!config) return
      setConfig({ ...config, auto_fetch })
      notifyDirtyState(true)
    },
    [config, notifyDirtyState],
  )

  // 🆕 调整平台顺序（拖拽排序）：该顺序会作为报告页平台卡片的出现顺序保存
  // fromIndex 的卡片移动到 toIndex 位置
  const reorderPlatform = React.useCallback(
    (fromIndex: number, toIndex: number) => {
      if (!config) return
      if (
        fromIndex === toIndex ||
        fromIndex < 0 ||
        toIndex < 0 ||
        fromIndex >= config.platforms.length ||
        toIndex >= config.platforms.length
      ) {
        return
      }

      const newPlatforms = [...config.platforms]
      const [moved] = newPlatforms.splice(fromIndex, 1)
      newPlatforms.splice(toIndex, 0, moved)
      setConfig({ ...config, platforms: newPlatforms })
      notifyDirtyState(true)
    },
    [config, notifyDirtyState],
  )

  const isBaseConfigDirty = useMemo(() => {
    if (!config || !initialConfig) return false
    return JSON.stringify(config) !== JSON.stringify(initialConfig)
  }, [config, initialConfig])

  const isLibrarySourceDirty = useMemo(
    () =>
      !areLibrarySourcePreferencesEqual(
        librarySourceDraft,
        savedLibrarySourcePreferences,
      ),
    [librarySourceDraft, savedLibrarySourcePreferences],
  )

  const isModuleVisibilityDirty = useMemo(
    () =>
      !areModuleVisibilityPreferencesEqual(
        moduleVisibilityDraft,
        savedModuleVisibilityPreferences,
      ),
    [moduleVisibilityDraft, savedModuleVisibilityPreferences],
  )

  const isHitokotoDirty = useMemo(
    () => !areHitokotoConfigsEqual(hitokotoDraft, savedHitokotoConfig),
    [hitokotoDraft, savedHitokotoConfig],
  )

  const isReportSettingsDirty = useMemo(
    () => !areReportSettingsEqual(reportSettingsDraft, savedReportSettings),
    [reportSettingsDraft, savedReportSettings],
  )

  const isPermissionDirty = useMemo(
    () =>
      JSON.stringify(permissionConfig) !==
      JSON.stringify(savedPermissionConfig),
    [permissionConfig, savedPermissionConfig],
  )

  const isNotificationDirty = useMemo(
    () =>
      !areNotificationPreferencesEqual(
        notificationDraft,
        savedNotificationPreferences,
      ),
    [notificationDraft, savedNotificationPreferences],
  )

  const isOAuthDirty = useMemo(
    () => !areOAuthSettingsEqual(oauthDraft, savedOAuthSettings),
    [oauthDraft, savedOAuthSettings],
  )

  const isFavoritesDirty = useMemo(
    () => JSON.stringify(favorites) !== JSON.stringify(savedFavorites),
    [favorites, savedFavorites],
  )

  const isConfigDirty =
    isBaseConfigDirty ||
    isLibrarySourceDirty ||
    isModuleVisibilityDirty ||
    isHitokotoDirty ||
    isReportSettingsDirty ||
    isPermissionDirty ||
    isNotificationDirty ||
    isOAuthDirty ||
    isFavoritesDirty

  useEffect(() => {
    notifyDirtyState(isConfigDirty)
  }, [isConfigDirty, notifyDirtyState])

  useEffect(() => {
    if (!isConfigDirty) return

    const warnAboutUnsavedChanges = (event: BeforeUnloadEvent) => {
      event.preventDefault()
    }
    window.addEventListener('beforeunload', warnAboutUnsavedChanges)
    return () =>
      window.removeEventListener('beforeunload', warnAboutUnsavedChanges)
  }, [isConfigDirty])

  const getSectionProps = (sectionId: string) => {
    const item = quickAccessItems.find((i) => i.id === sectionId)
    if (!item) {
      // 理论上不会发生，因为 activeSection 总是有效的
      return { title: '', icon: null, description: '' }
    }
    return {
      title: item.label,
      icon: item.icon,
      sectionId: item.id,
      description:
        searchableContent.find((c) => c.section === sectionId)?.description ||
        '',
    }
  }

  const renderActiveSection = () => {
    if (!config) return null

    const props = getSectionProps(activeSection)

    switch (activeSection) {
      case 'platforms':
        return (
          <div className="config-section">
            <div className="section-header">
              <div className="section-header-left">
                <span className="section-icon icon-platforms">
                  {props.icon}
                </span>
                <div>
                  <h2 className="section-title">{props.title}</h2>
                  <p className="section-description">{props.description}</p>
                </div>
              </div>
            </div>

            <PlatformAutoRefreshSettings
              value={config.auto_fetch}
              enabledPlatformCount={
                config.platforms.filter((platform) => platform.enabled).length
              }
              onChange={updateAutoFetchConfig}
            />

            <div className="platforms-grid">
              {config.platforms.map((platform, index) =>
                (() => {
                  const platformConfigured = isPlatformConfigured(platform)
                  const toggleTitle = !platformConfigured
                    ? t.config.notConfigured
                    : undefined

                  const isDragging = dragIndex === index
                  const isDragOver =
                    dragOverIndex === index && dragIndex !== index

                  return (
                    <div
                      key={platform.name}
                      className={`platform-card platform-card-enter${
                        isDragging ? ' dragging' : ''
                      }${isDragOver ? ' drag-over' : ''}`}
                      style={{
                        cursor: 'pointer',
                        animationDelay: `${0.2 + index * 0.05}s`,
                      }}
                      draggable={dragArmedIndex === index}
                      onClick={() => setPlatformModalOpen(platform.name)}
                      onDragStart={(e) => {
                        setDragIndex(index)
                        e.dataTransfer.effectAllowed = 'move'
                      }}
                      onDragOver={(e) => {
                        if (dragIndex === null) return
                        e.preventDefault()
                        e.dataTransfer.dropEffect = 'move'
                        if (dragOverIndex !== index) setDragOverIndex(index)
                      }}
                      onDrop={(e) => {
                        e.preventDefault()
                        if (dragIndex !== null)
                          reorderPlatform(dragIndex, index)
                        setDragIndex(null)
                        setDragOverIndex(null)
                        setDragArmedIndex(null)
                      }}
                      onDragEnd={() => {
                        setDragIndex(null)
                        setDragOverIndex(null)
                        setDragArmedIndex(null)
                      }}
                    >
                      <div className="platform-header">
                        <div className="platform-info">
                          <button
                            type="button"
                            className="platform-drag-handle"
                            aria-label={t.config.dragToReorder}
                            title={t.config.dragToReorder}
                            onClick={(e) => e.stopPropagation()}
                            onPointerDown={() => setDragArmedIndex(index)}
                            onPointerUp={() => setDragArmedIndex(null)}
                          >
                            <span className="platform-order-num">
                              {index + 1}
                            </span>
                            <LuGripVertical className="platform-drag-grip" />
                          </button>
                          <div className="platform-icon-wrapper">
                            <PlatformIcon
                              platform={platform.name}
                              className="platform-icon"
                            />
                          </div>
                          <div className="platform-details">
                            <div className="platform-title-row">
                              <h3 className="platform-name">{platform.name}</h3>
                              <span
                                className={`status-badge ${platformConfigured ? 'configured' : 'unconfigured'}`}
                              >
                                {platformConfigured
                                  ? t.config.configured
                                  : t.config.notConfigured}
                              </span>
                            </div>
                            <p className="platform-desc">
                              {getPlatformDescription(platform)}
                            </p>
                          </div>
                        </div>
                        <div className="platform-actions">
                          <label
                            className={`toggle-switch ${platformConfigured ? '' : 'disabled'}`}
                            onClick={(e) => e.stopPropagation()}
                            title={toggleTitle}
                          >
                            <input
                              type="checkbox"
                              checked={platform.enabled}
                              onChange={() => togglePlatform(index)}
                              aria-label={`Enable ${platform.name}`}
                              disabled={!platformConfigured}
                            />
                            <span className="toggle-slider"></span>
                          </label>
                        </div>
                      </div>
                    </div>
                  )
                })(),
              )}
            </div>
          </div>
        )
      case 'ai':
        return (
          <AiConfigSection
            configFields={config.ai_config.config_fields}
            updateValue={updateAiFieldValue}
            onSpeechTest={handleSpeechTest}
            {...props}
          />
        )
      case 'ui':
        return (
          <UiConfigSection
            configFields={config.ui_config.config_fields}
            updateValue={updateUiFieldValue}
            getFieldLabel={getFieldLabel}
            getFieldPlaceholder={getFieldPlaceholder}
            {...props}
          />
        )
      case 'oauth':
        return (
          <OAuthConfigSection
            configFields={config.ui_config.config_fields}
            providers={oauthDraft.providers}
            allowRegister={oauthDraft.allowLocalRegistration}
            loading={oauthLoading}
            onProvidersChange={(providers) =>
              setOAuthDraft((current) => ({ ...current, providers }))
            }
            onAllowRegisterChange={(allowLocalRegistration) =>
              setOAuthDraft((current) => ({
                ...current,
                allowLocalRegistration,
              }))
            }
            {...props}
          />
        )
      case 'music':
        return (
          <MusicConfigSection
            configFields={config.ui_config.config_fields}
            updateValue={updateUiFieldValue}
            onMessage={(msg) => {
              showMessage(msg, 'success')
            }}
            {...props}
          />
        )
      case 'network':
        return (
          <NetworkConfigSection
            configFields={config.ui_config.config_fields}
            updateValue={updateUiFieldValue}
            {...props}
          />
        )
      case 'permissions':
        return (
          <PermissionsConfigSection
            permissionConfig={permissionConfig}
            updatePermissionConfig={updatePermissionConfig}
            loading={permissionLoading}
            {...props}
          />
        )
      case 'modules':
        return (
          <ModuleConfigSection
            sourceDraft={librarySourceDraft}
            setSourceDraft={setLibrarySourceDraft}
            visibilityDraft={moduleVisibilityDraft}
            setVisibilityDraft={setModuleVisibilityDraft}
            isSourceDirty={isLibrarySourceDirty}
            saveRevision={librarySourceSaveRevision}
            onSourcePreferencesLoaded={handleLibrarySourcePreferencesLoaded}
            hitokotoDraft={hitokotoDraft}
            setHitokotoDraft={setHitokotoDraft}
            reportSettingsDraft={reportSettingsDraft}
            setReportSettingsDraft={setReportSettingsDraft}
            onMessage={handleModuleMessage}
            {...props}
          />
        )
      case 'notifications':
        return (
          <NotificationConfigSection
            preferences={notificationDraft}
            sources={notificationSources}
            events={notificationEvents}
            loading={notificationLoading}
            onChange={setNotificationDraft}
            {...props}
          />
        )
      case 'advanced':
        return (
          <AdvancedConfigSection
            onReset={handleReset}
            onMessage={(msg, type = 'info') => showMessage(msg, type)}
            {...props}
          />
        )
      case 'about':
        return <AboutConfigSection {...props} />
      default:
        return null
    }
  }

  if (loading) {
    return null
  }

  if (!config) {
    return (
      <div className="modern-config-error">
        <FaExclamationTriangle className="error-icon" />
        <p>{t.config.loadConfigFailed}</p>
      </div>
    )
  }

  return (
    <motion.div
      className="modern-config-container"
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -20 }}
      transition={{ duration: 0.4, ease: [0.34, 1.56, 0.64, 1] }}
    >
      {/* 消息提示 */}
      {message && <Toast message={message} type={messageType} />}

      {/* 配置导航卡片 */}
      <motion.div
        className="config-nav-card"
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.1, duration: 0.3 }}
      >
        <div className="config-nav-header">
          <div className="nav-header-left">
            <span className="nav-icon">
              <MyriadConfigIcon kind="ui" />
            </span>
            <div>
              <h3 className="nav-title">{t.config.title}</h3>
              <p className="nav-subtitle">{t.config.selectProject}</p>
            </div>
          </div>
          <div className="nav-header-search">
            <div className="search-input-wrapper">
              <FaSearch className="search-icon" />
              <input
                type="text"
                placeholder={t.config.searchConfig}
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="search-input"
              />
              {searchQuery && (
                <button
                  onClick={() => setSearchQuery('')}
                  className="search-clear"
                  aria-label="Clear search"
                >
                  <FaTimes />
                </button>
              )}
            </div>
          </div>
        </div>

        {/* 搜索结果 */}
        {searchQuery ? (
          <div className="search-results">
            <h4 className="search-results-title">
              {t.config.searchResults} ({filteredContent.length})
            </h4>
            <div className="search-results-list">
              {filteredContent.length > 0 ? (
                filteredContent.map((item, index) => (
                  <button
                    key={index}
                    onClick={() => {
                      handleSectionChange(item.section)
                    }}
                    className="search-result-item"
                  >
                    <div className="search-result-content">
                      <h4>{item.title}</h4>
                      <p>{item.description}</p>
                    </div>
                    <span className="search-result-arrow">→</span>
                  </button>
                ))
              ) : (
                <div className="search-no-results">
                  <p>{t.config.noMatchingConfig}</p>
                </div>
              )}
            </div>
          </div>
        ) : (
          <div className="config-nav-content">
            {/* 收藏夹 */}
            {favorites.length > 0 && (
              <div className="nav-section">
                <div className="nav-section-header">
                  <FaStar className="nav-section-icon" />
                  <span className="nav-section-title">
                    {t.config.favorites}
                  </span>
                </div>
                <div className="quick-access-grid">
                  {favorites.map((fav) => {
                    const item = quickAccessItems.find((i) => i.id === fav)
                    return item ? (
                      <QuickAccessCard
                        key={item.id}
                        item={item}
                        isActive={activeSection === item.section}
                        isFavorite={true}
                        onCardClick={handleSectionChange}
                        onToggleFavorite={toggleFavorite}
                      />
                    ) : null
                  })}
                </div>
              </div>
            )}

            {/* 所有配置 */}
            <div className="nav-section">
              <div className="nav-section-header">
                <span className="nav-section-title">{t.config.allConfig}</span>
              </div>
              <div className="quick-access-grid">
                {quickAccessItems.map((item) => (
                  <QuickAccessCard
                    key={item.id}
                    item={item}
                    isActive={activeSection === item.section}
                    isFavorite={favorites.includes(item.id)}
                    onCardClick={handleSectionChange}
                    onToggleFavorite={toggleFavorite}
                  />
                ))}
              </div>
            </div>
          </div>
        )}
      </motion.div>

      {/* 配置内容区域 */}
      {!searchQuery && (
        <div className="config-content">{renderActiveSection()}</div>
      )}

      {/* 平台配置弹窗 */}
      {platformModalOpen &&
        config &&
        (() => {
          const platformIndex = config.platforms.findIndex(
            (p) => p.name === platformModalOpen,
          )
          if (platformIndex === -1) return null
          const platform = config.platforms[platformIndex]

          return (
            <div
              className="modal-overlay"
              onClick={() => setPlatformModalOpen(null)}
            >
              <div
                className="modal-content"
                onClick={(e) => e.stopPropagation()}
              >
                <div className="modal-header">
                  <div className="modal-title-section">
                    <div className="platform-icon-wrapper">
                      <PlatformIcon
                        platform={platform.name}
                        className="platform-icon"
                      />
                    </div>
                    <div>
                      <h3 className="modal-title">{platform.name}</h3>
                      <p className="modal-subtitle">
                        {getPlatformDescription(platform)}
                      </p>
                    </div>
                  </div>
                  <button
                    onClick={() => setPlatformModalOpen(null)}
                    className="modal-close-button"
                    aria-label={t.config.closeLabel}
                  >
                    <FaTimes />
                  </button>
                </div>

                <div className="modal-body">
                  {isBangumiPlatform(platform) && (
                    <div
                      className={`platform-requirement-hint ${
                        hasBangumiCredential(platform) ? 'is-ok' : 'is-warning'
                      }`}
                    >
                      {t.config.bangumiCredentialRequirement}
                    </div>
                  )}

                  {platform.name.toLowerCase() === 'discord' && (
                    <div
                      className="platform-requirement-hint is-ok"
                      style={{ marginBottom: 12 }}
                    >
                      <p style={{ margin: '0 0 10px' }}>
                        {t.config.discordConnectHint}
                      </p>
                      <button
                        type="button"
                        className="btn-base btn-primary"
                        onClick={connectDiscordOAuth}
                        style={{ width: '100%' }}
                      >
                        {platform.has_token
                          ? t.config.discordReconnect
                          : t.config.discordConnect}
                      </button>
                    </div>
                  )}

                  {platform.config_fields.map((field) => (
                    <div key={field.key} className="config-field">
                      <label
                        htmlFor={`modal-platform-${platformIndex}-${field.key}`}
                        className="field-label"
                      >
                        {getPlatformFieldLabel(platform, field)}
                        {field.required && <span className="required">*</span>}
                      </label>
                      <input
                        id={`modal-platform-${platformIndex}-${field.key}`}
                        type={field.field_type}
                        value={field.value}
                        onChange={(e) =>
                          updateFieldValue(
                            platformIndex,
                            field.key,
                            e.target.value,
                          )
                        }
                        onFocus={(e) => {
                          // 🔒 如果是掩码值，自动选中全部内容，用户输入会直接替换
                          const isMasked =
                            e.target.value === '••••••••' ||
                            e.target.value === '********'
                          if (isMasked) {
                            e.target.select()
                          }
                        }}
                        placeholder={getPlatformFieldPlaceholder(
                          platform,
                          field,
                        )}
                        className="field-input"
                      />
                    </div>
                  ))}
                </div>
                <div className="modal-footer">
                  <button
                    onClick={() => setPlatformModalOpen(null)}
                    className="btn-base btn-primary"
                  >
                    {t.common.confirm}
                  </button>
                </div>
              </div>
            </div>
          )
        })()}

      {isConfigDirty && (
        <div className="floating-save-container">
          <button
            onClick={handleSave}
            className="btn-base btn-primary floating-save-btn"
            aria-label={t.config.saveConfigLabel}
            disabled={saving}
          >
            <svg
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
              width="20"
              height="20"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M5 13l4 4L19 7"
              />
            </svg>
            <span>{saving ? t.config.savingConfig : t.config.saveConfig}</span>
          </button>
        </div>
      )}
    </motion.div>
  )
}

export default ModernConfigForm
