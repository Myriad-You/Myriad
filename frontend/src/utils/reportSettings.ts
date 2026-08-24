// 报告过期设置（设置页 → 模块设置 → 报告页设置）
// 存于后端 configurations 表（report_settings 键），随全局保存统一提交
import { currentCopy } from '../i18n/localeCopy'
import apiService from '../services/api'

export interface ReportSettings {
  /** 是否启用报告过期（关闭时报告永不过期） */
  expiryEnabled: boolean
  /** 过期后读取时自动后台重新生成（只调 AI，不重新抓平台数据） */
  autoRegenerate: boolean
  /** 过期天数（1-365） */
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
      response.message || currentCopy().errors.operationFailed,
    )
  }
  return normalizeReportSettings(response.config)
}

/** 比较两份报告设置是否等价（用于统一保存流程的脏检测） */
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
