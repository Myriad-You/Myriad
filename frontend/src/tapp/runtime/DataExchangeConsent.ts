import type { PreparedDataExchange } from '../services/TappApiService'
import { createStore } from '../../utils/store'

export type DataExchangeConsentDecision =
  'allow' | 'deny' | 'expired' | 'cancelled'

export interface DataExchangeConsentRequest {
  prepared: PreparedDataExchange
  queuedCount: number
}

interface PendingConsent {
  prepared: PreparedDataExchange
  resolve: (decision: DataExchangeConsentDecision) => void
  timeout: ReturnType<typeof setTimeout>
  signal?: AbortSignal
  abortListener?: () => void
}

interface ConsentSnapshot {
  current: DataExchangeConsentRequest | null
}

let queue: PendingConsent[] = []
const snapshot = createStore<ConsentSnapshot>(
  { current: null },
  (a, b) =>
    a.current?.prepared === b.current?.prepared &&
    a.current?.queuedCount === b.current?.queuedCount,
)

function publish(): void {
  const current = queue[0]
  snapshot.set({
    current: current
      ? { prepared: current.prepared, queuedCount: queue.length - 1 }
      : null,
  })
}

function settle(
  requestId: string,
  decision: DataExchangeConsentDecision,
): boolean {
  const index = queue.findIndex(
    (entry) => entry.prepared.requestId === requestId,
  )
  if (index < 0) return false

  const entry = queue[index]!
  queue = queue.toSpliced(index, 1)
  clearTimeout(entry.timeout)
  if (entry.signal && entry.abortListener) {
    entry.signal.removeEventListener('abort', entry.abortListener)
  }
  entry.resolve(decision)
  publish()
  return true
}

export function requestDataExchangeConsent(
  prepared: PreparedDataExchange,
  signal?: AbortSignal,
): Promise<DataExchangeConsentDecision> {
  if (signal?.aborted) return Promise.resolve('cancelled')
  if (queue.some((entry) => entry.prepared.requestId === prepared.requestId)) {
    return Promise.resolve('cancelled')
  }

  const deadline = Date.parse(prepared.expiresAt)
  const remaining = Number.isFinite(deadline) ? deadline - Date.now() : 0
  if (remaining <= 0) return Promise.resolve('expired')

  const { promise, resolve } = Promise.withResolvers<DataExchangeConsentDecision>()
  const entry: PendingConsent = {
    prepared,
    resolve,
    timeout: setTimeout(
      settle,
      Math.min(remaining, 2_147_483_647),
      prepared.requestId,
      'expired',
    ),
    signal,
  }
  if (signal) {
    entry.abortListener = () => settle(prepared.requestId, 'cancelled')
    signal.addEventListener('abort', entry.abortListener, { once: true })
  }
  queue.push(entry)
  publish()
  return promise
}

export function decideDataExchangeConsent(
  requestId: string,
  allowed: boolean,
): boolean {
  if (queue[0]?.prepared.requestId !== requestId) return false
  return settle(requestId, allowed ? 'allow' : 'deny')
}

export const subscribeDataExchangeConsent = snapshot.subscribe
export const getDataExchangeConsentSnapshot = snapshot.get
