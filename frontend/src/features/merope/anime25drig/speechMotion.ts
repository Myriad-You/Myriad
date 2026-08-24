export interface AutoSpeechPose {
  mouthOpen: number
  mouthForm: number
  phraseActivity: number
  browAccent: number
  headAccent: number
}

type RandomSource = () => number

const ZERO_SPEECH: AutoSpeechPose = {
  mouthOpen: 0,
  mouthForm: 0,
  phraseActivity: 0,
  browAccent: 0,
  headAccent: 0,
}

export function speechPhraseAmplitudeScale(progress: number): number {
  const bounded = unitInterval(progress)
  const onset = mix(0.88, 1, smootherstep(bounded / 0.16))
  const ending = mix(1, 0.82, smootherstep((bounded - 0.72) / 0.28))
  return onset * ending
}

export function speechPhraseIntervalScale(progress: number): number {
  const bounded = unitInterval(progress)
  return mix(1, 1.18, smootherstep((bounded - 0.72) / 0.28))
}

/**
 * Text-free preview speech for the workbench.
 *
 * Anime2.5DRig's original demo chooses an unrelated mouth target every
 * 70-180ms. This controller keeps the same lightweight, client-only role, but
 * groups syllables into words and phrases and interpolates adjacent shapes.
 * The returned object is reused so the animation loop does not allocate.
 */
export class AutoSpeechController {
  private readonly output: AutoSpeechPose = { ...ZERO_SPEECH }
  private initialized = false
  private enabled = false
  private speaking = false
  private nextEventAt = Number.POSITIVE_INFINITY
  private phraseStartedAt = 0
  private phraseEndsAt = 0
  private phraseDuration = 0
  private syllablesRemaining = 0
  private emphasisCooldown = 0
  private emphasisStartedAt = Number.NEGATIVE_INFINITY
  private transitionStartedAt = 0
  private transitionDuration = 0.08
  private fromOpen = 0
  private toOpen = 0
  private fromForm = 0
  private toForm = 0

  constructor(private readonly random: RandomSource = Math.random) {}

  sample(timeSeconds: number, enabled: boolean): Readonly<AutoSpeechPose> {
    const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
    if (!this.initialized) {
      this.initialized = true
      this.enabled = enabled
      this.nextEventAt = enabled
        ? now + this.randomRange(0.08, 0.16)
        : Number.POSITIVE_INFINITY
    }

    if (enabled !== this.enabled) {
      this.enabled = enabled
      if (!enabled) {
        this.reset(now)
        return this.output
      }
      this.nextEventAt = now + this.randomRange(0.08, 0.16)
    }

    if (!this.enabled) return this.output

    this.resolve(now)
    // Preserve time-based behavior after a throttled or dropped frame without
    // allowing an unbounded catch-up loop in the render path.
    for (let event = 0; event < 12 && now >= this.nextEventAt; event += 1) {
      const scheduledAt = this.nextEventAt
      this.advance(scheduledAt)
      this.resolve(now)
    }
    return this.output
  }

  private advance(now: number): void {
    if (!this.speaking) {
      this.speaking = true
      this.phraseStartedAt = now
      this.phraseDuration = this.randomRange(1.5, 3.1)
      this.phraseEndsAt = now + this.phraseDuration
      this.syllablesRemaining = this.randomInteger(2, 5)
      this.emphasisCooldown = 0
      this.scheduleSyllable(now)
      return
    }

    if (now >= this.phraseEndsAt) {
      this.speaking = false
      this.beginTransition(now, 0, 0, this.randomRange(0.16, 0.22))
      this.nextEventAt = now + this.randomRange(0.42, 0.92)
      return
    }

    if (this.syllablesRemaining <= 0) {
      this.syllablesRemaining = this.randomInteger(2, 5)
      this.beginTransition(
        now,
        this.randomRange(0.06, 0.12),
        0,
        this.randomRange(0.14, 0.19),
      )
      this.nextEventAt = now + this.randomRange(0.11, 0.24)
      return
    }

    this.scheduleSyllable(now)
  }

  private scheduleSyllable(now: number): void {
    const phraseProgress =
      this.phraseDuration > 0
        ? (now - this.phraseStartedAt) / this.phraseDuration
        : 0
    const interval =
      this.randomRange(0.105, 0.185) * speechPhraseIntervalScale(phraseProgress)
    const emphasized = this.emphasisCooldown === 0 && this.randomUnit() < 0.14
    this.emphasisCooldown = emphasized
      ? 2
      : Math.max(0, this.emphasisCooldown - 1)
    if (emphasized) this.emphasisStartedAt = now
    let openness = emphasized
      ? this.randomRange(0.62, 0.8)
      : this.randomRange(0.28, 0.61)
    openness *= speechPhraseAmplitudeScale(phraseProgress)
    // Large adjacent jumps read as sprite switching on a two-difference mouth.
    // Keep enough contrast for articulation while preserving visual continuity.
    openness = clamp(openness, this.toOpen - 0.34, this.toOpen + 0.34)
    openness = clamp(openness, 0.22, 0.8)
    let form = this.randomRange(-0.11, 0.11)
    form = clamp(form, this.toForm - 0.12, this.toForm + 0.12)
    this.beginTransition(
      now,
      openness,
      form,
      Math.min(interval * 0.72, this.randomRange(0.08, 0.11)),
    )
    this.syllablesRemaining -= 1
    this.nextEventAt = now + interval
  }

  private beginTransition(
    now: number,
    mouthOpen: number,
    mouthForm: number,
    duration: number,
  ): void {
    this.resolve(now)
    this.fromOpen = this.output.mouthOpen
    this.fromForm = this.output.mouthForm
    this.toOpen = mouthOpen
    this.toForm = mouthForm
    this.transitionStartedAt = now
    this.transitionDuration = Math.max(0.001, duration)
  }

  private resolve(now: number): Readonly<AutoSpeechPose> {
    const progress = smootherstep(
      (now - this.transitionStartedAt) / this.transitionDuration,
    )
    this.output.mouthOpen = mix(this.fromOpen, this.toOpen, progress)
    this.output.mouthForm = mix(this.fromForm, this.toForm, progress)
    this.output.phraseActivity = this.resolvePhraseActivity(now)
    const emphasisElapsed = now - this.emphasisStartedAt
    // Brows anticipate the visual beat while the smaller nod lands after it.
    this.output.browAccent = attackReleasePulse(emphasisElapsed, 0, 0.065, 0.2)
    this.output.headAccent = attackReleasePulse(
      emphasisElapsed,
      0.045,
      0.1,
      0.22,
    )
    return this.output
  }

  private resolvePhraseActivity(now: number): number {
    if (this.phraseDuration <= 0) return 0
    const sinceStart = now - this.phraseStartedAt
    const untilEnd = this.phraseEndsAt - now
    if (sinceStart < 0 || untilEnd <= 0) return 0
    return smootherstep(sinceStart / 0.16) * smootherstep(untilEnd / 0.22)
  }

  private reset(now: number): void {
    this.speaking = false
    this.nextEventAt = Number.POSITIVE_INFINITY
    this.phraseStartedAt = now
    this.phraseEndsAt = now
    this.phraseDuration = 0
    this.syllablesRemaining = 0
    this.emphasisCooldown = 0
    this.emphasisStartedAt = Number.NEGATIVE_INFINITY
    this.transitionStartedAt = now
    this.fromOpen = 0
    this.toOpen = 0
    this.fromForm = 0
    this.toForm = 0
    this.output.mouthOpen = 0
    this.output.mouthForm = 0
    this.output.phraseActivity = 0
    this.output.browAccent = 0
    this.output.headAccent = 0
  }

  private randomInteger(minimum: number, maximum: number): number {
    return Math.min(maximum, Math.floor(this.randomRange(minimum, maximum + 1)))
  }

  private randomRange(minimum: number, maximum: number): number {
    return minimum + (maximum - minimum) * this.randomUnit()
  }

  private randomUnit(): number {
    const value = this.random()
    return Number.isFinite(value) ? clamp(value, 0, 1) : 0.5
  }
}

function smootherstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * bounded * (bounded * (bounded * 6 - 15) + 10)
}

function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * amount
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function unitInterval(value: number): number {
  return Number.isFinite(value) ? clamp(value, 0, 1) : 0
}

function attackReleasePulse(
  elapsed: number,
  delay: number,
  attack: number,
  release: number,
): number {
  const shifted = elapsed - delay
  if (!Number.isFinite(shifted) || shifted < 0) return 0
  if (shifted < attack) return smootherstep(shifted / attack)
  if (shifted < attack + release) {
    return 1 - smootherstep((shifted - attack) / release)
  }
  return 0
}
