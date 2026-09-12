import { isUselessErrorText, userFacingError } from './userFacingError'

export interface ReportErrorCopy {
  generateNeedData: string
  generateEmptySummary: string
}

function rawText(reason: unknown): string {
  if (typeof reason === 'string') return reason.trim()
  if (reason instanceof Error) return reason.message.trim()
  return ''
}

export function reportUserFacingError(
  reason: unknown,
  fallback: string,
  copy: ReportErrorCopy,
): string {
  const text = rawText(reason)
  if (
    /平台数据未获取|raw data file not found|failed to process\s+\w+|failed to fetch data/i.test(
      text,
    )
  ) {
    return copy.generateNeedData
  }
  if (/empty summary|stats not found|stats missing/i.test(text)) {
    return copy.generateEmptySummary
  }
  if (/failed to save report|serialize report|insert report|report persist/i.test(text)) {
    return fallback
  }
  if (text.includes('未能生成报告') && isUselessErrorText(text.split('：').pop() || '')) {
    return fallback
  }
  return userFacingError(reason, fallback)
}
