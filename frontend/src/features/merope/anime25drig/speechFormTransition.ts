import type { SpeechGesture } from '../speech/phraseGestures'
import { SPEECH_GESTURES } from '../speech/phraseGestures'

/** Generic accents participate in the same blend, but have no named gesture. */
type SpeechForm = SpeechGesture | 'accent'
type Shares = Record<SpeechForm, number>
const FORMS = [...SPEECH_GESTURES, 'accent'] as const

function shares(form?: SpeechForm): Shares {
  return Object.fromEntries(
    FORMS.map((key) => [key, key === form ? 1 : 0]),
  ) as Shares
}

/**
 * A shape revision is not a new motion unit or a second timing envelope.
 * Transport the relative shape shares from what was drawn to the revised
 * form. The existing unit still supplies all intensity and stroke timing.
 */
export class SpeechFormTransition {
  private target: SpeechForm
  private from: Shares
  private readonly output: Shares
  private startedAt = 0
  private duration = 0

  constructor(form: string) {
    this.target = speechForm(form)
    this.from = shares(this.target)
    this.output = { ...this.from }
  }

  revise(form: string, drawnAt: number, peakAt: number): void {
    const next = speechForm(form)
    if (next === this.target) return
    this.from = { ...this.sample(drawnAt) }
    this.target = next
    this.startedAt = drawnAt
    // Aim to finish in the remaining preparation. A prediction-led renderer
    // may already be near/past the peak; keep a small arrival instead of a cut.
    // Neither this duration nor a restatement moves the scheduler's pegs.
    this.duration = Math.min(0.16, Math.max(0.06, peakAt - drawnAt))
  }

  sample(now: number): Readonly<Shares> {
    const t =
      this.duration > 0
        ? Math.max(0, Math.min(1, (now - this.startedAt) / this.duration))
        : 1
    const blend = t * t * t * (t * (t * 6 - 15) + 10)
    for (const form of FORMS) {
      const target = form === this.target ? 1 : 0
      this.output[form] = this.from[form] + (target - this.from[form]) * blend
    }
    return this.output
  }

  /** Cancellation releases the current shape, not an unperformed revision. */
  freeze(drawnAt: number): void {
    this.from = { ...this.sample(drawnAt) }
    this.startedAt = drawnAt
    this.duration = Number.POSITIVE_INFINITY
  }
}

function speechForm(form: string): SpeechForm {
  return SPEECH_GESTURES.includes(form as SpeechGesture)
    ? (form as SpeechGesture)
    : 'accent'
}
