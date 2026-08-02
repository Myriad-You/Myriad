import type { NotificationPreferences } from '../../../services/notificationPreferencesApi'
import type { ModuleVisibilityPreferences } from '../../../utils/moduleVisibility'
import type { OAuthSettings } from '../../../utils/oauthSettings'
import type { HitokotoConfig } from '../../../utils/quote'
import type { ReportSettings } from '../../../utils/reportSettings'
import type { FederationPolicyDraft } from '../FederationConfigSection'
import type { LibrarySourcePreferences } from '../ModuleConfigSection'
import type { PermissionConfigValues } from '../PermissionsConfigSection'
import type { Config } from './types'
import { useEffect, useMemo } from 'react'
import {
  areNotificationPreferencesEqual,

} from '../../../services/notificationPreferencesApi'
import { deepEqual } from '../../../utils/deepEqual'
import {
  areModuleVisibilityPreferencesEqual,

} from '../../../utils/moduleVisibility'
import {
  areOAuthSettingsEqual,

} from '../../../utils/oauthSettings'
import {
  areHitokotoConfigsEqual,

} from '../../../utils/quote'
import {
  areReportSettingsEqual,

} from '../../../utils/reportSettings'
import { areFederationPoliciesEqual } from '../FederationConfigSection'
import { areLibrarySourcePreferencesEqual } from '../ModuleConfigSection'

export function useConfigDirty(args: {
  config: Config | null
  initialConfig: Config | null
  librarySourceDraft: LibrarySourcePreferences
  savedLibrarySourcePreferences: LibrarySourcePreferences
  moduleVisibilityDraft: ModuleVisibilityPreferences
  savedModuleVisibilityPreferences: ModuleVisibilityPreferences
  hitokotoDraft: HitokotoConfig
  savedHitokotoConfig: HitokotoConfig
  reportSettingsDraft: ReportSettings
  savedReportSettings: ReportSettings
  permissionConfig: PermissionConfigValues
  savedPermissionConfig: PermissionConfigValues
  notificationDraft: NotificationPreferences
  savedNotificationPreferences: NotificationPreferences
  oauthDraft: OAuthSettings
  savedOAuthSettings: OAuthSettings
  federationPolicyDraft: FederationPolicyDraft
  savedFederationPolicy: FederationPolicyDraft
  isAdmin: boolean
  favorites: string[]
  savedFavorites: string[]
  notifyDirtyState: (dirty: boolean) => void
}) {
  const {
    config,
    initialConfig,
    librarySourceDraft,
    savedLibrarySourcePreferences,
    moduleVisibilityDraft,
    savedModuleVisibilityPreferences,
    hitokotoDraft,
    savedHitokotoConfig,
    reportSettingsDraft,
    savedReportSettings,
    permissionConfig,
    savedPermissionConfig,
    notificationDraft,
    savedNotificationPreferences,
    oauthDraft,
    savedOAuthSettings,
    federationPolicyDraft,
    savedFederationPolicy,
    isAdmin,
    favorites,
    savedFavorites,
    notifyDirtyState,
  } = args

  const isBaseConfigDirty = useMemo(() => {
    if (!config || !initialConfig) return false
    return !deepEqual(config, initialConfig)
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
    () => !deepEqual(permissionConfig, savedPermissionConfig),
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

  const isFederationDirty = useMemo(
    () =>
      isAdmin &&
      !areFederationPoliciesEqual(federationPolicyDraft, savedFederationPolicy),
    [isAdmin, federationPolicyDraft, savedFederationPolicy],
  )

  const isFavoritesDirty = useMemo(
    () => !deepEqual(favorites, savedFavorites),
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
    isFederationDirty ||
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

  return {
    isConfigDirty,
    isLibrarySourceDirty,
    isBaseConfigDirty,
  }
}
