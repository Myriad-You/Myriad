import type { PerformanceDirective } from '../../../services/agent/types'

export interface PlaybackDirectionSnapshot {
  version: number
  closed: boolean
  performance: unknown
}

export interface PlaybackDirectionScope {
  runId: string
  messageId: string
  generation: number
}

export async function sendPlaybackObservation(
  url: string,
  signal: AbortSignal,
  deps: {
    token: () => Promise<string | null>
    clearToken: () => void
    capture: () => { upcomingText: string; rig: unknown }
    fetch: typeof fetch
  },
): Promise<void> {
  const csrf = await deps.token()
  if (signal.aborted || !csrf) return
  const response = await deps.fetch(url, {
    method: 'PUT',
    credentials: 'include',
    signal,
    headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
    body: JSON.stringify(deps.capture()),
  })
  if (response.status === 403 && !signal.aborted) deps.clearToken()
}

interface Dependencies {
  observe: (scope: PlaybackDirectionScope, signal: AbortSignal) => Promise<void>
  read: (
    runId: string,
    after: number,
    signal: AbortSignal,
  ) => Promise<PlaybackDirectionSnapshot>
  close: (runId: string) => void
  current: (scope: PlaybackDirectionScope) => boolean
  playing: (scope: PlaybackDirectionScope) => boolean
  sanitize: (value: unknown) => PerformanceDirective | null
  deliver: (
    scope: PlaybackDirectionScope,
    performance: PerformanceDirective,
  ) => void
  note: (reason: string, scope: PlaybackDirectionScope) => void
}

/** Owns only transport lifetime. */
export class PlaybackDirectionClient {
  private active:
    | (PlaybackDirectionScope & {
        controller: AbortController
        after: number
        textDone: boolean
        observing: boolean
        observedAt: number
      })
    | null = null

  constructor(private readonly deps: Dependencies) {}

  get isActive(): boolean {
    return this.active !== null
  }

  start(scope: PlaybackDirectionScope): void {
    if (this.active?.runId === scope.runId) return
    this.stop()
    if (!this.deps.current(scope)) return
    const active = {
      ...scope,
      controller: new AbortController(),
      after: 0,
      textDone: false,
      observing: false,
      observedAt: -Infinity,
    }
    this.active = active
    void this.read(active)
    this.check()
  }

  textEnded(messageId: string): void {
    if (this.active?.messageId === messageId) this.active.textDone = true
    this.check()
  }

  check(nowMs: number = performance.now()): void {
    const active = this.active
    if (
      active &&
      (!this.deps.current(active) ||
        (active.textDone && !this.deps.playing(active)))
    ) {
      this.stop()
      return
    }
    if (active && !active.observing && nowMs - active.observedAt >= 500) {
      active.observing = true
      active.observedAt = nowMs
      void this.deps
        .observe(active, active.controller.signal)
        .catch(() => {}) // Feedback failure must not close valid delivery.
        .finally(() => {
          active.observing = false
        })
    }
  }

  cancel(messageId: string): void {
    if (this.active?.messageId === messageId) this.stop()
  }

  stop(): void {
    const active = this.active
    this.active = null
    if (!active) return
    active.controller.abort()
    this.deps.close(active.runId)
  }

  private async read(
    active: NonNullable<PlaybackDirectionClient['active']>,
  ): Promise<void> {
    try {
      while (this.active === active) {
        const result = await this.deps.read(
          active.runId,
          active.after,
          active.controller.signal,
        )
        if (this.active !== active) return
        this.check()
        if (this.active !== active) {
          this.deps.note('expired', active)
          return
        }
        if (
          !Number.isSafeInteger(result.version) ||
          result.version < active.after ||
          typeof result.closed !== 'boolean'
        ) {
          throw new Error('Invalid direction cursor')
        }
        if (result.version > active.after) {
          active.after = result.version
          const performance = this.deps.sanitize(result.performance)
          if (performance) {
            this.deps.note('received', active)
            this.deps.deliver(active, performance)
          }
        }
        if (result.closed) {
          // Closing transport must never clear plans that are already playing on the body.
          this.stop()
          return
        }
      }
    } catch {
      if (this.active === active) {
        this.deps.note('unavailable', active)
        this.stop()
      }
    }
  }
}
