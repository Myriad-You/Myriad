/**
 * Delivery-queue UI classification helpers.
 * Keep in sync with backend `is_user_cancelled_delivery_error` /
 * `should_offer_retry_for_dead_error` in federation/delivery.rs.
 */

/** Intentional local/API cancel — `cancelled:…` prefix (not a peer fail). */
export function isCancelledDeliveryError(
  errorMessage?: string | null,
): boolean {
  const s = errorMessage?.trim()
  if (!s) return false
  return s.toLowerCase().startsWith('cancelled:')
}

/** Dead rows that are not intentional cancels may show Retry. */
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
