import type { BehaviorPlan } from './behavior'
import { MEROPE_LISTENING_EVENT } from '../events/performanceEvents'
import { compilePerformanceBehaviorPlan } from './performanceBehaviorPlan'

/** Typing this recently still counts as them talking to her. */
const TYPING_HOLDS_MS = 1500
/** A long message is followed by a fresh look of attention this often. */
const RENEW_MS = 4000

/**
 * While they type to her she attends, as a listener does: she leans in a
 * little and looks up at them (the `listen` expression), renewed now and
 * then over a long message, gone a moment after they stop. A candidate plan
 * like a touch's; the behavior scheduler places it among the others.
 */
export class ListeningMotionSource {
  private plan: BehaviorPlan | null = null
  private since = 0
  private lastTypedAt = Number.NEGATIVE_INFINITY
  private revision = 0
  private timer: ReturnType<typeof setTimeout> | null = null
  private listening = false

  constructor(private readonly changed: () => void, private readonly now: () => number) {}

  start(): void {
    if (this.listening || typeof window === 'undefined') return
    window.addEventListener(MEROPE_LISTENING_EVENT, this.onTyping)
    this.listening = true
  }

  stop(): void {
    if (typeof window !== 'undefined') window.removeEventListener(MEROPE_LISTENING_EVENT, this.onTyping)
    this.listening = false
    this.clear()
  }

  private readonly onTyping = (): void => this.typed(this.now())

  /** They typed a character. */
  typed(nowMs: number): void {
    this.lastTypedAt = nowMs
    if (!this.plan || nowMs - this.since >= RENEW_MS) {
      this.since = nowMs
      this.revision += 1
      this.plan = compilePerformanceBehaviorPlan(
        { phase: 'reaction', moodRevision: 0, motionStyle: 'even', plan: { cues: [{
          intent: 'listen', atMs: 0, intensity: 0.8, tempo: 0.9, fadeInMs: 300, fadeOutMs: 600, interrupt: 'if-lower',
        }] } },
        nowMs,
        `listening:${this.revision}`,
      )
      this.changed()
    }
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => this.lapse(), TYPING_HOLDS_MS)
  }

  /** Her attention while they are typing; null once they have stopped. */
  current(nowMs: number): BehaviorPlan | null {
    return nowMs - this.lastTypedAt < TYPING_HOLDS_MS ? this.plan : null
  }

  private lapse(): void {
    this.timer = null
    if (this.current(this.now())) return
    this.plan = null
    this.changed()
  }

  private clear(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    this.plan = null
    this.lastTypedAt = Number.NEGATIVE_INFINITY
  }
}
