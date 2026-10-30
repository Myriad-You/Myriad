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
  private nextCheckAt = Number.POSITIVE_INFINITY
  private checking = false
  private hasThought = false

  constructor(private readonly random: RandomSource = Math.random) {}

  sample(
    timeSeconds: number,
    enabled: boolean,
    baseGaze = { eyeX: 0, eyeY: 0 },
  ): Readonly<ThinkingMotionPose> {
    const now = finiteTime(timeSeconds)
    if (!this.initialized) {
      this.initialized = true
      this.enabled = enabled
      if (enabled) this.nextShiftAt = now + this.randomRange(0.18, 0.32)
      if (enabled) this.nextCheckAt = now + this.randomRange(6, 10)
    }

    this.resolve(now)
    if (enabled !== this.enabled) {
      this.enabled = enabled
      if (enabled) {
        this.nextShiftAt = now + this.randomRange(0.18, 0.32)
        this.nextCheckAt = now + this.randomRange(6, 10)
        this.checking = false
        this.hasThought = false
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
      // A delayed frame starts from the displayed pose, not an already elapsed transition.
      this.beginThoughtShift(
        now - this.nextShiftAt > 0.1 ? now : this.nextShiftAt,
        baseGaze,
      )
      return this.resolve(now)
    }
    return this.output
  }

  private beginThoughtShift(
    now: number,
    baseGaze: { eyeX: number; eyeY: number },
  ): void {
    const returning = this.checking
    this.checking = !returning && now >= this.nextCheckAt
    if (returning) this.nextCheckAt = now + this.randomRange(6, 12)
    const moveHead =
      !this.hasThought || (!this.checking && this.randomUnit() < 0.25)
    const moveFace =
      !this.hasThought || (!this.checking && this.randomUnit() < 0.35)
    const pursing = this.randomUnit() < 0.4
    const target: ThinkingMotionPose = {
      // Eyes may inspect another point; the head keeps its thoughtful lean.
      // Do not mirror the head every time the gaze changes sides.
      angleX: moveHead ? -this.randomRange(0.035, 0.065) : this.output.angleX,
      angleY: moveHead ? this.randomRange(0.015, 0.05) : this.output.angleY,
      angleZ: moveHead ? -this.randomRange(0.035, 0.065) : this.output.angleZ,
      // A check-in cancels the authored gaze bias rather than merely zeroing
      // this overlay (which would leave the character looking off-screen).
      eyeX: this.checking ? -baseGaze.eyeX : -this.randomRange(0.16, 0.24),
      eyeY: this.checking ? -baseGaze.eyeY : this.randomRange(-0.06, 0.04),
      brow: moveFace ? this.randomRange(-0.025, 0.055) : this.output.brow,
      mouthCY: moveFace ? this.randomRange(-0.025, 0.035) : this.output.mouthCY,
      mouthCAng: moveFace
        ? this.randomRange(-0.035, 0.035)
        : this.output.mouthCAng,
      mouthScale: !moveFace
        ? this.output.mouthScale
        : pursing
          ? -this.randomRange(0.045, 0.075)
          : this.randomRange(-0.01, 0.02),
    }
    const eyeDuration = this.randomRange(0.28, 0.38)
    const headDelay = this.randomRange(0.09, 0.15)
    const headDuration = this.randomRange(0.95, 1.25)
    const faceDelay = headDelay + this.randomRange(0.04, 0.1)
    const faceDuration = this.randomRange(0.48, 0.7)
    this.beginTransition(now, target, {
      eyeDuration,
      headDelay,
      headDuration,
      faceDelay,
      faceDuration,
    })
    this.hasThought = true
    const settleEnd = Math.max(
      eyeDuration,
      headDelay + headDuration,
      faceDelay + faceDuration,
    )
    this.nextShiftAt =
      now +
      (this.checking
        ? eyeDuration + this.randomRange(0.6, 1.1)
        : settleEnd + this.randomRange(1.8, 3.8))
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
