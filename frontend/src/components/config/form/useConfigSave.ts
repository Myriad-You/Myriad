import type { Dispatch, SetStateAction } from 'react'
import type { NotificationPreferences } from '../../../services/notificationPreferencesApi'
import type { ModuleVisibilityPreferences } from '../../../utils/moduleVisibility'
import type { OAuthSettings } from '../../../utils/oauthSettings'
import type { HitokotoConfig } from '../../../utils/quote'
import type { ReportSettings } from '../../../utils/reportSettings'
import type { FederationPolicyDraft } from '../FederationConfigSection'
import type { LibrarySourcePreferences } from '../ModuleConfigSection'
import type { PermissionConfigValues } from '../PermissionsConfigSection'
import type {
  Config,
  SaveLibrarySourcePreferencesResponse,
  ShowMessage,
} from './types'
import { useCallback } from 'react'
import { userFacingError } from '../../../utils/userFacingError'
import { API_URL } from '../../../config'
import {
  reloadSystemConfig,
  updateConfig,
  updatePermissionsConfig,
} from '../../../lib/api'
import apiService from '../../../services/api'
import { federationApi } from '../../../services/federationApi'
import notificationPreferencesApi, {
  areNotificationPreferencesEqual,
  cloneNotificationPreferences,

} from '../../../services/notificationPreferencesApi'
import { getCSRFToken } from '../../../utils/csrf'
import { deepEqual } from '../../../utils/deepEqual'
import { dispatchLibraryPreferencesUpdated } from '../../../utils/libraryPreferences'
import {
  areModuleVisibilityPreferencesEqual,
  dispatchModuleVisibilityPreferencesUpdated,

  normalizeModuleVisibilityPreferences,
  updateModuleVisibilityPreferences,
} from '../../../utils/moduleVisibility'
import {
  areOAuthSettingsEqual,
  cloneOAuthSettings,

  updateOAuthSettings,
} from '../../../utils/oauthSettings'
import {
  areHitokotoConfigsEqual,

  updateHitokotoConfig,
} from '../../../utils/quote'
import {
  areReportSettingsEqual,

  updateReportSettings,
} from '../../../utils/reportSettings'
import {
  clearDedupCache,
  clearLibraryDataCache,
} from '../../../utils/requestDedup'
import {
  areFederationPoliciesEqual,

  federationPolicyToUpdateRequest,
} from '../FederationConfigSection'
import {
  areLibrarySourcePreferencesEqual,

  normalizeLibraryPreferences,
} from '../ModuleConfigSection'
import {
  hasBangumiCredential,
  isBangumiPlatform,
} from '../PlatformsConfigSection'
import {
  configChangesNeedFooterReload,
  configChangesNeedHardReload,
  configChangesNeedMetadataReload,
  configChangesNeedPlatformsCacheInvalidation,
  configChangesNeedPwaReload,
  configChangesNeedRuntimeReload,
  configChangesNeedWallpaperReload,
} from '../uiBagOwnership'
import { snapshotConfigNavScroll } from './configNavPersistence'

interface ConfigI18n {
  configEmpty: string
  configSaved: string
  bangumiCredentialMissing: string
  savingConfig: string
  configSaveFailed: string
  /** When some sections already committed before a later step failed */
  partialSaveWarning: string
  librarySourceSaveFailed: string
  librarySourceSaved: string
  moduleVisibilitySaved: string
  hitokotoSaved: string
  reportSettingsSaved: string
  permissionsSaveFailed: string
  permissionsSaved: string
  federationPolicySaved: string
  savedSuccess: string
  savedSuccessRuntimeReload: string
  savedSuccessHardReload: string
  hardReloadPreparing: string
  refreshing: string
}

export function useConfigSave(args: {
  config: Config | null
  initialConfig: Config | null
  setInitialConfig: (c: Config | null) => void
  setSaving: (v: boolean) => void
  notifyDirtyState: (dirty: boolean) => void
  showMessage: ShowMessage
  t: { config: ConfigI18n; errors: { networkError: string } }
  isAdmin: boolean
  userId?: number
  librarySourceDraft: LibrarySourcePreferences
  savedLibrarySourcePreferences: LibrarySourcePreferences
  setLibrarySourceDraft: Dispatch<SetStateAction<LibrarySourcePreferences>>
  setSavedLibrarySourcePreferences: Dispatch<
    SetStateAction<LibrarySourcePreferences>
  >
  setLibrarySourceSaveRevision: Dispatch<SetStateAction<number>>
  moduleVisibilityDraft: ModuleVisibilityPreferences
  savedModuleVisibilityPreferences: ModuleVisibilityPreferences
  setModuleVisibilityDraft: Dispatch<
    SetStateAction<ModuleVisibilityPreferences>
  >
  setSavedModuleVisibilityPreferences: Dispatch<
    SetStateAction<ModuleVisibilityPreferences>
  >
  hitokotoDraft: HitokotoConfig
  savedHitokotoConfig: HitokotoConfig
  setHitokotoDraft: Dispatch<SetStateAction<HitokotoConfig>>
  setSavedHitokotoConfig: Dispatch<SetStateAction<HitokotoConfig>>
  reportSettingsDraft: ReportSettings
  savedReportSettings: ReportSettings
  setReportSettingsDraft: Dispatch<SetStateAction<ReportSettings>>
  setSavedReportSettings: Dispatch<SetStateAction<ReportSettings>>
  permissionConfig: PermissionConfigValues
  savedPermissionConfig: PermissionConfigValues
  setSavedPermissionConfig: Dispatch<SetStateAction<PermissionConfigValues>>
  notificationDraft: NotificationPreferences
  savedNotificationPreferences: NotificationPreferences
  setNotificationDraft: Dispatch<SetStateAction<NotificationPreferences>>
  setSavedNotificationPreferences: Dispatch<
    SetStateAction<NotificationPreferences>
  >
  oauthDraft: OAuthSettings
  savedOAuthSettings: OAuthSettings
  setOAuthDraft: Dispatch<SetStateAction<OAuthSettings>>
  setSavedOAuthSettings: Dispatch<SetStateAction<OAuthSettings>>
  federationPolicyDraft: FederationPolicyDraft
  savedFederationPolicy: FederationPolicyDraft
  setSavedFederationPolicy: Dispatch<SetStateAction<FederationPolicyDraft>>
  favorites: string[]
  savedFavorites: string[]
  setSavedFavorites: Dispatch<SetStateAction<string[]>>
}) {
  const {
    config,
    initialConfig,
    setInitialConfig,
    setSaving,
    notifyDirtyState,
    showMessage,
    t,
    isAdmin,
    userId,
    librarySourceDraft,
    savedLibrarySourcePreferences,
    setLibrarySourceDraft,
    setSavedLibrarySourcePreferences,
    setLibrarySourceSaveRevision,
    moduleVisibilityDraft,
    savedModuleVisibilityPreferences,
    setModuleVisibilityDraft,
    setSavedModuleVisibilityPreferences,
    hitokotoDraft,
    savedHitokotoConfig,
    setHitokotoDraft,
    setSavedHitokotoConfig,
    reportSettingsDraft,
    savedReportSettings,
    setReportSettingsDraft,
    setSavedReportSettings,
    permissionConfig,
    savedPermissionConfig,
    setSavedPermissionConfig,
    notificationDraft,
    savedNotificationPreferences,
    setNotificationDraft,
    setSavedNotificationPreferences,
    oauthDraft,
    savedOAuthSettings,
    setOAuthDraft,
    setSavedOAuthSettings,
    federationPolicyDraft,
    savedFederationPolicy,
    setSavedFederationPolicy,
    favorites,
    savedFavorites,
    setSavedFavorites,
  } = args

  return useCallback(async () => {
    if (!config) {
      window.dispatchEvent(
        new CustomEvent('config-save-result', {
          detail: { success: false, message: t.config.configEmpty },
        }),
      )
      return
    }

    const hasConfigChanges =
      Boolean(initialConfig) && !deepEqual(config, initialConfig)
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
    const hasPermissionChanges = !deepEqual(
      permissionConfig,
      savedPermissionConfig,
    )
    const hasNotificationChanges = !areNotificationPreferencesEqual(
      notificationDraft,
      savedNotificationPreferences,
    )
    const hasOAuthChanges = !areOAuthSettingsEqual(
      oauthDraft,
      savedOAuthSettings,
    )
    const hasFederationChanges =
      isAdmin &&
      !areFederationPoliciesEqual(federationPolicyDraft, savedFederationPolicy)
    const hasFavoriteChanges = !deepEqual(favorites, savedFavorites)

    if (
      !hasConfigChanges &&
      !hasLibrarySourceChanges &&
      !hasModuleVisibilityChanges &&
      !hasHitokotoChanges &&
      !hasReportSettingsChanges &&
      !hasPermissionChanges &&
      !hasNotificationChanges &&
      !hasOAuthChanges &&
      !hasFederationChanges &&
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

    type PendingClean = () => void | Promise<void>
    const pendingClean: PendingClean[] = []

    try {
      let resultMessage = t.config.configSaved

      if (hasConfigChanges) {
        await getCSRFToken(true)
        const result = await updateConfig(config)
        if (result?.success === false) {
          throw new Error(result.message || t.config.configSaveFailed)
        }
        resultMessage = result.message || t.config.configSaved
        const snapshot = JSON.parse(JSON.stringify(config)) as Config
        pendingClean.push(() => setInitialConfig(snapshot))
      }

      if (hasLibrarySourceChanges) {
        const saved =
          await apiService.put<SaveLibrarySourcePreferencesResponse>(
            '/library/preferences',
            librarySourceDraft,
          )
        if (!saved.success) {
          throw new Error(saved.message || t.config.librarySourceSaveFailed)
        }
        const preferences = normalizeLibraryPreferences(saved.preferences)
        pendingClean.push(() => {
          setLibrarySourceDraft(preferences)
          setSavedLibrarySourcePreferences(preferences)
          setLibrarySourceSaveRevision((revision) => revision + 1)
          clearLibraryDataCache()
          dispatchLibraryPreferencesUpdated({
            categories: preferences.categories,
            layout: preferences.layout,
          })
        })
        if (!hasConfigChanges) resultMessage = t.config.librarySourceSaved
      }

      if (hasModuleVisibilityChanges) {
        const raw = await updateModuleVisibilityPreferences(
          moduleVisibilityDraft,
        )
        const preferences = normalizeModuleVisibilityPreferences(raw)
        pendingClean.push(() => {
          setModuleVisibilityDraft(preferences)
          setSavedModuleVisibilityPreferences(preferences)
          dispatchModuleVisibilityPreferencesUpdated(preferences)
        })
        if (!hasConfigChanges) resultMessage = t.config.moduleVisibilitySaved
      }

      if (hasHitokotoChanges) {
        const saved = await updateHitokotoConfig(hitokotoDraft)
        pendingClean.push(() => {
          setHitokotoDraft(saved)
          setSavedHitokotoConfig(saved)
        })
        if (!hasConfigChanges) resultMessage = t.config.hitokotoSaved
      }

      if (hasReportSettingsChanges) {
        const saved = await updateReportSettings(reportSettingsDraft)
        pendingClean.push(() => {
          setReportSettingsDraft(saved)
          setSavedReportSettings(saved)
        })
        if (!hasConfigChanges) resultMessage = t.config.reportSettingsSaved
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
        const nextPerm = { ...permissionConfig }
        pendingClean.push(async () => {
          setSavedPermissionConfig(nextPerm)
          const { TappRuntime } =
            await import('../../../tapp/runtime/TappRuntime')
          await TappRuntime.getInstance().refreshPermissionGrants()
        })
        if (!hasConfigChanges) resultMessage = t.config.permissionsSaved
      }

      if (hasNotificationChanges) {
        const saved = await notificationPreferencesApi.update(
          notificationDraft,
          userId,
        )
        const normalized = cloneNotificationPreferences(saved)
        pendingClean.push(() => {
          setNotificationDraft(normalized)
          setSavedNotificationPreferences(
            cloneNotificationPreferences(normalized),
          )
        })
      }

      if (hasOAuthChanges) {
        const saved = await updateOAuthSettings(oauthDraft)
        const cloned = cloneOAuthSettings(saved)
        pendingClean.push(() => {
          setOAuthDraft(cloned)
          setSavedOAuthSettings(cloneOAuthSettings(saved))
        })
      }

      if (hasFederationChanges) {
        await federationApi.updateTrustPolicy(
          federationPolicyToUpdateRequest(federationPolicyDraft),
        )
        const nextFed = { ...federationPolicyDraft }
        pendingClean.push(() => setSavedFederationPolicy(nextFed))
        if (!hasConfigChanges) resultMessage = t.config.federationPolicySaved
      }

      if (hasFavoriteChanges) {
        const nextFav = [...favorites]
        pendingClean.push(() => {
          localStorage.setItem('config_favorites', JSON.stringify(nextFav))
          setSavedFavorites(nextFav)
        })
      }

      for (const apply of pendingClean) {
        await apply()
      }

      notifyDirtyState(false)
      window.dispatchEvent(
        new CustomEvent('config-save-result', {
          detail: { success: true, message: resultMessage },
        }),
      )

      if (!hasConfigChanges) {
        showMessage(resultMessage, 'success', 3000)
        return
      }

      const needHardReload =
        Boolean(initialConfig) &&
        configChangesNeedHardReload(config, initialConfig!, deepEqual)
      const needRuntimeReload =
        Boolean(initialConfig) &&
        configChangesNeedRuntimeReload(config, initialConfig!)
      const needWallpaperReload =
        Boolean(initialConfig) &&
        configChangesNeedWallpaperReload(config, initialConfig!)
      const needMetadataReload =
        Boolean(initialConfig) &&
        configChangesNeedMetadataReload(config, initialConfig!)
      const needFooterReload =
        Boolean(initialConfig) &&
        configChangesNeedFooterReload(config, initialConfig!)
      const needPwaReload =
        Boolean(initialConfig) &&
        configChangesNeedPwaReload(config, initialConfig!)
      const needPlatformsCachePurge =
        Boolean(initialConfig) &&
        configChangesNeedPlatformsCacheInvalidation(
          config,
          initialConfig!,
          deepEqual,
        )

      // Soft side-effects (no full-page reload): wallpaper, library cache, etc.
      if (needWallpaperReload) {
        clearDedupCache(`${API_URL}/api/config/ui`)
        // Bust 1s lastLoadResult debounce so soft reload is not a no-op
        void import('../../../hooks/useWallpaper').then((m) => {
          m.invalidateWallpaperLoadCache()
          window.dispatchEvent(new CustomEvent('wallpaperConfigChanged'))
        })
      }
      if (needMetadataReload) {
        clearDedupCache(`${API_URL}/api/config/metadata`)
        void import('../../../utils/siteMetadata').then((m) => {
          void m.refreshSiteMetadata()
        })
      }
      if (needFooterReload) {
        clearDedupCache(`${API_URL}/api/config/ui`)
        window.dispatchEvent(new CustomEvent('footerConfigChanged'))
      }
      if (needPwaReload) {
        clearDedupCache(`${API_URL}/api/config/ui`)
        const raw = config.ui_config?.config_fields?.find(
          (f) => f.key === 'pwa_enabled',
        )?.value
        const enabled = raw !== 'false' && raw !== '0'
        void import('../../../utils/pwa').then((m) => {
          void m.applyPwaEnabled(enabled)
        })
      }
      if (needPlatformsCachePurge) {
        clearLibraryDataCache()
      }

      // Proxy / API mirrors: backend hot-reload only — no location.reload.
      if (needRuntimeReload && !needHardReload) {
        try {
          await getCSRFToken(true)
          await reloadSystemConfig()
        } catch {
          // Config is already persisted; outbound clients may lag until next restart.
        }
        showMessage(t.config.savedSuccessRuntimeReload, 'success', 4000)
        return
      }

      if (!needHardReload) {
        // AI / platforms / auto_fetch / pure UI bags: toast only (side-effects above).
        showMessage(t.config.savedSuccess, 'success', 3000)
        return
      }

      // Hard-reload path: sticky toast that keeps “full page reload” intent
      // (do not overwrite with generic savedSuccess — that hid the reload cue).
      showMessage(t.config.hardReloadPreparing, 'success', 0)

      try {
        await getCSRFToken(true)
        await reloadSystemConfig()
      } catch {
        // Config already persisted; still reload so UI picks up full state.
      }
      showMessage(t.config.savedSuccessHardReload, 'success', 0)
      snapshotConfigNavScroll()
      setTimeout(() => {
        window.location.reload()
      }, 2000)
    } catch (error) {
      // Some sections may already have succeeded (pendingClean filled).
      // Commit those so UI dirty flags only reflect remaining unsaved drafts.
      const applied: PendingClean[] = []
      for (const apply of pendingClean) {
        try {
          await apply()
          applied.push(apply)
        } catch {
          /* best-effort */
        }
      }
      const partial = applied.length > 0
      const detail = userFacingError(error, t.errors.networkError)
      const errorMsg = partial
        ? `${t.config.partialSaveWarning}: ${detail}`
        : `${t.config.configSaveFailed}: ${detail}`
      showMessage(errorMsg, partial ? 'warning' : 'error', 0)
      window.dispatchEvent(
        new CustomEvent('config-save-result', {
          detail: {
            success: false,
            partial,
            message: errorMsg,
          },
        }),
      )
    } finally {
      setSaving(false)
    }
  }, [
    config,
    initialConfig,
    setInitialConfig,
    setSaving,
    notifyDirtyState,
    showMessage,
    t,
    isAdmin,
    userId,
    librarySourceDraft,
    savedLibrarySourcePreferences,
    setLibrarySourceDraft,
    setSavedLibrarySourcePreferences,
    setLibrarySourceSaveRevision,
    moduleVisibilityDraft,
    savedModuleVisibilityPreferences,
    setModuleVisibilityDraft,
    setSavedModuleVisibilityPreferences,
    hitokotoDraft,
    savedHitokotoConfig,
    setHitokotoDraft,
    setSavedHitokotoConfig,
    reportSettingsDraft,
    savedReportSettings,
    setReportSettingsDraft,
    setSavedReportSettings,
    permissionConfig,
    savedPermissionConfig,
    setSavedPermissionConfig,
    notificationDraft,
    savedNotificationPreferences,
    setNotificationDraft,
    setSavedNotificationPreferences,
    oauthDraft,
    savedOAuthSettings,
    setOAuthDraft,
    setSavedOAuthSettings,
    federationPolicyDraft,
    savedFederationPolicy,
    setSavedFederationPolicy,
    favorites,
    savedFavorites,
    setSavedFavorites,
  ])
}
