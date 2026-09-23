import type { UnresolvedRestoredMedia } from '../lib/settingsRestoreNotice'
import { currentCopy } from '../i18n/localeCopy'
import {
  parseSettingKeys,
  parseUnresolvedRestoredMedia,
} from '../lib/settingsRestoreNotice'
import { invalidatePermissionConfig } from '../utils/permissionConfig'
import { isUselessErrorText, userFacingError } from '../utils/userFacingError'
import { apiService, parseApiErrorBody } from './api'

function extractApiErrorMessage(data: unknown, fallback: string): string {
  const parsed = parseApiErrorBody(data, 400)
  const raw =
    parsed.message && parsed.message !== 'API Error: 400' ? parsed.message : ''
  return userFacingError(raw, fallback)
}

/** A successful HTTP response can still reject a configuration write. */
function assertConfigWriteSuccess(data: unknown, fallbackMessage: string): void {
  if (
    data &&
    typeof data === 'object' &&
    Object.hasOwn(data, 'success') &&
    (data as { success: unknown }).success !== true
  ) {
    throw new Error(extractApiErrorMessage(data, fallbackMessage))
  }
}

// SetupWizard fetches setup itself; not this module.
export function fetchConfig(): Promise<any> {
  return apiService.get('/config')
}

export async function updateConfig(config: any): Promise<any> {
  const data = await apiService.post('/config', config)
  assertConfigWriteSuccess(data, currentCopy().errors.configSaveFailed)
  return data
}

export function fetchSettingsBackup(): Promise<any> {
  return apiService.get('/config/settings-backup')
}

export interface SettingsRestorePreview {
  backup_version: number
  current_version: number
  restore_count: number
  preserve_count: number
  ignored_count: number
  migrated_count: number
  invalid_count: number
  ignored_keys: string[]
  invalid_keys: string[]
}

export async function previewSettingsBackup(backup: unknown) {
  const data = await apiService.post<{ preview?: SettingsRestorePreview, error?: unknown }>(
    '/config/settings-backup/preview',
    backup,
  )
  if (!data?.preview) {
    const previewError = typeof data?.error === 'string' ? data.error : ''
    throw new Error(
      previewError && !isUselessErrorText(previewError)
        ? previewError
        : currentCopy().errors.settingsBackupPreviewFailed,
    )
  }
  return data.preview
}

export interface SettingsRestoreResult {
  success: true
  message?: string
  requires_reload?: boolean
  preview?: SettingsRestorePreview
  /** Local media the backup cites that does not exist here; left unbound. */
  unresolved_media: UnresolvedRestoredMedia[]
  /**
   * Settings the restore skipped as invalid, keeping their current values
   * (`preview.invalid_keys`, key names only).
   */
  skipped_settings: string[]
}

export async function restoreSettingsBackup(
  backup: unknown,
): Promise<SettingsRestoreResult> {
  const data = await apiService.post<any>('/config/settings-backup', backup)
  if (data?.success !== true) {
    const restoreError = typeof data?.error === 'string' ? data.error : ''
    throw new Error(
      restoreError && !isUselessErrorText(restoreError)
        ? restoreError
        : currentCopy().errors.settingsBackupRestoreFailed,
    )
  }
  invalidatePermissionConfig()
  return {
    ...data,
    unresolved_media: parseUnresolvedRestoredMedia(data.unresolved_media),
    skipped_settings: parseSettingKeys(data.preview?.invalid_keys),
  } as SettingsRestoreResult
}

export function fetchPermissionsConfig(): Promise<any> {
  return apiService.get('/config/permissions')
}

export async function updatePermissionsConfig(
  permissions: Record<string, boolean | number>,
): Promise<any> {
  const data = await apiService.post('/config/permissions', permissions)
  assertConfigWriteSuccess(data, currentCopy().errors.configSaveFailed)
  invalidatePermissionConfig()
  return data
}

export async function reloadSystemConfig(): Promise<any> {
  const data = await apiService.post('/system/reload-config')
  assertConfigWriteSuccess(data, currentCopy().errors.configReloadFailed)
  invalidatePermissionConfig()
  return data
}

export interface SpeechTestResponse {
  success: boolean
  provider?: string
  tts_ok?: boolean
  asr_ok?: boolean
  tts_skipped?: boolean
  audio?: string
  transcript?: string
  error?: string
}

export function testSpeechService(): Promise<SpeechTestResponse> {
  return apiService.post('/speech/test')
}
