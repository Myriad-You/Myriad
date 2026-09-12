export function isCancelledDeliveryError(
  errorMessage?: string | null,
): boolean {
  const s = errorMessage?.trim()
  if (!s) return false
  return s.toLowerCase().startsWith('cancelled:')
}

export function shouldOfferDeliveryRetry(item: {
  status: string
  error_message?: string | null
  retryable?: boolean
  intentional_cancel?: boolean
}): boolean {
  if (item.retryable === true) return true
  if (item.retryable === false) return false
  if (item.status === 'pending' || item.status === 'failed') return true
  if (item.status !== 'dead') return false
  if (item.intentional_cancel === true) return false
  return !isCancelledDeliveryError(item.error_message)
}
