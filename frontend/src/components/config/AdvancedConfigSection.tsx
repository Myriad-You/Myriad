import type { SettingsRestorePreview } from '../../services/configApi'

import {
  FaGlobe,
  FaSave,
  FaTimes,
  LuDownload,
  LuMemoryStick,
  LuPin,
  LuRefreshCw,
  LuUpload,
} from '@lib/icons'
import React, { useCallback, useEffect, useRef, useState } from 'react'
import { useConfigI18n as useI18n } from '../../contexts/I18nContext'
import {
  clearRestoreNotice,
  EMPTY_RESTORE_NOTICE,
  formatRestoreNotice,
  hasRestoreNotice,
  readRestoreNotice,
  stashRestoreNotice,
  summarizeKeys,
} from '../../lib/settingsRestoreNotice'
import {
  fetchSettingsBackup,
  previewSettingsBackup,
  restoreSettingsBackup,
  updateConfig,
} from '../../services/configApi'

import { purgeFrontendCachesAndReload } from '../../utils/frontendCachePurge'
import { userFacingError } from '../../utils/userFacingError'
import {
  ButtonItem,
  InputItem,
  SettingGroup,
  SettingsButton,
  SettingSection,
  SwitchItem,
  useSettingGuide,
} from '../settings'
import McpConfigPanel from './McpConfigPanel'
import RuntimeDiagnostics from './RuntimeDiagnostics'

interface UiConfigField {
  key: string
  value: string
}

interface AdvancedConfigSectionProps {
  onReset: () => void
  title: string
  icon: React.ReactNode
  description: string
  sectionId?: string
  uiConfigFields: UiConfigField[]
  updateUiFieldValue: (key: string, value: string) => void
  onMessage?: (
    msg: string,
    type?: 'success' | 'error' | 'warning' | 'info',
  ) => void
}

const SETTINGS_BACKUP_FORMAT = 'myriad-settings-backup'
const MIN_SETTINGS_BACKUP_VERSION = 1
const SETTINGS_BACKUP_VERSION = 2

interface ClientPreferenceDescriptor {
  key: string
  schemaVersion: number
}

interface ClientPreferenceBackupEntry {
  schema_version: number
  value: string
}

interface ClientPreferenceRestorePlan {
  values: Record<string, string>
  restoreCount: number
  preserveCount: number
  ignoredKeys: string[]
  invalidKeys: string[]
}

// only registry of browser prefs; add/remove local settings here
const CLIENT_PREFERENCE_REGISTRY: ClientPreferenceDescriptor[] = [
  { key: 'theme', schemaVersion: 1 },
  { key: 'locale', schemaVersion: 1 },
  { key: 'animation-preference', schemaVersion: 1 },
  { key: 'config_favorites', schemaVersion: 1 },
  { key: 'phantasiai_tts_settings', schemaVersion: 1 },
  { key: 'phantasi-reader-settings', schemaVersion: 1 },
]

function collectClientPreferences(): Record<
  string,
  ClientPreferenceBackupEntry
> {
  return Object.fromEntries(
    CLIENT_PREFERENCE_REGISTRY.flatMap((descriptor) => {
      const value = localStorage.getItem(descriptor.key)
      return value === null
        ? []
        : [
            [
              descriptor.key,
              { schema_version: descriptor.schemaVersion, value },
            ],
          ]
    }),
  )
}

function planClientPreferenceRestore(
  data: unknown,
): ClientPreferenceRestorePlan {
  const preferences =
    data && typeof data === 'object' && !Array.isArray(data)
      ? (data as Record<string, unknown>)
      : {}
  const registry = new Map(
    CLIENT_PREFERENCE_REGISTRY.map((descriptor) => [
      descriptor.key,
      descriptor,
    ]),
  )
  const values: Record<string, string> = {}
  const ignoredKeys = Iterator.from(
    new Set(Object.keys(preferences)).difference(new Set(registry.keys())),
  ).toArray()
  const invalidKeys: string[] = []

  for (const descriptor of CLIENT_PREFERENCE_REGISTRY) {
    const raw = preferences[descriptor.key]
    if (typeof raw === 'string') {
      // v1 client prefs are flat strings
      values[descriptor.key] = raw
      continue
    }
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) continue
    const entry = raw as Partial<ClientPreferenceBackupEntry>
    if (
      typeof entry.value !== 'string' ||
      entry.schema_version !== descriptor.schemaVersion
    ) {
      invalidKeys.push(descriptor.key)
      continue
    }
    values[descriptor.key] = entry.value
  }

  return {
    values,
    restoreCount: Object.keys(values).length,
    preserveCount:
      CLIENT_PREFERENCE_REGISTRY.length - Object.keys(values).length,
    ignoredKeys,
    invalidKeys,
  }
}

function restoreClientPreferences(plan: ClientPreferenceRestorePlan | null) {
  if (!plan) return
  for (const [key, value] of Object.entries(plan.values)) {
    localStorage.setItem(key, value)
  }
}

function combineRestorePreviews(
  backend: SettingsRestorePreview,
  client: ClientPreferenceRestorePlan,
): SettingsRestorePreview {
  return {
    ...backend,
    restore_count: backend.restore_count + client.restoreCount,
    preserve_count: backend.preserve_count + client.preserveCount,
    ignored_count: backend.ignored_count + client.ignoredKeys.length,
    invalid_count: backend.invalid_count + client.invalidKeys.length,
    ignored_keys: [
      ...backend.ignored_keys,
      ...client.ignoredKeys.map((key) => `client:${key}`),
    ],
    invalid_keys: [
      ...backend.invalid_keys,
      ...client.invalidKeys.map((key) => `client:${key}`),
    ],
  }
}

function isVersionedSettingsBackup(
  data: unknown,
): data is Record<string, unknown> {
  if (!data || typeof data !== 'object' || Array.isArray(data)) return false
  const backup = data as Record<string, unknown>
  return (
    backup.format === SETTINGS_BACKUP_FORMAT &&
    typeof backup.version === 'number' &&
    backup.version >= MIN_SETTINGS_BACKUP_VERSION &&
    backup.version <= SETTINGS_BACKUP_VERSION &&
    Array.isArray(backup.configurations) &&
    Boolean(backup.effective_config) &&
    Boolean(backup.user_preferences)
  )
}

function isLegacyConfig(data: unknown): data is Record<string, unknown> {
  if (!data || typeof data !== 'object' || Array.isArray(data)) return false
  const config = data as Record<string, unknown>
  return Boolean(
    config.platforms &&
    config.ai_config &&
    config.report_config &&
    config.ui_config,
  )
}

export const AdvancedConfigSection: React.FC<AdvancedConfigSectionProps> = ({
  onReset,
  title,
  icon,
  description,
  sectionId,
  uiConfigFields,
  updateUiFieldValue,
  onMessage,
}) => {
  const { t, format } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  // Carried across the reload that follows a restore; shown until dismissed.
  const [restoreNotice, setRestoreNotice] = useState(() => readRestoreNotice())
  const formattedRestoreNotice = hasRestoreNotice(restoreNotice)
    ? formatRestoreNotice(
        restoreNotice,
        {
          mediaTitle: t.config.restoreUnresolvedMediaTitle,
          skippedTitle: t.config.restoreSkippedSettingsTitle,
          more: t.config.restoreUnresolvedMediaMore,
          wallpaper: t.config.restoreUnresolvedMediaWallpaper,
          sticker: t.config.restoreUnresolvedMediaSticker,
        },
        format,
      )
    : null
  const dismissRestoreNotice = useCallback(() => {
    clearRestoreNotice()
    setRestoreNotice(EMPTY_RESTORE_NOTICE)
  }, [])
  const [resetConfirmOpen, setResetConfirmOpen] = useState(false)
  const [cachePurgeArmed, setCachePurgeArmed] = useState(false)
  const [cachePurgeLoading, setCachePurgeLoading] = useState(false)
  const cachePurgeArmTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )
  const [importConfirmOpen, setImportConfirmOpen] = useState(false)
  const [pendingImportData, setPendingImportData] = useState<unknown>(null)
  const [pendingClientRestore, setPendingClientRestore] =
    useState<ClientPreferenceRestorePlan | null>(null)
  const [restorePreview, setRestorePreview] =
    useState<SettingsRestorePreview | null>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const invalidPreviewKeys = summarizeKeys(restorePreview?.invalid_keys ?? [])
  const previewText = {
    restore: t.config.importPreviewRestore,
    preserve: t.config.importPreviewPreserve,
    ignored: t.config.importPreviewIgnored,
    migrated: t.config.importPreviewMigrated,
    invalid: t.config.importPreviewInvalid,
  }

  const getUiFieldValue = useCallback(
    (key: string) => uiConfigFields.find((f) => f.key === key)?.value || '',
    [uiConfigFields],
  )
  const isProxyEnabled = getUiFieldValue('proxy_enabled') === 'true'
  const isMemorySaverEnabled =
    getUiFieldValue('memory_saver_enabled') === 'true'
  const isPreciseLocationEnabled =
    getUiFieldValue('precise_location_enabled') === 'true'
  const closeImportConfirm = useCallback(() => {
    setImportConfirmOpen(false)
    setPendingImportData(null)
    setPendingClientRestore(null)
    setRestorePreview(null)
  }, [])

  const handleExport = useCallback(async () => {
    try {
      const backup = await fetchSettingsBackup()
      const json = JSON.stringify(
        {
          ...backup,
          client_preferences: collectClientPreferences(),
        },
        null,
        2,
      )
      const blob = new Blob([json], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `myriad-settings-backup-${new Date().toISOString().slice(0, 10)}.json`
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
      URL.revokeObjectURL(url)
      onMessage?.(t.config.exportConfigSuccess, 'success')
    } catch (error) {
      console.error('Export failed:', error)
      onMessage?.(
        userFacingError(error, t.config.exportConfigFailed),
        'error',
      )
    }
  }, [t, onMessage])

  const handleImportClick = useCallback(() => {
    fileInputRef.current?.click()
  }, [])

  const handleFileChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0]
      if (!file) return

      const reader = new FileReader()
      reader.onload = async (event) => {
        try {
          const data = JSON.parse(event.target?.result as string)
          if (!isVersionedSettingsBackup(data) && !isLegacyConfig(data)) {
            onMessage?.(t.config.importConfigInvalid, 'error')
            return
          }
          if (isVersionedSettingsBackup(data)) {
            const clientPlan = planClientPreferenceRestore(
              data.client_preferences,
            )
            const backendPreview = await previewSettingsBackup(data)
            setPendingClientRestore(clientPlan)
            setRestorePreview(
              combineRestorePreviews(backendPreview, clientPlan),
            )
          } else {
            setPendingClientRestore(null)
            setRestorePreview(null)
          }
          setPendingImportData(data)
          setImportConfirmOpen(true)
        } catch {
          onMessage?.(t.config.importConfigInvalid, 'error')
        }
      }
      reader.readAsText(file)
      e.target.value = ''
    },
    [t, onMessage],
  )

  const handleImportConfirm = useCallback(async () => {
    if (!pendingImportData) return
    setImportConfirmOpen(false)

    try {
      let attentionCount = 0
      if (isVersionedSettingsBackup(pendingImportData)) {
        const result = await restoreSettingsBackup(pendingImportData)
        restoreClientPreferences(pendingClientRestore)
        attentionCount =
          result.unresolved_media.length + result.skipped_settings.length
        // An empty notice also clears one left by an earlier restore.
        stashRestoreNotice({
          unresolvedMedia: result.unresolved_media,
          skippedSettings: result.skipped_settings,
        })
      } else {
        await updateConfig(pendingImportData)
      }
      if (attentionCount > 0) {
        onMessage?.(
          format(t.config.importConfigSuccessNeedsAttention, {
            count: attentionCount,
          }),
          'warning',
        )
      } else {
        onMessage?.(t.config.importConfigSuccess, 'success')
      }
      setTimeout(() => {
        window.location.reload()
      }, 2000)
    } catch (error) {
      console.error('Import failed:', error)
      onMessage?.(
        userFacingError(error, t.config.importConfigFailed),
        'error',
      )
    } finally {
      setPendingImportData(null)
      setPendingClientRestore(null)
      setRestorePreview(null)
    }
  }, [pendingClientRestore, pendingImportData, t, format, onMessage])

  const clearCachePurgeArmTimer = useCallback(() => {
    if (cachePurgeArmTimerRef.current != null) {
      clearTimeout(cachePurgeArmTimerRef.current)
      cachePurgeArmTimerRef.current = null
    }
  }, [])

  useEffect(() => () => clearCachePurgeArmTimer(), [clearCachePurgeArmTimer])

  const handleForceRefreshCacheClick = useCallback(async () => {
    if (cachePurgeLoading) return

    if (!cachePurgeArmed) {
      setCachePurgeArmed(true)
      clearCachePurgeArmTimer()
      cachePurgeArmTimerRef.current = setTimeout(() => {
        setCachePurgeArmed(false)
        cachePurgeArmTimerRef.current = null
      }, 4000)
      return
    }

    clearCachePurgeArmTimer()
    setCachePurgeArmed(false)
    setCachePurgeLoading(true)
    try {
      await purgeFrontendCachesAndReload(800)
      onMessage?.(t.config.forceRefreshFrontendCacheSuccess, 'success')
    } catch (error) {
      console.error('Frontend cache purge failed:', error)
      onMessage?.(
        userFacingError(error, t.config.forceRefreshFrontendCacheFailed),
        'error',
      )
      setCachePurgeLoading(false)
    }
  }, [
    cachePurgeArmed,
    cachePurgeLoading,
    clearCachePurgeArmTimer,
    t,
    onMessage,
  ])

  return (
    <SettingSection
      title={title}
      icon={icon}
      description={description}
      sectionId={sectionId}
    >
      <RuntimeDiagnostics onMessage={onMessage} />

      <McpConfigPanel onMessage={onMessage} />

      <SettingGroup
        title={t.config.memorySaverGroup}
        description={t.config.memorySaverGroupDesc}
        {...bindGuide('advanced.memorySaver', g.advanced.memorySaver)}
        icon={<LuMemoryStick />}
      >
        <SwitchItem
          itemKey="memory_saver_enabled"
          label={t.config.memorySaver}
          description={t.config.memorySaverHint}
          {...bindGuide('advanced.memorySaverEnable', g.advanced.memorySaverEnable)}
          value={isMemorySaverEnabled}
          onChange={(v) =>
            updateUiFieldValue('memory_saver_enabled', v.toString())
          }
          layout="horizontal"
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.preciseLocationGroup}
        description={t.config.preciseLocationGroupDesc}
        {...bindGuide('advanced.preciseLocation', g.advanced.preciseLocation)}
        icon={<LuPin />}
      >
        <SwitchItem
          itemKey="precise_location_enabled"
          label={t.config.preciseLocation}
          description={t.config.preciseLocationHint}
          {...bindGuide(
            'advanced.preciseLocationEnable',
            g.advanced.preciseLocationEnable,
          )}
          value={isPreciseLocationEnabled}
          onChange={(v) =>
            updateUiFieldValue('precise_location_enabled', v.toString())
          }
          layout="horizontal"
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.network}
        description={t.config.networkDesc}
        {...bindGuide('advanced.network', g.advanced.network)}
        icon={<FaGlobe />}
      >
        <SwitchItem
          itemKey="proxy_enabled"
          label={t.config.enableProxy}
          description={
            t.config.enableProxyHint
          }
          {...bindGuide('advanced.proxyEnable', g.advanced.proxyEnable)}
          value={isProxyEnabled}
          onChange={(v) => updateUiFieldValue('proxy_enabled', v.toString())}
          layout="horizontal"
        />
        <InputItem
          itemKey="proxy_url"
          label={t.config.proxyUrl}
          value={getUiFieldValue('proxy_url')}
          onChange={(v) => updateUiFieldValue('proxy_url', v)}
          placeholder={t.config.proxyUrlPlaceholder}
          hint={
            isProxyEnabled
              ? t.config.proxyUrlHint
              : t.config.proxyUrlDisabledHint
          }
          {...bindGuide('advanced.proxyUrl', g.advanced.proxyUrl)}
          layout="vertical"
        />
        <InputItem
          itemKey="proxy_bypass"
          label={t.config.proxyBypass}
          value={getUiFieldValue('proxy_bypass')}
          onChange={(v) => updateUiFieldValue('proxy_bypass', v)}
          placeholder="localhost,127.0.0.1,bilibili.com"
          hint={
            t.config.proxyBypassHint
          }
          {...bindGuide('advanced.proxyBypass', g.advanced.proxyBypass)}
          layout="vertical"
        />
        <InputItem
          itemKey="gemini_base_url"
          label={t.config.geminiBaseUrl}
          value={getUiFieldValue('gemini_base_url')}
          onChange={(v) => updateUiFieldValue('gemini_base_url', v)}
          placeholder="https://generativelanguage.googleapis.com"
          hint={
            t.config.geminiBaseUrlHint
          }
          {...bindGuide('advanced.geminiBaseUrl', g.advanced.geminiBaseUrl)}
          layout="vertical"
        />
        <InputItem
          itemKey="github_api_base_url"
          label={t.config.githubApiBaseUrl}
          value={getUiFieldValue('github_api_base_url')}
          onChange={(v) => updateUiFieldValue('github_api_base_url', v)}
          placeholder="https://api.github.com"
          hint={
            t.config.githubApiBaseUrlHint
          }
          {...bindGuide('advanced.githubApiBaseUrl', g.advanced.githubApiBaseUrl)}
          layout="vertical"
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.frontendCacheTitle}
        description={t.config.frontendCacheDesc}
        {...bindGuide('advanced.frontendCache', g.advanced.frontendCache)}
        icon={<LuRefreshCw />}
      >
        <ButtonItem
          itemKey="force_refresh_frontend_cache"
          label={t.config.forceRefreshFrontendCache}
          description={t.config.forceRefreshFrontendCacheDesc}
          {...bindGuide(
            'advanced.forceRefreshCache',
            g.advanced.forceRefreshCache,
          )}
          buttonText={
            cachePurgeArmed
              ? t.config.forceRefreshFrontendCacheConfirm
              : t.config.forceRefreshFrontendCacheButton
          }
          buttonIcon={<LuRefreshCw size={14} />}
          onClick={() => {
            void handleForceRefreshCacheClick()
          }}
          variant={cachePurgeArmed ? 'danger' : 'secondary'}
          layout="horizontal"
          loading={cachePurgeLoading}
          disabled={cachePurgeLoading}
        />
      </SettingGroup>

      <SettingGroup
        title={t.config.configBackupTitle}
        description={t.config.configBackupDesc}
        {...bindGuide('advanced.backup', g.advanced.backup)}
        icon={<FaSave />}
      >
        {formattedRestoreNotice && (
          <div
            className="settings-stat-chip is-warning settings-restore-media-notice"
            role="status"
          >
            {formattedRestoreNotice.skipped && (
              <>
                <p className="settings-restore-media-title">
                  {formattedRestoreNotice.skipped.title}
                </p>
                <ul className="settings-restore-media-list">
                  {formattedRestoreNotice.skipped.lines.map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                </ul>
                {formattedRestoreNotice.skipped.more && (
                  <p>{formattedRestoreNotice.skipped.more}</p>
                )}
              </>
            )}
            {formattedRestoreNotice.media && (
              <>
                <p className="settings-restore-media-title">
                  {formattedRestoreNotice.media.title}
                </p>
                <p>{t.config.restoreUnresolvedMediaHint}</p>
                <ul className="settings-restore-media-list">
                  {formattedRestoreNotice.media.lines.map((line) => (
                    <li key={line}>{line}</li>
                  ))}
                </ul>
                {formattedRestoreNotice.media.more && (
                  <p>{formattedRestoreNotice.media.more}</p>
                )}
              </>
            )}
            <SettingsButton variant="secondary" onClick={dismissRestoreNotice}>
              {t.config.restoreUnresolvedMediaDismiss}
            </SettingsButton>
          </div>
        )}
        <ButtonItem
          itemKey="export_config"
          label={t.config.exportConfig}
          description={t.config.exportConfigDesc}
          {...bindGuide('advanced.exportConfig', g.advanced.exportConfig)}
          buttonText={t.config.exportConfig}
          buttonIcon={<LuDownload size={14} />}
          onClick={handleExport}
          variant="secondary"
          layout="horizontal"
        />
        <ButtonItem
          itemKey="import_config"
          label={t.config.importConfig}
          description={t.config.importConfigDesc}
          {...bindGuide('advanced.importConfig', g.advanced.importConfig)}
          buttonText={t.config.importConfig}
          buttonIcon={<LuUpload size={14} />}
          onClick={handleImportClick}
          variant="secondary"
          layout="horizontal"
        />
        <input
          ref={fileInputRef}
          type="file"
          accept=".json"
          className="hidden"
          title="Import configuration file"
          onChange={handleFileChange}
        />
        <ButtonItem
          itemKey="reset_config"
          label={t.config.resetConfig}
          description={
            t.config.resetConfigDesc
          }
          {...bindGuide('advanced.resetConfig', g.advanced.resetConfig)}
          buttonText={t.config.resetConfig}
          onClick={() => setResetConfirmOpen(true)}
          variant="danger"
          layout="horizontal"
        />
      </SettingGroup>

      {resetConfirmOpen && (
        <div
          className="modal-overlay"
          onClick={() => setResetConfirmOpen(false)}
        >
          <div
            className="modal-content modal-small"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="modal-header">
              <h3 className="modal-title text-danger">
                {t.config.resetConfig}
              </h3>
              <button
                onClick={() => setResetConfirmOpen(false)}
                className="modal-close-button"
                title={t.common.close}
                aria-label={t.common.close}
              >
                <FaTimes />
              </button>
            </div>
            <div className="modal-body">
              <p className="settings-modal-message">
                {t.config.resetConfirmMessage}
              </p>
            </div>
            <div className="modal-footer">
              <SettingsButton
                variant="secondary"
                onClick={() => setResetConfirmOpen(false)}
              >
                {t.common.cancel}
              </SettingsButton>
              <SettingsButton
                variant="danger"
                onClick={() => {
                  onReset()
                  setResetConfirmOpen(false)
                }}
              >
                {t.common.confirm}
              </SettingsButton>
            </div>
          </div>
        </div>
      )}

      {importConfirmOpen && (
        <div className="modal-overlay" onClick={closeImportConfirm}>
          <div
            className="modal-content modal-small"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="modal-header">
              <h3 className="modal-title">{t.config.importConfig}</h3>
              <button
                onClick={closeImportConfirm}
                className="modal-close-button"
                title={t.common.close}
                aria-label={t.common.close}
              >
                <FaTimes />
              </button>
            </div>
            <div className="modal-body">
              <p className="settings-modal-message">
                {t.config.importConfirmMessage}
              </p>
              {restorePreview && (
                <div className="settings-stat-grid">
                  <div className="settings-stat-chip is-ok">
                    {previewText.restore}: {restorePreview.restore_count}
                  </div>
                  <div className="settings-stat-chip is-info">
                    {previewText.preserve}: {restorePreview.preserve_count}
                  </div>
                  <div className="settings-stat-chip is-muted">
                    {previewText.ignored}: {restorePreview.ignored_count}
                  </div>
                  <div className="settings-stat-chip is-accent">
                    {previewText.migrated}: {restorePreview.migrated_count}
                  </div>
                  {restorePreview.invalid_count > 0 && (
                    <div className="settings-stat-chip is-danger is-wide">
                      {previewText.invalid}: {restorePreview.invalid_count}
                      {invalidPreviewKeys.shown.length > 0 && (
                        <span className="settings-restore-invalid-keys">
                          {invalidPreviewKeys.shown.join(', ')}
                          {invalidPreviewKeys.hidden > 0 &&
                            ` ${format(t.config.restoreUnresolvedMediaMore, {
                              count: invalidPreviewKeys.hidden,
                            })}`}
                        </span>
                      )}
                    </div>
                  )}
                </div>
              )}
            </div>
            <div className="modal-footer">
              <SettingsButton variant="secondary" onClick={closeImportConfirm}>
                {t.common.cancel}
              </SettingsButton>
              <SettingsButton variant="primary" onClick={handleImportConfirm}>
                {t.common.confirm}
              </SettingsButton>
            </div>
          </div>
        </div>
      )}
    </SettingSection>
  )
}
