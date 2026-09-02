export interface AmbientPose {
  angleX: number
  angleY: number
  angleZ: number
  body: number
  eyeX: number
  eyeY: number
}

type RandomSource = () => number

const ZERO_POSE: AmbientPose = {
  angleX: 0,
  angleY: 0,
  angleZ: 0,
  body: 0,
  eyeX: 0,
  eyeY: 0,
}

/**
 * Low-frequency ambient acting with eye lead, head follow and body lag.
 * The returned object is reused so the animation loop does not allocate.
 */
export class AmbientMotionController {
  private readonly from: AmbientPose = { ...ZERO_POSE }
  private readonly to: AmbientPose = { ...ZERO_POSE }
  private readonly output: AmbientPose = { ...ZERO_POSE }
  private initialized = false
  private enabled = false
  private transitionStartedAt = 0
  private eyeDuration = 0.2
  private headDelay = 0.08
  private headDuration = 0.8
  private bodyDelay = 0.2
  private bodyDuration = 1
  private nextPoseAt = Number.POSITIVE_INFINITY
  private lastDirection = 0

  constructor(private readonly random: RandomSource = Math.random) {}

  sample(timeSeconds: number, enabled: boolean): Readonly<AmbientPose> {
    const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
    if (!this.initialized) {
      this.initialized = true
      this.enabled = enabled
      if (enabled) this.nextPoseAt = now + this.randomRange(1.2, 2.4)
    }

    this.resolve(now)
    if (enabled !== this.enabled) {
      this.enabled = enabled
      if (enabled) {
        // Let the current rest (or leftover pose) finish, then glance again.
        this.nextPoseAt = now + this.randomRange(0.32, 0.85)
      } else {
        this.beginTransition(now, ZERO_POSE, {
          eyeDuration: 0.3,
          headDelay: 0,
          headDuration: 0.58,
          bodyDelay: 0.08,
          bodyDuration: 0.82,
        })
        this.nextPoseAt = Number.POSITIVE_INFINITY
      }
    }

    if (this.enabled && now >= this.nextPoseAt) {
      const scheduledAt = this.nextPoseAt
      this.beginRandomPose(scheduledAt)
      return this.resolve(now)
    }
    return this.output
  }

  private beginRandomPose(now: number): void {
    const target = this.nextPose()
    const headTravel = Math.max(
      Math.abs(target.angleX - this.output.angleX) / 0.3,
      Math.abs(target.angleY - this.output.angleY) / 0.18,
      Math.abs(target.angleZ - this.output.angleZ) / 0.12,
    )
    const headDuration =
      0.58 + Math.min(1.25, headTravel) * 0.48 + this.randomRange(0, 0.18)
    const headDelay = this.randomRange(0.06, 0.14)
    const bodyDelay = this.randomRange(0.16, 0.3)
    const bodyDuration = headDuration * this.randomRange(1.08, 1.22)
    const eyeDuration = this.randomRange(0.16, 0.28)
    this.beginTransition(now, target, {
      eyeDuration,
      headDelay,
      headDuration,
      bodyDelay,
      bodyDuration,
    })

    const transitionEnd = Math.max(
      eyeDuration,
      headDelay + headDuration,
      bodyDelay + bodyDuration,
    )
    let holdDuration = 2.4 + (this.randomUnit() + this.randomUnit()) * 1.15
    if (this.randomUnit() < 0.14) holdDuration += 1.6
    holdDuration *= 0.9
    this.nextPoseAt = now + transitionEnd + holdDuration
  }

  private beginTransition(
    now: number,
    target: Readonly<AmbientPose>,
    timing: {
      eyeDuration: number
      headDelay: number
      headDuration: number
      bodyDelay: number
      bodyDuration: number
    },
  ): void {
    copyPose(this.from, this.output)
    copyPose(this.to, target)
    this.transitionStartedAt = now
    this.eyeDuration = timing.eyeDuration
    this.headDelay = timing.headDelay
    this.headDuration = timing.headDuration
    this.bodyDelay = timing.bodyDelay
    this.bodyDuration = timing.bodyDuration
  }

  private resolve(now: number): Readonly<AmbientPose> {
    const elapsed = Math.max(0, now - this.transitionStartedAt)
    const eyeProgress = smootherstep(elapsed / this.eyeDuration)
    const headProgress = smootherstep(
      (elapsed - this.headDelay) / this.headDuration,
    )
    const bodyProgress = smootherstep(
      (elapsed - this.bodyDelay) / this.bodyDuration,
    )
    this.output.eyeX = mix(this.from.eyeX, this.to.eyeX, eyeProgress)
    this.output.eyeY = mix(this.from.eyeY, this.to.eyeY, eyeProgress)
    this.output.angleX = mix(this.from.angleX, this.to.angleX, headProgress)
    this.output.angleY = mix(this.from.angleY, this.to.angleY, headProgress)
    this.output.angleZ = mix(this.from.angleZ, this.to.angleZ, headProgress)
    this.output.body = mix(this.from.body, this.to.body, bodyProgress)
    return this.output
  }

  private nextPose(): AmbientPose {
    if (poseMagnitude(this.to) > 0.08 && this.randomUnit() < 0.32) {
      return { ...ZERO_POSE }
    }
    const direction = this.nextDirection()
    const style = this.randomUnit()
    if (style < 0.52) return this.horizontalGlance(direction)
    if (style < 0.76) return this.verticalAttention(direction)
    return this.listeningTilt(direction)
  }

  private horizontalGlance(direction: number): AmbientPose {
    const turn = this.randomRange(0.15, 0.38)
    const vertical = this.randomRange(-0.075, 0.075)
    return {
      angleX: direction * turn,
      angleY: vertical,
      angleZ: -direction * this.randomRange(0.025, 0.07),
      body: direction * this.randomRange(0.03, 0.1),
      eyeX: clamp(direction * (turn * 1.45 + 0.045), -0.6, 0.6),
      eyeY: clamp(vertical * 1.35, -0.25, 0.25),
    }
  }

  private verticalAttention(direction: number): AmbientPose {
    const verticalDirection = this.randomUnit() < 0.62 ? 1 : -1
    const vertical = verticalDirection * this.randomRange(0.09, 0.23)
    const turn = direction * this.randomRange(0.03, 0.09)
    return {
      angleX: turn,
      angleY: vertical,
      angleZ: direction * this.randomRange(0.02, 0.05),
      body: direction * this.randomRange(0.02, 0.055),
      eyeX: turn * 1.3,
      eyeY: clamp(vertical * 1.45, -0.34, 0.34),
    }
  }

  private listeningTilt(direction: number): AmbientPose {
    const vertical = this.randomRange(-0.04, 0.04)
    const turn = direction * this.randomRange(0.05, 0.115)
    return {
      angleX: turn,
      angleY: vertical,
      angleZ: direction * this.randomRange(0.07, 0.145),
      body: -direction * this.randomRange(0.045, 0.1),
      eyeX: turn * 1.25,
      eyeY: vertical * 1.2,
    }
  }

  private nextDirection(): number {
    let direction = this.randomUnit() < 0.5 ? -1 : 1
    if (direction === this.lastDirection && this.randomUnit() < 0.65) {
      direction = -direction
    }
    this.lastDirection = direction
    return direction
  }

  private randomRange(minimum: number, maximum: number): number {
    return minimum + (maximum - minimum) * this.randomUnit()
  }

  private randomUnit(): number {
    const value = this.random()
    return Number.isFinite(value) ? clamp(value, 0, 1) : 0.5
  }
}

function copyPose(target: AmbientPose, source: Readonly<AmbientPose>): void {
  target.angleX = source.angleX
  target.angleY = source.angleY
  target.angleZ = source.angleZ
  target.body = source.body
  target.eyeX = source.eyeX
  target.eyeY = source.eyeY
}

function poseMagnitude(pose: Readonly<AmbientPose>): number {
  return Math.max(
    Math.abs(pose.angleX),
    Math.abs(pose.angleY),
    Math.abs(pose.angleZ),
    Math.abs(pose.body),
  )
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
