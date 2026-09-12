import type { TouchObservation } from './touchGesture'
import type { TouchReaction } from './touchReaction'
import { TOUCH_REACTIONS } from './touchReaction'

export type TouchSummary = Pick<TouchObservation, 'region' | 'gesture' | 'durationMs' | 'repeatCount'>
interface Dependencies {
  request: (summary: TouchSummary, signal: AbortSignal) => Promise<unknown>
  apply: (revision: number, reaction: TouchReaction) => void
  now: () => number
}

/** One bounded request per semantic revision, never one per pointer frame. */
export class TouchAppraisal {
  private revision = -1
  private requested = -1
  private lastRequest = -Infinity
  private flight: AbortController | null = null
  private disposed = false
  constructor(private readonly deps: Dependencies) {}

  observe(touch: TouchObservation, revision: number): void {
    if (this.disposed) return
    if (revision !== this.revision) this.flight?.abort()
    this.revision = revision
    if (touch.phase === 'end' || touch.phase === 'cancel') {
      this.cancel()
      return
    }
    if (touch.durationMs < 400 || !['hold', 'stroke'].includes(touch.gesture)
      || this.flight || revision === this.requested || this.deps.now() - this.lastRequest < 5000) {
      return
    }
    this.requested = revision
    this.lastRequest = this.deps.now()
    const controller = new AbortController()
    this.flight = controller
    const timeout = setTimeout(() => controller.abort(), 3200)
    const summary: TouchSummary = {
      region: touch.region, gesture: touch.gesture,
      durationMs: Math.min(600_000, Math.max(120, Math.round(touch.durationMs))),
      repeatCount: Math.min(8, Math.max(1, touch.repeatCount)),
    }
    let onAbort: () => void = () => {}
    const aborted = new Promise<null>((resolve) => {
      onAbort = () => resolve(null)
      controller.signal.addEventListener('abort', onAbort, { once: true })
    })
    void Promise.race([
      this.deps.request(summary, controller.signal),
      aborted,
    ]).then((raw) => {
      if (controller.signal.aborted || this.disposed || this.revision !== revision
        || this.deps.now() - this.lastRequest > 3200) {
        return
      }
      const reaction = (raw as { reaction?: unknown } | null)?.reaction
      if (TOUCH_REACTIONS.includes(reaction as TouchReaction)) this.deps.apply(revision, reaction as TouchReaction)
    }).catch(() => { })
      .finally(() => {
        clearTimeout(timeout)
        controller.signal.removeEventListener('abort', onAbort)
        if (this.flight === controller) this.flight = null
      })
  }

  dispose(): void {
    this.disposed = true
    this.cancel()
  }

  cancel(): void {
    this.revision = -1
    this.flight?.abort()
  }
}
