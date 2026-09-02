import type { BehaviorQuality } from '../motion/behavior'
import type { SpeechProsodyPlan } from '../speech/prosody'

export interface CoSpeechExpressionOffset {
  brow: number
  eyeOpen: number
  angleY: number
  angleZ: number
  body: number
}

const RELEASE_RATE = 6.2

/** Keeps authored audio/viseme input on the same visual-prosody path. */
export class CoSpeechExpressionController {
  private readonly output: CoSpeechExpressionOffset = {
    brow: 0,
    eyeOpen: 0,
    angleY: 0,
    angleZ: 0,
    body: 0,
  }

  private readonly targetOffset: CoSpeechExpressionOffset = {
    brow: 0,
    eyeOpen: 0,
    angleY: 0,
    angleZ: 0,
    body: 0,
  }

  private previousEnergy = 0
  private accentStartedAt = Number.NEGATIVE_INFINITY
  private nextAccentAt = 0
  private lastTime = Number.NaN
  private initialized = false
  private plannedAccents: Array<{ at: number; intensity: number }> = []
  private plannedAccentIndex = 0
  private plannedUntil = Number.NEGATIVE_INFINITY
  private accentIntensity = 1
  private previousHeadBeat = 0
  private gestureDirection = 1

  setProsody(
    plan: SpeechProsodyPlan | null,
    playerTimeSeconds: number,
    wallNowMs: number = performance.now(),
  ): void {
    this.plannedAccents = []
    this.plannedAccentIndex = 0
    this.plannedUntil = Number.NEGATIVE_INFINITY
    if (!plan) return
    const ageSeconds = Math.max(0, wallNowMs - plan.startedAtMs) / 1_000
    const origin = playerTimeSeconds - ageSeconds
    this.plannedAccents = plan.accents.map((accent) => ({
      at: origin + accent.offsetMs / 1_000,
      intensity: unitInterval(accent.intensity),
    }))
    this.plannedUntil = origin + plan.durationMs / 1_000
    while (
      this.plannedAccentIndex < this.plannedAccents.length &&
      this.plannedAccents[this.plannedAccentIndex]!.at < playerTimeSeconds - 0.2
    ) {
      this.plannedAccentIndex += 1
    }
  }

  sample(
    timeSeconds: number,
    active: boolean,
    authoredEnergy: number | null,
    phraseActivity: number,
    browAccent: number,
    headAccent: number,
    quality?: Readonly<BehaviorQuality>,
  ): Readonly<CoSpeechExpressionOffset> {
    const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
    const dt = Number.isFinite(this.lastTime)
      ? clamp(now - this.lastTime, 0, 0.05)
      : 0
    this.lastTime = now
    const energy = authoredEnergy == null ? 0 : unitInterval(authoredEnergy)
    const planned = this.plannedAccents[this.plannedAccentIndex]
    if (active && planned && now >= planned.at - 0.065) {
      this.accentStartedAt = planned.at - 0.065
      this.accentIntensity = planned.intensity
      this.plannedAccentIndex += 1
    }
    const plannedProsodyActive = active && now <= this.plannedUntil
    if (
      active &&
      !plannedProsodyActive &&
      authoredEnergy != null &&
      energy >= 0.58 &&
      this.previousEnergy < 0.46 &&
      now >= this.nextAccentAt
    ) {
      this.accentStartedAt = now
      this.accentIntensity = 1
      this.nextAccentAt = now + 0.48
    }

    if (active && authoredEnergy != null) {
      this.previousEnergy = energy
    } else if (!active) {
      this.previousEnergy = 0
      this.accentStartedAt = Number.NEGATIVE_INFINITY
      this.nextAccentAt = now
      this.plannedAccents = []
      this.plannedAccentIndex = 0
      this.plannedUntil = Number.NEGATIVE_INFINITY
      this.previousHeadBeat = 0
    }

    const authoredActivity =
      active && authoredEnergy != null
        ? 0.22 + 0.78 * smootherstep((energy - 0.05) / 0.55)
        : 0
    const elapsed = now - this.accentStartedAt
    const authoredBrow = active
      ? attackReleasePulse(elapsed, 0, 0.065, 0.2) * this.accentIntensity
      : 0
    const authoredHead = active
      ? attackReleasePulse(elapsed, 0.045, 0.1, 0.22) * this.accentIntensity
      : 0
    const resolvedHeadAccent = Math.max(headAccent, authoredHead)
    const headBeat = unitInterval(resolvedHeadAccent)
    if (headBeat > 0.12 && this.previousHeadBeat <= 0.12) {
      this.gestureDirection *= -1
    }
    this.previousHeadBeat = headBeat
    writeOffset(
      this.targetOffset,
      Math.max(phraseActivity, authoredActivity),
      Math.max(browAccent, authoredBrow),
      resolvedHeadAccent,
      now,
      this.gestureDirection,
      quality,
    )
    const responseScale = qualityResponseScale(quality)
    if (!this.initialized) {
      this.initialized = true
      this.output.brow = this.targetOffset.brow
      this.output.eyeOpen = this.targetOffset.eyeOpen
      this.output.angleY = this.targetOffset.angleY
      this.output.angleZ = this.targetOffset.angleZ
      this.output.body = this.targetOffset.body
      return this.output
    }
    this.output.brow = stepRelease(
      this.output.brow,
      this.targetOffset.brow,
      dt,
      RELEASE_RATE * responseScale,
    )
    this.output.eyeOpen = stepRelease(
      this.output.eyeOpen,
      this.targetOffset.eyeOpen,
      dt,
      RELEASE_RATE * responseScale,
    )
    this.output.angleY = stepRelease(
      this.output.angleY,
      this.targetOffset.angleY,
      dt,
      RELEASE_RATE * responseScale,
    )
    this.output.angleZ = stepPose(
      this.output.angleZ,
      this.targetOffset.angleZ,
      dt,
      5.2 * responseScale,
    )
    this.output.body = stepPose(
      this.output.body,
      this.targetOffset.body,
      dt,
      4.2 * responseScale,
    )
    return this.output
  }
}

function writeOffset(
  output: CoSpeechExpressionOffset,
  phraseActivity: number,
  browAccent: number,
  headAccent: number,
  timeSeconds: number,
  gestureDirection: number,
  quality?: Readonly<BehaviorQuality>,
): Readonly<CoSpeechExpressionOffset> {
  const activity = unitInterval(phraseActivity)
  const browBeat = unitInterval(browAccent)
  const headBeat = unitInterval(headAccent)
  // Two incommensurate, low-frequency components keep conversational weight
  // transfer alive without a repeated left-right metronome. Accent direction
  // alternates per beat, matching the head stroke with a small torso carry.
  const tempo = relativeQuality(quality?.tempo, 1, 0.72)
  const density = relativeQuality(quality?.density, 0.8, 0.18)
  const directness = relativeQuality(quality?.directness, 0.72, -0.22)
  const asymmetry = relativeQuality(quality?.asymmetry, 0.2, 0.24)
  const rebound = relativeQuality(quality?.rebound, 0.35, 0.18)
  const motionTime = timeSeconds * tempo * density
  const drift =
    Math.sin(motionTime * 1.17 + 0.6) * 0.62 +
    Math.sin(motionTime * 0.43 + 2.1) * 0.38
  const carry =
    Math.sin(motionTime * 0.71 + 2.4) * 0.7 +
    Math.sin(motionTime * 0.31 + 0.1) * 0.3
  output.brow = (0.025 * activity + 0.07 * browBeat) * density
  output.eyeOpen = -0.018 * activity + 0.014 * browBeat
  output.angleY = (0.09 * headBeat) / directness
  output.angleZ =
    (0.075 * activity * drift * directness +
      0.09 * headBeat * gestureDirection * rebound) *
    asymmetry
  output.body =
    (0.22 * activity * carry * directness +
      0.2 * headBeat * gestureDirection * rebound) *
    density
  return output
}

function stepRelease(
  current: number,
  target: number,
  dt: number,
  rate: number,
): number {
  if (Math.abs(target) >= Math.abs(current) - 1e-6) return target
  return current + (target - current) * (1 - Math.exp(-rate * dt))
}

function stepPose(
  current: number,
  target: number,
  dt: number,
  response: number,
): number {
  return current + (target - current) * (1 - Math.exp(-response * dt))
}

function unitInterval(value: number): number {
  if (!Number.isFinite(value)) return 0
  return Math.max(0, Math.min(1, value))
}

function qualityResponseScale(
  quality: Readonly<BehaviorQuality> | undefined,
): number {
  const tempo = relativeQuality(quality?.tempo, 1, 0.72)
  const directness = relativeQuality(quality?.directness, 0.72, 0.22)
  const fluidity = relativeQuality(quality?.fluidity, 0.8, 0.28)
  return clamp((tempo * directness) / fluidity, 0.65, 1.45)
}

function relativeQuality(
  value: number | undefined,
  neutral: number,
  influence: number,
): number {
  const resolved =
    typeof value === 'number' && Number.isFinite(value) ? value : neutral
  return clamp(1 + (resolved - neutral) * influence, 0.65, 1.4)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
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

function smootherstep(value: number): number {
  const bounded = unitInterval(value)
  return bounded * bounded * bounded * (bounded * (bounded * 6 - 15) + 10)
}
