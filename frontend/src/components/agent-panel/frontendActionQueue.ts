import type { FrontendAction } from '../../services/agent/types'
import { frontendActionDedupeKey } from '../../services/agent/frontendActions'
import { authSubject } from '../../utils/authSubject'

/** Per-message ordering/dedupe, owned by an engine and the submitting identity. */
export class FrontendActionQueue {
  private readonly keys = new Map<string, Set<string>>()
  private readonly chains = new Map<string, Promise<void>>()
  private lifetime = new AbortController()

  constructor(
    private readonly execute: (action: FrontendAction, signal: AbortSignal) => Promise<unknown>,
    private readonly maxMessages: number,
  ) {}

  reset(): void {
    this.lifetime.abort()
    this.lifetime = new AbortController()
    this.keys.clear()
    this.chains.clear()
  }

  forget(messageId: string): void {
    this.keys.delete(messageId)
    this.chains.delete(messageId)
  }

  async enqueue(
    messageId: string,
    actions: Array<FrontendAction | null | undefined>,
    subject = authSubject.signal,
  ): Promise<unknown[]> {
    const signal = AbortSignal.any([subject, this.lifetime.signal])
    if (signal.aborted) return []
    const visible: unknown[] = []
    const keys = this.keys.get(messageId) ?? new Set<string>()
    this.keys.set(messageId, keys)
    while (this.keys.size > this.maxMessages) {
      const oldest = this.keys.keys().next().value
      if (oldest === undefined || oldest === messageId) break
      this.forget(oldest)
    }
    const run = async () => {
      for (const action of actions) {
        if (signal.aborted) return
        if (!action || typeof action !== 'object' || !Object.hasOwn(action, 'type')) continue
        const key = frontendActionDedupeKey(action)
        if (keys.has(key)) continue
        keys.add(key)
        try {
          const result = await this.execute(action, signal)
          if (signal.aborted) return
          if (result && typeof result === 'object' && [
            'query_windows', 'music_get_status', 'show_data', 'show_report',
          ].includes(action.type)) {
            visible.push(result)
          }
        } catch (error) {
          if (signal.aborted) return
          console.error('[AgentEngine] Frontend action failed:', error)
        }
      }
    }
    const prev = this.chains.get(messageId) ?? Promise.resolve()
    const next = prev.then(run, run)
    this.chains.set(messageId, next)
    await next
    return signal.aborted ? [] : visible
  }
}
