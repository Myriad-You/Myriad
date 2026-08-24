import { ApiError } from '../../../services/api'
import { isUselessErrorText } from '../../../utils/userFacingError'

const HOST_GENERATION_CODES = new Set([
  'report_dna_failed',
  'pro_unavailable',
  'standard_unavailable',
  'name_suggest_failed',
  'name_unusable',
  'persona_draft_failed',
  'persona_unusable',
  'visual_design_failed',
  'visual_design_unusable',
  'visual_design_language',
  'visual_language_required',
  'visual_design_required',
  'visual_identity_invalid',
  'persona_contract_invalid',
  'portrait_generation_in_progress',
  'character_visual_inputs_changed',
  'image_provider_unconfigured',
  'image_provider_unsupported',
  'image_provider_credits',
  'image_provider_unauthorized',
  'image_provider_rate_limited',
  'image_provider_rejected',
  'image_provider_invalid_response',
  'portrait_generation_failed',
  'portrait_edit_notes_required',
  'portrait_adjustment_invalid',
  'portrait_adjustment_out_of_scope',
  'portrait_required_for_edit',
  'see_through_token_required',
  'see_through_busy',
  'see_through_auth_failed',
  'see_through_quota_unavailable',
  'see_through_timeout',
  'see_through_upstream_failed',
  'see_through_invalid_input',
  'QUEUE_FULL',
  'agent_access_denied',
  'agent_processing_failed',
])

export function errorCode(reason: unknown): string | undefined {
  if (reason instanceof ApiError && reason.code) return reason.code
  if (
    reason &&
    typeof reason === 'object' &&
    'code' in reason &&
    typeof (reason as { code: unknown }).code === 'string'
    && (reason as { code: string }).code.trim()
  ) {
    return (reason as { code: string }).code
  }
  return undefined
}

export function isGenerationTimeout(reason: unknown): boolean {
  if (reason instanceof ApiError && reason.code === 'TIMEOUT') return true
  const message =
    reason instanceof Error ? reason.message : String(reason ?? '')
  return /timeout/i.test(message)
}

export function isPortraitInProgress(reason: unknown): boolean {
  return errorCode(reason) === 'portrait_generation_in_progress'
}

function apiErrorDetail(reason: unknown): string {
  if (reason instanceof Error) return reason.message.trim()
  return ''
}

function apiErrorHint(reason: unknown): string {
  if (
    reason &&
    typeof reason === 'object' &&
    'hint' in reason &&
    typeof (reason as { hint: unknown }).hint === 'string'
  ) {
    return (reason as { hint: string }).hint.trim()
  }
  return ''
}

function usefulDetail(detail: string, ...known: string[]): string {
  if (!detail) return ''
  if (isUselessErrorText(detail)) return ''
  if (known.some((item) => item && detail === item)) return ''
  return detail
}

function joinParts(...parts: string[]): string {
  return parts.filter(Boolean).join(' ')
}

export function generationFailureMessage(
  reason: unknown,
  fallback: string,
  timeoutMessage: string,
  byCode?: Record<string, string>,
): string {
  if (isGenerationTimeout(reason)) return timeoutMessage
  const code = errorCode(reason)
  const mapped = (code && byCode?.[code]) || ''
  const rawDetail = apiErrorDetail(reason)
  const stripped =
    code && rawDetail === code
      ? ''
      : code && rawDetail.startsWith(`${code}:`)
        ? rawDetail.slice(code.length + 1).trim()
        : rawDetail
  const detail = usefulDetail(stripped, mapped, fallback)
  const hint = usefulDetail(apiErrorHint(reason), mapped, fallback, detail)
  if (mapped) return joinParts(mapped, detail, hint)
  if (code && HOST_GENERATION_CODES.has(code)) {
    return joinParts(fallback, detail, hint)
  }
  if (detail) return joinParts(detail, hint)
  return joinParts(fallback, hint)
}
