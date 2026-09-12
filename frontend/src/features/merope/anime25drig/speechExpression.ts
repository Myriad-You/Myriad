import type { BehaviorQuality } from '../motion/behavior'
import type { SpeechProsodyPlan } from '../speech/prosody'
import type { CoSpeechGestureMix } from './behaviorMotion'

export const SPEECH_ACCENT_BROW_ATTACK = 0.13
export const SPEECH_ACCENT_HEAD_DELAY = 0.09
export const SPEECH_ACCENT_HEAD_ATTACK = 0.19
export const SPEECH_ACCENT_BROW_RELEASE = 0.2
export const SPEECH_ACCENT_HEAD_RELEASE = 0.22
export const SPEECH_TEXT_ACCENT_ATTACK = 0.11
export const SPEECH_TEXT_ACCENT_RELEASE = 0.18

export interface CoSpeechExpressionOffset {
  brow: number
  eyeOpen: number
  angleY: number
  angleZ: number
  body: number
}

const RELEASE_RATE = 6.2

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

  private readonly rendered: CoSpeechExpressionOffset = { ...this.output }

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
    // Suppression removes the visual beat, not the speech plan.
    this.plannedAccents = plan.accents
      .filter((accent) => accent.gesture !== 'none')
      .map((accent) => ({
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
    gesture?: Readonly<CoSpeechGestureMix>,
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
      ? attackReleasePulse(
          elapsed,
          0,
          SPEECH_ACCENT_BROW_ATTACK,
          SPEECH_ACCENT_BROW_RELEASE,
        ) * this.accentIntensity
      : 0
    const authoredHead = active
      ? attackReleasePulse(
          elapsed,
          SPEECH_ACCENT_HEAD_DELAY,
          SPEECH_ACCENT_HEAD_ATTACK,
          SPEECH_ACCENT_HEAD_RELEASE,
        ) * this.accentIntensity
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
      return this.composeGesture(gesture)
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
    return this.composeGesture(gesture)
  }

  private composeGesture(
    gesture?: Readonly<CoSpeechGestureMix>,
  ): Readonly<CoSpeechExpressionOffset> {
    const question = gesture?.question ?? 0
    const contrast = gesture?.contrast ?? 0
    const laugh = gesture?.laugh ?? 0
    const pulse = gesture?.laughPulse ?? 0
    const hesitate = gesture?.hesitate ?? 0
    const tease = gesture?.tease ?? 0
    const checkIn = gesture?.['check-in'] ?? 0
    const generic =
      1 - Math.min(1, question + contrast + laugh + hesitate + tease + checkIn)
    // Do not put these normalized shares into the generic smoothing history
    this.rendered.brow =
      this.output.brow * generic +
      question * 0.1 +
      contrast * 0.055 +
      laugh * 0.035 +
      hesitate * 0.025 +
      tease * 0.065 +
      checkIn * 0.055
    this.rendered.eyeOpen =
      this.output.eyeOpen * generic +
      question * 0.025 -
      laugh * 0.14 -
      hesitate * 0.03 -
      tease * 0.06 +
      checkIn * 0.025
    this.rendered.angleY =
      this.output.angleY * generic -
      question * 0.085 +
      contrast * 0.07 +
      laugh * 0.035 +
      pulse * 0.09 +
      hesitate * 0.045 -
      tease * 0.065 +
      checkIn * 0.055
    this.rendered.angleZ =
      this.output.angleZ * generic +
      question * 0.19 -
      contrast * 0.16 +
      laugh * 0.07 -
      hesitate * 0.1 +
      tease * 0.14 +
      checkIn * 0.035
    this.rendered.body =
      this.output.body * generic +
      question * 0.16 -
      contrast * 0.28 +
      pulse * 0.23 -
      hesitate * 0.1 +
      tease * 0.18 +
      checkIn * 0.15
    return this.rendered
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
