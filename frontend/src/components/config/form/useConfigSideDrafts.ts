import type { NotificationEventDefinition, NotificationPreferences, NotificationSourceKey } from '../../../services/notificationPreferencesApi'
import type { ModuleVisibilityPreferences } from '../../../utils/moduleVisibility'
import type { OAuthSettings } from '../../../utils/oauthSettings'
import type { HitokotoConfig } from '../../../utils/quote'
import type { ReportSettings } from '../../../utils/reportSettings'
import type { FederationPolicyDraft } from '../FederationConfigSection'
import type { LibrarySourcePreferences } from '../ModuleConfigSection'
import type { PermissionConfigValues } from '../PermissionsConfigSection'
import type { SaveLibrarySourcePreferencesResponse, ShowMessage } from './types'
import { useCallback, useState } from 'react'
import {
  fetchPermissionsConfig,
} from '../../../lib/api'
import apiService from '../../../services/api'
import { federationApi } from '../../../services/federationApi'
import notificationPreferencesApi, {
  cloneNotificationPreferences,
  DEFAULT_NOTIFICATION_CATALOG,
  DEFAULT_NOTIFICATION_PREFERENCES,

} from '../../../services/notificationPreferencesApi'
import { dispatchLibraryPreferencesUpdated } from '../../../utils/libraryPreferences'
import {
  DEFAULT_MODULE_VISIBILITY_PREFERENCES,
  dispatchModuleVisibilityPreferencesUpdated,
  fetchModuleVisibilityPreferences,

  normalizeModuleVisibilityPreferences,
  updateModuleVisibilityPreferences,
} from '../../../utils/moduleVisibility'
import {
  cloneOAuthSettings,
  DEFAULT_OAUTH_SETTINGS,
  fetchOAuthSettings,

} from '../../../utils/oauthSettings'
import {
  DEFAULT_HITOKOTO_CONFIG,
  fetchHitokotoConfig,

  updateHitokotoConfig,
} from '../../../utils/quote'
import {
  DEFAULT_REPORT_SETTINGS,
  fetchReportSettings,

  updateReportSettings,
} from '../../../utils/reportSettings'
import { clearLibraryDataCache } from '../../../utils/requestDedup'
import {
  DEFAULT_FEDERATION_POLICY,

  federationPolicyFromApi,
} from '../FederationConfigSection'
import {
  DEFAULT_LIBRARY_SOURCE_PREFERENCES,

  normalizeLibraryPreferences,
} from '../ModuleConfigSection'
import { DEFAULT_PERMISSION_CONFIG } from './defaults'

export function useConfigSideDrafts(
  isAdmin: boolean,
  showMessage: ShowMessage,
  t: {
    config: {
      loadConfigFailed: string
      moduleVisibilityLoadFailed: string
      hitokotoLoadFailed: string
      reportSettingsLoadFailed: string
      librarySourceSaveFailed: string
    }
  },
) {
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

  const [federationPolicyDraft, setFederationPolicyDraft] =
    useState<FederationPolicyDraft>(DEFAULT_FEDERATION_POLICY)
  const [savedFederationPolicy, setSavedFederationPolicy] =
    useState<FederationPolicyDraft>(DEFAULT_FEDERATION_POLICY)

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

  const loadPermissionConfig = useCallback(async () => {
    try {
      setPermissionLoading(true)
      const response = await fetchPermissionsConfig()
      if (response.success && response.config) {
        const { guest, user, user_ai_quota, guest_ai_quota } = response.config
        const loaded: PermissionConfigValues = {
          user_perm_ai_generate: user.ai_generate,
          user_perm_ai_analyze: user.ai_analyze,
          user_perm_ai_chat: user.ai_chat,
          user_perm_report_write: user.report_write,
          user_perm_network_fetch: user.network_fetch,
          user_perm_component_theme: user.component_theme,
          user_perm_shortcut_register: user.shortcut_register,
          user_perm_event_publish: user.event_publish,
          user_perm_ai_image: user.ai_image,
          user_perm_scheduler_register: user.scheduler_register,
          user_perm_speech_tts: user.speech_tts,
          user_perm_speech_asr: user.speech_asr,
          user_perm_storage_write: user.storage_write ?? false,
          user_perm_brew_comment_write: user.brew_comment_write ?? false,
          guest_perm_ai_generate: guest.ai_generate,
          guest_perm_ai_analyze: guest.ai_analyze,
          guest_perm_ai_chat: guest.ai_chat,
          guest_perm_report_write: guest.report_write,
          guest_perm_network_fetch: guest.network_fetch,
          guest_perm_component_theme: guest.component_theme,
          guest_perm_shortcut_register: guest.shortcut_register,
          guest_perm_event_publish: guest.event_publish,
          guest_perm_ai_image: guest.ai_image,
          guest_perm_scheduler_register: guest.scheduler_register,
          guest_perm_speech_tts: guest.speech_tts,
          guest_perm_speech_asr: guest.speech_asr,
          guest_perm_storage_write: guest.storage_write ?? false,
          guest_perm_brew_comment_write: guest.brew_comment_write ?? false,
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
  }, [showMessage, t.config.loadConfigFailed])

  const loadFederationPolicy = useCallback(async () => {
    if (!isAdmin) return
    try {
      const p = await federationApi.getTrustPolicy()
      const draft = federationPolicyFromApi(p)
      setFederationPolicyDraft(draft)
      setSavedFederationPolicy(draft)
    } catch (error) {
      console.error('Failed to load federation trust policy:', error)
    }
  }, [isAdmin])

  const updateFederationPolicy = useCallback(
    (patch: Partial<FederationPolicyDraft>) => {
      setFederationPolicyDraft((prev) => ({ ...prev, ...patch }))
    },
    [],
  )

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
  }, [showMessage, t.config.loadConfigFailed])

  const handleLibrarySourcePreferencesLoaded = useCallback(
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

  const loadModuleVisibilityPreferences = useCallback(async () => {
    try {
      const preferences = await fetchModuleVisibilityPreferences()
      setSavedModuleVisibilityPreferences(preferences)
      setModuleVisibilityDraft(preferences)
    } catch {
      showMessage(t.config.moduleVisibilityLoadFailed, 'error')
    }
  }, [showMessage, t.config.moduleVisibilityLoadFailed])

  const loadHitokotoSettings = useCallback(async () => {
    try {
      // 配置编辑器必须看到权威值，不吃 fetchHitokotoConfig 的进程内缓存，
      // 否则草稿可能基于最多 5 分钟前的旧配置，保存时把别处的改动覆盖掉
      const config = await fetchHitokotoConfig({ force: true })
      setSavedHitokotoConfig(config)
      setHitokotoDraft(config)
    } catch {
      showMessage(t.config.hitokotoLoadFailed, 'error')
    }
  }, [showMessage, t.config.hitokotoLoadFailed])

  const loadReportSettings = useCallback(async () => {
    try {
      const settings = await fetchReportSettings()
      setSavedReportSettings(settings)
      setReportSettingsDraft(settings)
    } catch {
      showMessage(t.config.reportSettingsLoadFailed, 'error')
    }
  }, [showMessage, t.config.reportSettingsLoadFailed])

  /** 立即写入库来源（用于 modules 重置等旁路路径） */
  const writeLibrarySourcePreferences = useCallback(
    async (draft: LibrarySourcePreferences) => {
      const saved = await apiService.put<SaveLibrarySourcePreferencesResponse>(
        '/library/preferences',
        draft,
      )
      if (!saved.success) {
        throw new Error(saved.message || t.config.librarySourceSaveFailed)
      }
      return normalizeLibraryPreferences(saved.preferences)
    },
    [t.config.librarySourceSaveFailed],
  )

  const applyLibrarySourcePreferences = useCallback(
    (preferences: LibrarySourcePreferences) => {
      setLibrarySourceDraft(preferences)
      setSavedLibrarySourcePreferences(preferences)
      setLibrarySourceSaveRevision((r) => r + 1)
      clearLibraryDataCache()
      dispatchLibraryPreferencesUpdated({
        categories: preferences.categories,
        layout: preferences.layout,
      })
    },
    [],
  )

  const writeModuleVisibility = useCallback(
    async (draft: ModuleVisibilityPreferences) => {
      const raw = await updateModuleVisibilityPreferences(draft)
      return normalizeModuleVisibilityPreferences(raw)
    },
    [],
  )

  const applyModuleVisibility = useCallback(
    (preferences: ModuleVisibilityPreferences) => {
      setModuleVisibilityDraft(preferences)
      setSavedModuleVisibilityPreferences(preferences)
      dispatchModuleVisibilityPreferencesUpdated(preferences)
    },
    [],
  )

  const writeHitokoto = useCallback(
    async (draft: HitokotoConfig) => updateHitokotoConfig(draft),
    [],
  )

  const applyHitokoto = useCallback((saved: HitokotoConfig) => {
    setHitokotoDraft(saved)
    setSavedHitokotoConfig(saved)
  }, [])

  const writeReportSettings = useCallback(
    async (draft: ReportSettings) => updateReportSettings(draft),
    [],
  )

  const applyReportSettings = useCallback((saved: ReportSettings) => {
    setReportSettingsDraft(saved)
    setSavedReportSettings(saved)
  }, [])

  return {
    permissionConfig,
    setPermissionConfig,
    savedPermissionConfig,
    setSavedPermissionConfig,
    permissionLoading,
    updatePermissionConfig,
    loadPermissionConfig,

    notificationDraft,
    setNotificationDraft,
    savedNotificationPreferences,
    setSavedNotificationPreferences,
    notificationSources,
    notificationEvents,
    notificationLoading,
    loadNotificationSettings,

    oauthDraft,
    setOAuthDraft,
    savedOAuthSettings,
    setSavedOAuthSettings,
    oauthLoading,
    loadOAuthSettings,

    librarySourceDraft,
    setLibrarySourceDraft,
    savedLibrarySourcePreferences,
    setSavedLibrarySourcePreferences,
    librarySourceSaveRevision,
    setLibrarySourceSaveRevision,
    handleLibrarySourcePreferencesLoaded,
    writeLibrarySourcePreferences,
    applyLibrarySourcePreferences,

    moduleVisibilityDraft,
    setModuleVisibilityDraft,
    savedModuleVisibilityPreferences,
    setSavedModuleVisibilityPreferences,
    loadModuleVisibilityPreferences,
    writeModuleVisibility,
    applyModuleVisibility,

    hitokotoDraft,
    setHitokotoDraft,
    savedHitokotoConfig,
    setSavedHitokotoConfig,
    loadHitokotoSettings,
    writeHitokoto,
    applyHitokoto,

    reportSettingsDraft,
    setReportSettingsDraft,
    savedReportSettings,
    setSavedReportSettings,
    loadReportSettings,
    writeReportSettings,
    applyReportSettings,

    federationPolicyDraft,
    setFederationPolicyDraft,
    savedFederationPolicy,
    setSavedFederationPolicy,
    loadFederationPolicy,
    updateFederationPolicy,
  }
}
