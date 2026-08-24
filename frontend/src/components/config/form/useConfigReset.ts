import type { Dispatch, SetStateAction } from 'react'
import type { NotificationPreferences } from '../../../services/notificationPreferencesApi'
import type { ModuleVisibilityPreferences } from '../../../utils/moduleVisibility'
import type { OAuthSettings } from '../../../utils/oauthSettings'
import type { HitokotoConfig } from '../../../utils/quote'
import type { ReportSettings } from '../../../utils/reportSettings'
import type { FederationPolicyDraft } from '../FederationConfigSection'
import type { LibrarySourcePreferences } from '../ModuleConfigSection'
import type { PermissionConfigValues } from '../PermissionsConfigSection'
import type { Config, ConfigField, ShowMessage } from './types'
import { useCallback } from 'react'
import { userFacingError } from '../../../utils/userFacingError'
import {
  fetchConfig,
  updateConfig,
  updatePermissionsConfig,
} from '../../../lib/api'
import { federationApi } from '../../../services/federationApi'
import notificationPreferencesApi, {
  cloneNotificationPreferences,
  DEFAULT_NOTIFICATION_PREFERENCES,

} from '../../../services/notificationPreferencesApi'
import { getCSRFToken } from '../../../utils/csrf'
import { dispatchLibraryPreferencesUpdated } from '../../../utils/libraryPreferences'
import {
  DEFAULT_MODULE_VISIBILITY_PREFERENCES,
  dispatchModuleVisibilityPreferencesUpdated,

  normalizeModuleVisibilityPreferences,
  updateModuleVisibilityPreferences,
} from '../../../utils/moduleVisibility'
import {
  cloneOAuthSettings,
  DEFAULT_OAUTH_SETTINGS,

  updateOAuthSettings,
} from '../../../utils/oauthSettings'
import {
  DEFAULT_HITOKOTO_CONFIG,

  updateHitokotoConfig,
} from '../../../utils/quote'
import {
  DEFAULT_REPORT_SETTINGS,

  updateReportSettings,
} from '../../../utils/reportSettings'
import { clearLibraryDataCache } from '../../../utils/requestDedup'
import {
  DEFAULT_FEDERATION_POLICY,

  federationPolicyToUpdateRequest,
} from '../FederationConfigSection'
import {
  DEFAULT_LIBRARY_SOURCE_PREFERENCES,

  normalizeLibraryPreferences,
} from '../ModuleConfigSection'
import {
  ADVANCED_RESET_KEYS,
  ALL_OWNED_UI_BAG_KEYS,
  MODULE_UI_RESET_KEYS,
  PLATFORMS_UI_RESET_KEYS,
  UI_RESET_KEYS,
} from '../uiBagOwnership'
import {
  defaultAiFieldValue,
  defaultTripoFieldValue,
  defaultUiFieldValue,
  mapConfigFields,
} from './defaultFieldValues'
import {
  DEFAULT_AUTO_FETCH_CONFIG,
  DEFAULT_PERMISSION_CONFIG,
} from './defaults'

interface ResetI18n {
  resettingConfig: string
  savingDefault: string
  resetFailed: string
  configReset: string
  librarySourceSaveFailed: string
  permissionsSaveFailed: string
  resetCurrentPageNone: string
  resetCurrentPageDone: string
}

export function useConfigReset(args: {
  config: Config | null
  setConfig: (c: Config | null) => void
  setInitialConfig: (c: Config | null) => void
  setSaving: (v: boolean) => void
  setPlatformFocus: (v: string | null) => void
  notifyDirtyState: (dirty: boolean) => void
  showMessage: ShowMessage
  t: { config: ResetI18n; errors: { unknown: string } }
  activeSection: string
  isAdmin: boolean
  userId?: number
  oauthDraft: OAuthSettings
  /** Apply side-draft clean state after per-section reset */
  side: {
    setLibrarySourceDraft: Dispatch<SetStateAction<LibrarySourcePreferences>>
    setSavedLibrarySourcePreferences: Dispatch<
      SetStateAction<LibrarySourcePreferences>
    >
    setLibrarySourceSaveRevision: Dispatch<SetStateAction<number>>
    setModuleVisibilityDraft: Dispatch<
      SetStateAction<ModuleVisibilityPreferences>
    >
    setSavedModuleVisibilityPreferences: Dispatch<
      SetStateAction<ModuleVisibilityPreferences>
    >
    setHitokotoDraft: Dispatch<SetStateAction<HitokotoConfig>>
    setSavedHitokotoConfig: Dispatch<SetStateAction<HitokotoConfig>>
    setReportSettingsDraft: Dispatch<SetStateAction<ReportSettings>>
    setSavedReportSettings: Dispatch<SetStateAction<ReportSettings>>
    setOAuthDraft: Dispatch<SetStateAction<OAuthSettings>>
    setSavedOAuthSettings: Dispatch<SetStateAction<OAuthSettings>>
    setFederationPolicyDraft: Dispatch<SetStateAction<FederationPolicyDraft>>
    setSavedFederationPolicy: Dispatch<SetStateAction<FederationPolicyDraft>>
    setPermissionConfig: Dispatch<SetStateAction<PermissionConfigValues>>
    setSavedPermissionConfig: Dispatch<SetStateAction<PermissionConfigValues>>
    setNotificationDraft: Dispatch<SetStateAction<NotificationPreferences>>
    setSavedNotificationPreferences: Dispatch<
      SetStateAction<NotificationPreferences>
    >
  }
}) {
  const {
    config,
    setConfig,
    setInitialConfig,
    setSaving,
    setPlatformFocus,
    notifyDirtyState,
    showMessage,
    t,
    activeSection,
    isAdmin,
    userId,
    oauthDraft,
    side,
  } = args

  const handleReset = useCallback(async () => {
    showMessage(t.config.resettingConfig, 'info', 0)
    setSaving(true)
    try {
      const data = await fetchConfig()
      const clearedData = {
        ...data,
        auto_fetch: DEFAULT_AUTO_FETCH_CONFIG,
        platforms: data.platforms.map(
          (platform: Config['platforms'][number]) => ({
            ...platform,
            enabled: false,
            has_token: false,
            config_fields: platform.config_fields.map((field) => ({
              ...field,
              value: '',
            })),
          }),
        ),
        ai_config: {
          ...data.ai_config,
          config_fields: data.ai_config.config_fields.map(
            (field: ConfigField) => ({
              ...field,
              value: defaultAiFieldValue(field.key),
            }),
          ),
        },
        tripo_config: {
          ...data.tripo_config,
          config_fields: data.tripo_config.config_fields.map(
            (field: ConfigField) => ({
              ...field,
              value: defaultTripoFieldValue(field.key),
            }),
          ),
        },
        ui_config: {
          ...data.ui_config,
          config_fields: mapConfigFields(
            data.ui_config.config_fields,
            defaultUiFieldValue,
            new Set(ALL_OWNED_UI_BAG_KEYS),
          ),
        },
      }

      setConfig(clearedData)
      await new Promise((resolve) => setTimeout(resolve, 200))
      showMessage(t.config.savingDefault, 'info', 0)
      await getCSRFToken(true)

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
      const errorMsg = `${t.config.resetFailed}${userFacingError(error, t.errors.unknown)}`
      showMessage(errorMsg, 'error', 0)
      window.dispatchEvent(
        new CustomEvent('config-reset-result', {
          detail: { success: false, message: errorMsg },
        }),
      )
    } finally {
      setSaving(false)
    }
  }, [setConfig, setInitialConfig, setSaving, showMessage, t])

  const handleResetCurrentPage = useCallback(async () => {
    if (!config) return
    const section = activeSection
    showMessage(t.config.resettingConfig, 'info', 0)
    setSaving(true)

    try {
      await getCSRFToken(true)

      if (section === 'platforms') {
        const platformUiKeys = new Set(PLATFORMS_UI_RESET_KEYS)
        const next = {
          ...config,
          auto_fetch: DEFAULT_AUTO_FETCH_CONFIG,
          platforms: config.platforms.map((platform) => ({
            ...platform,
            enabled: false,
            has_token: false,
            config_fields: platform.config_fields.map((field) => ({
              ...field,
              value: '',
            })),
          })),
          ui_config: {
            ...config.ui_config,
            config_fields: mapConfigFields(
              config.ui_config.config_fields,
              defaultUiFieldValue,
              platformUiKeys,
            ),
          },
        }
        const result = await updateConfig(next)
        if (result?.success === false) {
          throw new Error(result.message || t.config.resetFailed)
        }
        setConfig(next)
        setInitialConfig(JSON.parse(JSON.stringify(next)))
        setPlatformFocus(null)
      } else if (section === 'ai') {
        const next = {
          ...config,
          ai_config: {
            ...config.ai_config,
            config_fields: mapConfigFields(
              config.ai_config.config_fields,
              defaultAiFieldValue,
            ),
          },
        }
        const result = await updateConfig(next)
        if (result?.success === false) {
          throw new Error(result.message || t.config.resetFailed)
        }
        setConfig(next)
        setInitialConfig(JSON.parse(JSON.stringify(next)))
      } else if (section === 'tripo') {
        const next = {
          ...config,
          tripo_config: {
            ...config.tripo_config,
            config_fields: mapConfigFields(
              config.tripo_config.config_fields,
              defaultTripoFieldValue,
            ),
          },
        }
        const result = await updateConfig(next)
        if (result?.success === false) {
          throw new Error(result.message || t.config.resetFailed)
        }
        setConfig(next)
        setInitialConfig(JSON.parse(JSON.stringify(next)))
      } else if (section === 'basic') {
        const uiKeys = new Set(UI_RESET_KEYS)
        const next = {
          ...config,
          ui_config: {
            ...config.ui_config,
            config_fields: mapConfigFields(
              config.ui_config.config_fields,
              defaultUiFieldValue,
              uiKeys,
            ),
          },
        }
        const result = await updateConfig(next)
        if (result?.success === false) {
          throw new Error(result.message || t.config.resetFailed)
        }
        setConfig(next)
        setInitialConfig(JSON.parse(JSON.stringify(next)))
      } else if (section === 'advanced') {
        const advancedKeys = new Set(ADVANCED_RESET_KEYS)
        const next = {
          ...config,
          ui_config: {
            ...config.ui_config,
            config_fields: mapConfigFields(
              config.ui_config.config_fields,
              defaultUiFieldValue,
              advancedKeys,
            ),
          },
        }
        const result = await updateConfig(next)
        if (result?.success === false) {
          throw new Error(result.message || t.config.resetFailed)
        }
        setConfig(next)
        setInitialConfig(JSON.parse(JSON.stringify(next)))
      } else if (section === 'modules') {
        const moduleUiKeys = new Set(MODULE_UI_RESET_KEYS)
        const nextConfig = {
          ...config,
          ui_config: {
            ...config.ui_config,
            config_fields: mapConfigFields(
              config.ui_config.config_fields,
              defaultUiFieldValue,
              moduleUiKeys,
            ),
          },
        }
        const result = await updateConfig(nextConfig)
        if (result?.success === false) {
          throw new Error(result.message || t.config.resetFailed)
        }
        setConfig(nextConfig)
        setInitialConfig(JSON.parse(JSON.stringify(nextConfig)))

        const lib = normalizeLibraryPreferences(
          DEFAULT_LIBRARY_SOURCE_PREFERENCES,
        )
        const { default: apiService } = await import('../../../services/api')
        const libSaved = await apiService.put<{
          success: boolean
          preferences?: unknown
          message?: string
        }>('/library/preferences', lib)
        if (!libSaved.success) {
          throw new Error(libSaved.message || t.config.librarySourceSaveFailed)
        }
        const libNorm = normalizeLibraryPreferences(
          libSaved.preferences as never,
        )
        side.setLibrarySourceDraft(libNorm)
        side.setSavedLibrarySourcePreferences(libNorm)
        side.setLibrarySourceSaveRevision((r) => r + 1)
        clearLibraryDataCache()
        dispatchLibraryPreferencesUpdated({
          categories: libNorm.categories,
          layout: libNorm.layout,
        })

        const vis = await updateModuleVisibilityPreferences(
          DEFAULT_MODULE_VISIBILITY_PREFERENCES,
        )
        const visNorm = normalizeModuleVisibilityPreferences(vis)
        side.setModuleVisibilityDraft(visNorm)
        side.setSavedModuleVisibilityPreferences(visNorm)
        dispatchModuleVisibilityPreferencesUpdated(visNorm)

        const hitokoto = await updateHitokotoConfig(DEFAULT_HITOKOTO_CONFIG)
        side.setHitokotoDraft(hitokoto)
        side.setSavedHitokotoConfig(hitokoto)

        const reports = await updateReportSettings(DEFAULT_REPORT_SETTINGS)
        side.setReportSettingsDraft(reports)
        side.setSavedReportSettings(reports)
      } else if (section === 'oauth' || section === 'users') {
        const next =
          section === 'users'
            ? {
                ...oauthDraft,
                allowLocalRegistration:
                  DEFAULT_OAUTH_SETTINGS.allowLocalRegistration,
                privateTappInstallCleanup:
                  DEFAULT_OAUTH_SETTINGS.privateTappInstallCleanup,
                privateTappInstallInactivityDays:
                  DEFAULT_OAUTH_SETTINGS.privateTappInstallInactivityDays,
              }
            : cloneOAuthSettings(DEFAULT_OAUTH_SETTINGS)
        const saved = await updateOAuthSettings(next)
        side.setOAuthDraft(cloneOAuthSettings(saved))
        side.setSavedOAuthSettings(cloneOAuthSettings(saved))
      } else if (section === 'federation') {
        if (!isAdmin) throw new Error(t.config.resetFailed)
        await federationApi.updateTrustPolicy(
          federationPolicyToUpdateRequest(DEFAULT_FEDERATION_POLICY),
        )
        side.setFederationPolicyDraft({ ...DEFAULT_FEDERATION_POLICY })
        side.setSavedFederationPolicy({ ...DEFAULT_FEDERATION_POLICY })
      } else if (section === 'permissions') {
        await getCSRFToken(true)
        const response = await updatePermissionsConfig({
          ...DEFAULT_PERMISSION_CONFIG,
        })
        if (!response.success) {
          throw new Error(response.message || t.config.permissionsSaveFailed)
        }
        side.setPermissionConfig({ ...DEFAULT_PERMISSION_CONFIG })
        side.setSavedPermissionConfig({ ...DEFAULT_PERMISSION_CONFIG })
        const { TappRuntime } =
          await import('../../../tapp/runtime/TappRuntime')
        await TappRuntime.getInstance().refreshPermissionGrants()
      } else if (section === 'notifications') {
        const saved = await notificationPreferencesApi.update(
          DEFAULT_NOTIFICATION_PREFERENCES,
          userId,
        )
        const normalized = cloneNotificationPreferences(saved)
        side.setNotificationDraft(normalized)
        side.setSavedNotificationPreferences(
          cloneNotificationPreferences(normalized),
        )
      } else {
        showMessage(t.config.resetCurrentPageNone, 'info')
        return
      }

      notifyDirtyState(false)
      showMessage(
        t.config.resetCurrentPageDone,
        'success',
        3000,
      )
    } catch (error) {
      const errorMsg = `${t.config.resetFailed}${userFacingError(error, t.errors.unknown)}`
      showMessage(errorMsg, 'error', 0)
    } finally {
      setSaving(false)
    }
  }, [
    activeSection,
    config,
    isAdmin,
    notifyDirtyState,
    oauthDraft,
    setConfig,
    setInitialConfig,
    setPlatformFocus,
    setSaving,
    showMessage,
    side,
    t,
    userId,
  ])

  return { handleReset, handleResetCurrentPage }
}
