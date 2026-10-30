import type { TouchSummary } from './touchAppraisal'
import type { TouchObservation } from './touchGesture'
import type { TouchReaction } from './touchReaction'

export type TouchCompletion = TouchSummary & { displayedReaction: TouchReaction | null }

export class TouchEncounter {
  private pending: TouchCompletion | null = null
  private timer: ReturnType<typeof setTimeout> | null = null
  private contactId = -1
  private completedId = -1
  private lastSentAt = -Infinity
  constructor(private readonly publish: (summary: TouchCompletion) => void, private readonly now = () => performance.now()) {}

  observe(touch: TouchObservation, displayedReaction: TouchReaction | null = null): void {
    if (touch.phase === 'cancel') { this.cancel(); return }
    if (touch.phase === 'start') {
      if (this.pending?.region !== touch.region) this.cancel()
      if (this.timer) clearTimeout(this.timer)
      this.timer = null
      this.contactId = touch.id
      return
    }
    if (touch.phase !== 'end' || touch.id !== this.contactId || touch.id === this.completedId) return
    this.completedId = touch.id
    if (Number.isFinite(touch.durationMs) && touch.durationMs >= 0
      && (touch.gesture === 'hold' || touch.gesture === 'stroke')) {
      const prior = this.pending?.region === touch.region ? this.pending : null
      this.pending = { region: touch.region,
        displayedReaction,
        gesture: prior?.gesture === 'stroke' || touch.gesture === 'stroke' ? 'stroke' : 'hold',
        durationMs: Math.min(600_000, (prior?.durationMs ?? 0) + Math.round(touch.durationMs)),
        repeatCount: Math.min(8, (prior?.repeatCount ?? 0) + 1) }
    }
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => {
      const summary = this.pending
      this.pending = null
      this.timer = null
      if (!summary || summary.durationMs < 1200 || this.now() - this.lastSentAt < 30_000) return
      this.lastSentAt = this.now()
      this.publish(summary)
    }, 700)
  }

  cancel(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    this.pending = null
    this.contactId = -1
  }
}
