import { currentCopy } from '../i18n/localeCopy'
import apiService from '../services/api'
import { userFacingError } from './userFacingError'

export interface ReportSettings {
  expiryEnabled: boolean
  autoRegenerate: boolean
  expiryDays: number
}

export const DEFAULT_REPORT_SETTINGS: ReportSettings = {
  expiryEnabled: false,
  autoRegenerate: false,
  expiryDays: 7,
}

interface ReportSettingsResponse {
  success: boolean
  message?: string
  config?: Partial<ReportSettings>
}

function normalizeReportSettings(
  raw: Partial<ReportSettings> | undefined,
): ReportSettings {
  const days = Number(raw?.expiryDays)
  return {
    expiryEnabled: Boolean(raw?.expiryEnabled),
    autoRegenerate: Boolean(raw?.autoRegenerate),
    expiryDays: Number.isFinite(days)
      ? Math.min(365, Math.max(1, Math.round(days)))
      : DEFAULT_REPORT_SETTINGS.expiryDays,
  }
}

export async function fetchReportSettings(): Promise<ReportSettings> {
  const response = await apiService.get<ReportSettingsResponse>(
    '/config/report-settings',
  )
  return normalizeReportSettings(response.config)
}

export async function updateReportSettings(
  settings: ReportSettings,
): Promise<ReportSettings> {
  const response = await apiService.put<ReportSettingsResponse>(
    '/config/report-settings',
    normalizeReportSettings(settings),
  )
  if (!response.success) {
    throw new Error(
      userFacingError(
        response.message,
        currentCopy().config.reportSettingsSaveFailed,
      ),
    )
  }
  return normalizeReportSettings(response.config)
}

export function areReportSettingsEqual(
  left: ReportSettings,
  right: ReportSettings,
): boolean {
  return (
    left.expiryEnabled === right.expiryEnabled &&
    left.autoRegenerate === right.autoRegenerate &&
    left.expiryDays === right.expiryDays
  )
}
