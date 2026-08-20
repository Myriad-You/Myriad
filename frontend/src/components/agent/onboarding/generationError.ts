import { ApiError } from '../../../services/api'

const HOST_GENERATION_CODES = new Set([
  'report_dna_failed',
  'pro_unavailable',
  'name_suggest_failed',
  'persona_draft_failed',
])

export function isGenerationTimeout(reason: unknown): boolean {
  if (reason instanceof ApiError && reason.code === 'TIMEOUT') return true
  const message = reason instanceof Error ? reason.message : String(reason ?? '')
  return /timeout/i.test(message)
}

export function generationFailureMessage(
  reason: unknown,
  fallback: string,
  timeoutMessage: string,
): string {
  if (isGenerationTimeout(reason)) return timeoutMessage
  if (reason instanceof ApiError && reason.code && HOST_GENERATION_CODES.has(reason.code)) {
    return fallback
  }
  if (reason instanceof Error && reason.message.trim()) return reason.message
  return fallback
}
