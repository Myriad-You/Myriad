import type { StylizedExpressionTargets } from './expressionRegistry'
import { resolveStylizedExpressionTargets } from './expressionRegistry'

export interface StylizedExpressionMotion {
  anger: number
  speechless: number
  maniac: number
  silly: number
  lovestruck: number
  brow: number
  browAngL: number
  browAngR: number
  browAngSym: number
  eyeOpen: number
  eyeX: number
  eyeY: number
  irisScale: number
  mouthForm: number
  mouthOpen: number
  mouthCY: number
  mouthCAng: number
  mouthScale: number
  maniacUpperMouthPulse: number
  maniacHeadPulse: number
  sillyEyeScale: number
  sillyIrisOffsetXL: number
  sillyIrisOffsetYL: number
  sillyIrisOffsetXR: number
  sillyIrisOffsetYR: number
  sillyMouthOpen: number
  sillyHeadPulse: number
  lovestruckHeartScale: number
  lovestruckFaceScale: number
  lovestruckDroolOffsetY: number
  lovestruckMouthOpen: number
  lovestruckMouthRound: number
  lovestruckMouthScale: number
  lovestruckHeadPulse: number
  angleX: number
  angleY: number
  angleZ: number
  body: number
  ambientScale: number
  angerMarkScale: number
  angerMarkOffsetY: number
  angerMarkRotation: number
  speechlessSweatScale: number
  speechlessSweatOffsetX: number
  speechlessSweatOffsetY: number
  speechlessSweatRotation: number
}

const ZERO_MOTION: StylizedExpressionMotion = {
  anger: 0,
  speechless: 0,
  maniac: 0,
  silly: 0,
  lovestruck: 0,
  brow: 0,
  browAngL: 0,
  browAngR: 0,
  browAngSym: 0,
  eyeOpen: 0,
  eyeX: 0,
  eyeY: 0,
  irisScale: 0,
  mouthForm: 0,
  mouthOpen: 0,
  mouthCY: 0,
  mouthCAng: 0,
  mouthScale: 0,
  maniacUpperMouthPulse: 0,
  maniacHeadPulse: 0,
  sillyEyeScale: 0,
  sillyIrisOffsetXL: 0,
  sillyIrisOffsetYL: 0,
  sillyIrisOffsetXR: 0,
  sillyIrisOffsetYR: 0,
  sillyMouthOpen: 0,
  sillyHeadPulse: 0,
  lovestruckHeartScale: 0,
  lovestruckFaceScale: 0,
  lovestruckDroolOffsetY: 0,
  lovestruckMouthOpen: 0,
  lovestruckMouthRound: 0,
  lovestruckMouthScale: 0,
  lovestruckHeadPulse: 0,
  angleX: 0,
  angleY: 0,
  angleZ: 0,
  body: 0,
  ambientScale: 1,
  angerMarkScale: 0,
  angerMarkOffsetY: 0,
  angerMarkRotation: 0,
  speechlessSweatScale: 0,
  speechlessSweatOffsetX: 0,
  speechlessSweatOffsetY: 0,
  speechlessSweatRotation: 0,
}

/** Stages semantic expression channels instead of cross-fading the whole face. */
export class StylizedExpressionMotionController {
  private readonly output: StylizedExpressionMotion = { ...ZERO_MOTION }
  private readonly targetInput: StylizedExpressionTargets = {
    anger: 0,
    speechless: 0,
    maniac: 0,
    silly: 0,
    lovestruck: 0,
  }

  private readonly exclusiveTargets: StylizedExpressionTargets = {
    anger: 0,
    speechless: 0,
    maniac: 0,
    silly: 0,
    lovestruck: 0,
  }

  private lastTime = Number.NaN
  private anger = 0
  private speechless = 0
  private maniac = 0
  private silly = 0
  private lovestruck = 0
  private angerStartedAt = 0
  private speechlessStartedAt = 0
  private maniacStartedAt = 0
  private sillyStartedAt = 0
  private lovestruckStartedAt = 0
  private sillyPhaseOffset = 0
  private sillyActivations = 0
  private angerWasActive = false
  private speechlessWasActive = false
  private maniacWasActive = false
  private sillyWasActive = false
  private lovestruckWasActive = false

  sample(
    timeSeconds: number,
    angerTarget: number,
    speechlessTarget: number,
    maniacTarget: number,
    sillyTarget = 0,
    lovestruckTarget = 0,
  ): Readonly<StylizedExpressionMotion> {
    const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
    const dt = Number.isFinite(this.lastTime)
      ? clamp(now - this.lastTime, 0, 0.05)
      : 0
    this.lastTime = now

    this.targetInput.anger = angerTarget
    this.targetInput.speechless = speechlessTarget
    this.targetInput.maniac = maniacTarget
    this.targetInput.silly = sillyTarget
    this.targetInput.lovestruck = lovestruckTarget
    resolveStylizedExpressionTargets(this.targetInput, this.exclusiveTargets)
    const boundedAnger = this.exclusiveTargets.anger
    const boundedSpeechless = this.exclusiveTargets.speechless
    const boundedManiac = this.exclusiveTargets.maniac
    const boundedSilly = this.exclusiveTargets.silly
    const boundedLovestruck = this.exclusiveTargets.lovestruck
    const angerActive = boundedAnger > 0.025
    const speechlessActive = boundedSpeechless > 0.025
    const maniacActive = boundedManiac > 0.025
    const sillyActive = boundedSilly > 0.025
    const lovestruckActive = boundedLovestruck > 0.025
    if (angerActive && !this.angerWasActive) this.angerStartedAt = now
    if (speechlessActive && !this.speechlessWasActive) {
      this.speechlessStartedAt = now
    }
    if (maniacActive && !this.maniacWasActive) this.maniacStartedAt = now
    if (sillyActive && !this.sillyWasActive) {
      this.sillyStartedAt = now
      this.sillyPhaseOffset = (this.sillyActivations * SILLY_PHASE_STRIDE) % 1
      this.sillyActivations += 1
    }
    if (lovestruckActive && !this.lovestruckWasActive) {
      this.lovestruckStartedAt = now
    }
    this.angerWasActive = angerActive
    this.speechlessWasActive = speechlessActive
    this.maniacWasActive = maniacActive
    this.sillyWasActive = sillyActive
    this.lovestruckWasActive = lovestruckActive

    this.anger = approach(
      this.anger,
      boundedAnger,
      dt,
      boundedAnger > this.anger ? 9.5 : 5.2,
    )
    this.speechless = approach(
      this.speechless,
      boundedSpeechless,
      dt,
      boundedSpeechless > this.speechless ? 7.8 : 4.2,
    )
    this.maniac = approach(
      this.maniac,
      boundedManiac,
      dt,
      boundedManiac > this.maniac ? 8.6 : 4.6,
    )
    this.silly = approach(
      this.silly,
      boundedSilly,
      dt,
      boundedSilly > this.silly ? 8.2 : 4.4,
    )
    this.lovestruck = approach(
      this.lovestruck,
      boundedLovestruck,
      dt,
      boundedLovestruck > this.lovestruck ? 7.4 : 3.8,
    )

    const angerAge = Math.max(0, now - this.angerStartedAt)
    const speechlessAge = Math.max(0, now - this.speechlessStartedAt)
    const maniacAge = Math.max(0, now - this.maniacStartedAt)
    const sillyAge = Math.max(0, now - this.sillyStartedAt)
    const lovestruckAge = Math.max(0, now - this.lovestruckStartedAt)
    const angerBrow = staged(this.anger, angerAge, 0, 0.12)
    const angerEyes = staged(this.anger, angerAge, 0.045, 0.15)
    const angerMouth = staged(this.anger, angerAge, 0.09, 0.18)
    const angerPose = staged(this.anger, angerAge, 0.14, 0.22)
    const speechlessGaze = staged(this.speechless, speechlessAge, 0, 0.14)
    const speechlessEyes = staged(this.speechless, speechlessAge, 0.055, 0.2)
    const speechlessFace = staged(this.speechless, speechlessAge, 0.11, 0.22)
    const speechlessPose = staged(this.speechless, speechlessAge, 0.18, 0.26)
    const maniacGaze = staged(this.maniac, maniacAge, 0, 0.16)
    const maniacMouth = staged(this.maniac, maniacAge, 0.055, 0.25)
    const maniacPose = staged(this.maniac, maniacAge, 0.11, 0.28)
    const sillyFace = staged(this.silly, sillyAge, 0.035, 0.2)
    const sillyPose = staged(this.silly, sillyAge, 0.13, 0.26)
    const lovestruckEyes = staged(this.lovestruck, lovestruckAge, 0.04, 0.24)
    const lovestruckFace = staged(this.lovestruck, lovestruckAge, 0.12, 0.46)
    const lovestruckPose = staged(this.lovestruck, lovestruckAge, 0.2, 0.38)
    const angerTension =
      Math.sin(now * 6.1) * 0.02 * angerPose +
      Math.sin(now * 2.15 + 0.8) * 0.009 * angerPose
    const speechlessDrift = Math.sin(now * 1.35 + 1.2) * speechlessPose
    const maniacWobble =
      Math.sin(now * 2.7 + 0.3) * 0.035 * maniacPose +
      Math.sin(now * 5.9 + 1.1) * 0.012 * maniacPose
    const maniacMouthPhase = (now / 0.82 + 0.17) % 1
    const maniacUpperMouthCycle =
      maniacLaughCurve(maniacMouthPhase) * maniacMouth
    const delayedManiacHead =
      maniacLaughCurve((maniacMouthPhase + 0.94) % 1) * maniacMouth
    // Keep the same laugh cadence and slight delay while giving the head its own amplitude.
    const maniacHeadCycle = delayedManiacHead * 2
    const sillyPhase =
      (sillyAge / SILLY_LOOP_SECONDS + this.sillyPhaseOffset) % 1
    const sillyIrisXL = loopedKeyframe(sillyPhase, SILLY_IRIS_X_LEFT)
    const sillyIrisYL = loopedKeyframe(sillyPhase, SILLY_IRIS_Y_LEFT)
    const sillyIrisXR = loopedKeyframe(sillyPhase, SILLY_IRIS_X_RIGHT)
    const sillyIrisYR = loopedKeyframe(sillyPhase, SILLY_IRIS_Y_RIGHT)
    const sillyMouthOpen = loopedKeyframe(sillyPhase, SILLY_MOUTH_OPEN)

    const output = this.output
    output.anger = this.anger
    output.speechless = this.speechless
    output.maniac = this.maniac
    output.silly = this.silly
    output.lovestruck = this.lovestruck
    output.brow =
      -0.31 * angerBrow -
      0.13 * speechlessFace +
      0.12 * maniacGaze +
      0.075 * sillyFace +
      0.13 * lovestruckFace
    output.browAngL =
      0.17 * speechlessFace - 0.2 * maniacGaze - 0.035 * sillyFace
    output.browAngR =
      -0.055 * speechlessFace + 0.14 * maniacGaze + 0.055 * sillyFace
    output.browAngSym = 0.72 * angerBrow - 0.24 * lovestruckFace
    output.eyeOpen =
      -0.23 * angerEyes -
      0.39 * speechlessEyes +
      0.055 * maniacGaze -
      0.44 * lovestruckEyes
    output.eyeX =
      -0.47 * speechlessGaze +
      (0.035 + Math.sin(now * 4.1) * 0.025) * maniacGaze
    output.eyeY =
      0.075 * speechlessGaze +
      (-0.73 + Math.sin(now * 3.7 + 0.8) * 0.035) * maniacGaze +
      0.07 * lovestruckEyes
    output.irisScale =
      -0.075 * angerEyes -
      0.025 * speechlessEyes -
      0.39 * maniacGaze +
      0.025 * lovestruckEyes
    output.mouthForm =
      -0.74 * angerMouth -
      0.33 * speechlessFace +
      0.72 * maniacMouth +
      0.05 * lovestruckFace
    output.mouthOpen = 0.96 * maniacMouth
    output.mouthCY =
      0.065 * angerMouth + 0.045 * speechlessFace + 0.055 * maniacMouth
    output.mouthCAng = -0.075 * speechlessFace
    output.mouthScale =
      -0.1 * angerMouth - 0.1 * speechlessFace + 0.018 * maniacMouth
    output.maniacUpperMouthPulse = maniacUpperMouthCycle
    output.maniacHeadPulse = maniacHeadCycle
    output.sillyEyeScale = symbolPop(this.silly, sillyAge, 0.035, 0.2)
    output.sillyIrisOffsetXL = sillyIrisXL * sillyFace
    output.sillyIrisOffsetYL = sillyIrisYL * sillyFace
    output.sillyIrisOffsetXR = sillyIrisXR * sillyFace
    output.sillyIrisOffsetYR = sillyIrisYR * sillyFace
    output.sillyMouthOpen = sillyMouthOpen * sillyFace
    output.sillyHeadPulse =
      ((sillyMouthOpen - 0.2) * 0.48 + Math.sin(sillyAge * 2.1) * 0.07) *
      sillyPose
    const lovestruckBreath = Math.sin(lovestruckAge * 1.7 + 0.4)
    output.lovestruckHeartScale =
      symbolPop(this.lovestruck, lovestruckAge, 0.075, 0.24) *
      (1 + lovestruckBreath * 0.07 * lovestruckFace)
    output.lovestruckFaceScale =
      symbolPop(this.lovestruck, lovestruckAge, 0.14, 0.42) *
      (1 + lovestruckBreath * 0.007)
    output.lovestruckDroolOffsetY =
      (0.7 + Math.sin(lovestruckAge * 1.35 + 1.1) * 0.32) * lovestruckFace
    output.lovestruckMouthOpen =
      (0.4 + lovestruckBreath * 0.035) * lovestruckFace
    output.lovestruckMouthRound = 0.58 * lovestruckFace
    output.lovestruckMouthScale = -0.035 * lovestruckFace
    output.lovestruckHeadPulse = lovestruckBreath * 0.12 * lovestruckPose
    output.angleX =
      0.05 * angerPose +
      Math.sin(now * 6.1 + 0.7) * 0.012 * angerPose -
      0.035 * speechlessPose -
      (0.035 + Math.sin(now * 1.7 + 0.2) * 0.012) * maniacPose +
      0.018 * sillyPose +
      0.018 * lovestruckPose
    output.angleY =
      0.085 * angerPose -
      0.025 * speechlessPose -
      0.055 * maniacPose -
      maniacHeadCycle * 2.1 +
      maniacWobble * 0.35 -
      0.025 * sillyPose -
      0.018 * lovestruckPose
    output.angleZ =
      angerTension +
      0.075 * speechlessPose -
      0.12 * maniacPose +
      maniacWobble +
      Math.sin(sillyAge * 0.9) * 0.018 * sillyPose +
      Math.sin(lovestruckAge * 0.72) * 0.014 * lovestruckPose
    output.body =
      0.075 * angerPose -
      0.035 * speechlessPose +
      0.045 * maniacPose -
      0.018 * sillyPose -
      0.022 * lovestruckPose
    output.ambientScale = clamp(
      1 -
        0.2 * this.anger -
        0.62 * this.speechless -
        0.38 * this.maniac -
        0.72 * this.silly -
        0.55 * this.lovestruck,
      0.32,
      1,
    )

    const angerMark = symbolPop(this.anger, angerAge, 0.11, 0.2)
    const angerBreath =
      1 +
      Math.sin(now * 2.45 + 0.25) * 0.08 +
      Math.max(0, Math.sin(now * 4.9 + 0.35)) ** 5 * 0.055 +
      Math.sin(now * 1.15) * 0.018
    output.angerMarkScale = angerMark * angerBreath
    output.angerMarkOffsetY =
      (-2.2 * (1 - smootherstep(Math.min(1, angerAge / 0.28))) +
        Math.sin(now * 2.45 + 0.25) * 0.7) *
      angerMark
    output.angerMarkRotation =
      (0.035 + Math.sin(now * 2.45 + 0.55) * 0.018) * angerMark

    const sweat = symbolPop(this.speechless, speechlessAge, 0.16, 0.24)
    output.speechlessSweatScale = sweat
    output.speechlessSweatOffsetX =
      (1.4 * (1 - smootherstep(Math.min(1, speechlessAge / 0.4))) +
        speechlessDrift * 0.25) *
      sweat
    output.speechlessSweatOffsetY =
      (2.6 * smootherstep(Math.min(1, speechlessAge / 0.7)) +
        speechlessDrift * 0.45) *
      sweat
    output.speechlessSweatRotation = -0.035 * sweat
    return output
  }
}

export const SILLY_LOOP_SECONDS = 6

export const SILLY_IRIS_DRIFT_LIMIT = { x: 0.12, y: 0.1275 } as const

const SILLY_PHASE_STRIDE = 0.37

type MotionKeyframe = readonly [phase: number, value: number]

const SILLY_IRIS_X_LEFT: readonly MotionKeyframe[] = [
  [0, 0],
  [0.12, 0.015],
  [0.28, 0.075],
  [0.43, 0.12],
  [0.58, 0.0825],
  [0.76, -0.06],
  [0.88, -0.0825],
  [1, 0],
]
const SILLY_IRIS_Y_LEFT: readonly MotionKeyframe[] = [
  [0, 0],
  [0.12, 0.01125],
  [0.28, 0.0825],
  [0.43, 0.1275],
  [0.58, 0.0825],
  [0.76, 0.0525],
  [0.88, 0.015],
  [1, 0],
]
const SILLY_IRIS_X_RIGHT: readonly MotionKeyframe[] = [
  [0, 0],
  [0.12, -0.01125],
  [0.28, 0.03375],
  [0.43, 0.06375],
  [0.58, 0.01875],
  [0.76, -0.04125],
  [0.88, -0.03375],
  [1, 0],
]
const SILLY_IRIS_Y_RIGHT: readonly MotionKeyframe[] = [
  [0, 0],
  [0.12, 0.0075],
  [0.28, 0.0525],
  [0.43, 0.07125],
  [0.58, -0.01125],
  [0.76, -0.03],
  [0.88, -0.015],
  [1, 0],
]
const SILLY_MOUTH_OPEN: readonly MotionKeyframe[] = [
  [0, 0.15],
  [0.08, 0.15],
  [0.14, 0.42],
  [0.21, 0.14],
  [0.31, 0.14],
  [0.37, 0.38],
  [0.44, 0.13],
  [0.58, 0.13],
  [0.64, 0.34],
  [0.71, 0.14],
  [0.78, 0.14],
  [0.85, 0.78],
  [0.94, 0.72],
  [1, 0.15],
]

function loopedKeyframe(
  phase: number,
  keyframes: readonly MotionKeyframe[],
): number {
  const bounded = clamp(phase, 0, 1)
  for (let index = 1; index < keyframes.length; index += 1) {
    const next = keyframes[index]
    if (bounded > next[0]) continue
    const previous = keyframes[index - 1]
    return lerpSmooth(
      previous[1],
      next[1],
      (bounded - previous[0]) / Math.max(0.0001, next[0] - previous[0]),
    )
  }
  return keyframes.at(-1)?.[1] ?? 0
}

function staged(
  amount: number,
  age: number,
  delay: number,
  duration: number,
): number {
  if (amount <= 0) return 0
  return amount * smootherstep((age - delay) / duration)
}

function symbolPop(
  amount: number,
  age: number,
  delay: number,
  duration: number,
): number {
  if (amount <= 0 || age <= delay) return 0
  const progress = clamp((age - delay) / duration, 0, 1)
  const back = 1.70158
  const shifted = progress - 1
  const overshoot = 1 + (back + 1) * shifted ** 3 + back * shifted ** 2
  return amount * clamp(overshoot, 0, 1.08)
}

function maniacLaughCurve(phase: number): number {
  if (phase < 0.17) {
    return lerpSmooth(0, 0.027, phase / 0.17)
  }
  if (phase < 0.5) {
    return lerpSmooth(0.027, -0.007, (phase - 0.17) / 0.33)
  }
  if (phase < 0.72) {
    return lerpSmooth(-0.007, 0.008, (phase - 0.5) / 0.22)
  }
  return lerpSmooth(0.008, 0, (phase - 0.72) / 0.28)
}

function lerpSmooth(start: number, end: number, progress: number): number {
  return start + (end - start) * smootherstep(progress)
}

function approach(
  current: number,
  target: number,
  dt: number,
  response: number,
): number {
  return current + (target - current) * (1 - Math.exp(-response * dt))
}

function smootherstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded ** 3 * (bounded * (bounded * 6 - 15) + 10)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
