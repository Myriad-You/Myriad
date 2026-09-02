export interface ThinkingMotionPose {
  angleX: number
  angleY: number
  angleZ: number
  eyeX: number
  eyeY: number
  brow: number
  mouthCY: number
  mouthCAng: number
  mouthScale: number
}

type RandomSource = () => number

const ZERO_POSE: ThinkingMotionPose = {
  angleX: 0,
  angleY: 0,
  angleZ: 0,
  eyeX: 0,
  eyeY: 0,
  brow: 0,
  mouthCY: 0,
  mouthCAng: 0,
  mouthScale: 0,
}

/**
 * A constrained thinking loop: the eyes select a nearby thought point first,
 * then the head, brow, and closed-mouth shape settle after it. The output
 * object is reused so the render loop does not allocate.
 */
export class ThinkingMotionController {
  private readonly from: ThinkingMotionPose = { ...ZERO_POSE }
  private readonly to: ThinkingMotionPose = { ...ZERO_POSE }
  private readonly output: ThinkingMotionPose = { ...ZERO_POSE }
  private initialized = false
  private enabled = false
  private transitionStartedAt = 0
  private eyeDuration = 0.28
  private headDelay = 0.12
  private headDuration = 0.72
  private faceDelay = 0.18
  private faceDuration = 0.62
  private nextShiftAt = Number.POSITIVE_INFINITY
  private direction = 1

  constructor(private readonly random: RandomSource = Math.random) {}

  sample(timeSeconds: number, enabled: boolean): Readonly<ThinkingMotionPose> {
    const now = finiteTime(timeSeconds)
    if (!this.initialized) {
      this.initialized = true
      this.enabled = enabled
      if (enabled) this.nextShiftAt = now + this.randomRange(0.18, 0.32)
    }

    this.resolve(now)
    if (enabled !== this.enabled) {
      this.enabled = enabled
      if (enabled) {
        // Let the authored thinking pose settle before the first visible
        // refocus instead of changing both layers on the same frame.
        this.nextShiftAt = now + this.randomRange(0.18, 0.32)
      } else {
        this.beginTransition(now, ZERO_POSE, {
          eyeDuration: 0.24,
          headDelay: 0.04,
          headDuration: 0.48,
          faceDelay: 0,
          faceDuration: 0.36,
        })
        this.nextShiftAt = Number.POSITIVE_INFINITY
      }
    }

    if (this.enabled && now >= this.nextShiftAt) {
      const scheduledAt = this.nextShiftAt
      this.beginThoughtShift(scheduledAt)
      return this.resolve(now)
    }
    return this.output
  }

  private beginThoughtShift(now: number): void {
    this.direction *= -1
    const direction = this.direction
    const pursing = direction < 0
    const target: ThinkingMotionPose = {
      angleX: direction * this.randomRange(0.08, 0.14),
      angleY: this.randomRange(-0.09, 0.14),
      angleZ: direction * this.randomRange(0.07, 0.13),
      eyeX: direction * this.randomRange(0.16, 0.27),
      eyeY: this.randomRange(-0.13, 0.11),
      brow: this.randomRange(-0.025, 0.055),
      mouthCY: this.randomRange(-0.025, 0.035),
      mouthCAng: direction * this.randomRange(pursing ? 0.035 : 0.015, 0.06),
      mouthScale: pursing
        ? -this.randomRange(0.045, 0.075)
        : this.randomRange(-0.01, 0.02),
    }
    const eyeDuration = this.randomRange(0.28, 0.38)
    const headDelay = this.randomRange(0.09, 0.15)
    const headDuration = this.randomRange(0.5, 0.72)
    const faceDelay = headDelay + this.randomRange(0.04, 0.1)
    const faceDuration = this.randomRange(0.48, 0.7)
    this.beginTransition(now, target, {
      eyeDuration,
      headDelay,
      headDuration,
      faceDelay,
      faceDuration,
    })
    const settleEnd = Math.max(
      eyeDuration,
      headDelay + headDuration,
      faceDelay + faceDuration,
    )
    this.nextShiftAt = now + settleEnd + this.randomRange(1.6, 2.6)
  }

  private beginTransition(
    now: number,
    target: Readonly<ThinkingMotionPose>,
    timing: {
      eyeDuration: number
      headDelay: number
      headDuration: number
      faceDelay: number
      faceDuration: number
    },
  ): void {
    copyPose(this.from, this.output)
    copyPose(this.to, target)
    this.transitionStartedAt = now
    this.eyeDuration = timing.eyeDuration
    this.headDelay = timing.headDelay
    this.headDuration = timing.headDuration
    this.faceDelay = timing.faceDelay
    this.faceDuration = timing.faceDuration
  }

  private resolve(now: number): Readonly<ThinkingMotionPose> {
    const elapsed = Math.max(0, now - this.transitionStartedAt)
    const eyeXProgress = smootherstep(elapsed / this.eyeDuration)
    const eyeYProgress = smootherstep(
      (elapsed - 0.015) / (this.eyeDuration * 1.08),
    )
    const headProgress = settlingProgress(
      (elapsed - this.headDelay) / this.headDuration,
    )
    const verticalHeadProgress = settlingProgress(
      (elapsed - this.headDelay - 0.035) / (this.headDuration * 1.08),
    )
    const faceProgress = smootherstep(
      (elapsed - this.faceDelay) / this.faceDuration,
    )
    const mouthProgress = smootherstep(
      clamp(
        (elapsed - this.faceDelay - 0.08) / (this.faceDuration * 1.25),
        0,
        1,
      ) ** 1.15,
    )
    this.output.eyeX = mix(this.from.eyeX, this.to.eyeX, eyeXProgress)
    this.output.eyeY = mix(this.from.eyeY, this.to.eyeY, eyeYProgress)
    this.output.angleX = mix(this.from.angleX, this.to.angleX, headProgress)
    this.output.angleY = mix(
      this.from.angleY,
      this.to.angleY,
      verticalHeadProgress,
    )
    this.output.angleZ = mix(this.from.angleZ, this.to.angleZ, headProgress)
    this.output.brow = mix(this.from.brow, this.to.brow, faceProgress)
    this.output.mouthCY = mix(this.from.mouthCY, this.to.mouthCY, mouthProgress)
    this.output.mouthCAng = mix(
      this.from.mouthCAng,
      this.to.mouthCAng,
      mouthProgress,
    )
    this.output.mouthScale = mix(
      this.from.mouthScale,
      this.to.mouthScale,
      mouthProgress,
    )
    return this.output
  }

  private randomRange(minimum: number, maximum: number): number {
    return minimum + (maximum - minimum) * this.randomUnit()
  }

  private randomUnit(): number {
    const value = this.random()
    return Number.isFinite(value) ? clamp(value, 0, 1) : 0.5
  }
}

function copyPose(
  target: ThinkingMotionPose,
  source: Readonly<ThinkingMotionPose>,
): void {
  target.angleX = source.angleX
  target.angleY = source.angleY
  target.angleZ = source.angleZ
  target.eyeX = source.eyeX
  target.eyeY = source.eyeY
  target.brow = source.brow
  target.mouthCY = source.mouthCY
  target.mouthCAng = source.mouthCAng
  target.mouthScale = source.mouthScale
}

function smootherstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * bounded * (bounded * (bounded * 6 - 15) + 10)
}

/** A zero-velocity head start with a restrained inertial settle near the end. */
function settlingProgress(value: number): number {
  const bounded = clamp(value, 0, 1)
  if (bounded === 0 || bounded === 1) return bounded
  const decay = 5.2
  const angularRate = 4.4
  const response =
    1 -
    Math.exp(-decay * bounded) *
      (Math.cos(angularRate * bounded) +
        (decay / angularRate) * Math.sin(angularRate * bounded))
  const endpoint =
    1 -
    Math.exp(-decay) *
      (Math.cos(angularRate) + (decay / angularRate) * Math.sin(angularRate))
  return response / endpoint
}

function mix(from: number, to: number, amount: number): number {
  return from + (to - from) * amount
}

function finiteTime(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
