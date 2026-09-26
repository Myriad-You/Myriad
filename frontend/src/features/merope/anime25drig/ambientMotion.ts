import { MinimumJerkMotion } from './minimumJerk'

export interface AmbientPose {
  angleX: number
  angleY: number
  angleZ: number
  body: number
  eyeX: number
  eyeY: number
}

/** Normalized rig-space, not physiological degrees or scene perception. */
export const AMBIENT_HEAD_GAZE_SHARE = { x: 0.62, y: 0.55 } as const

export const EYE_SACCADE_FLOOR_SECONDS = 0.17
export const EYE_SACCADE_SECONDS_PER_UNIT = 0.05

export class AmbientMotionController {
  private readonly gazeX = new MinimumJerkMotion()
  private readonly gazeY = new MinimumJerkMotion()
  private readonly headX = new MinimumJerkMotion()
  private readonly headY = new MinimumJerkMotion()
  private readonly headZ = new MinimumJerkMotion()
  private readonly body = new MinimumJerkMotion()
  private readonly output: AmbientPose = {
    angleX: 0,
    angleY: 0,
    angleZ: 0,
    body: 0,
    eyeX: 0,
    eyeY: 0,
  }

  private initialized = false
  private enabled = false
  private nextPoseAt = Number.POSITIVE_INFINITY
  private targetX = 0
  private targetY = 0
  private inspectionCount = 0
  private previousRegion = -1

  constructor(private readonly random: () => number = Math.random) {}

  sample(timeSeconds: number, enabled: boolean): Readonly<AmbientPose> {
    const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
    if (!this.initialized) {
      this.initialized = true
      this.enabled = enabled
      if (enabled) this.nextPoseAt = now + this.range(0.7, 1.4)
    }
    this.resolve(now)
    if (enabled !== this.enabled) {
      this.enabled = enabled
      if (enabled) {
        this.nextPoseAt = now + this.range(0.25, 0.65)
      } else {
        this.gazeX.retarget(now, 0, 0.35)
        this.gazeY.retarget(now, 0, 0.35)
        this.headX.retarget(now, 0, 0.65)
        this.headY.retarget(now, 0, 0.65)
        this.headZ.retarget(now, 0, 0.65)
        this.body.retarget(now, 0, 0.85)
        this.nextPoseAt = Number.POSITIVE_INFINITY
      }
    }
    // Resolve at scheduled times, not the late frame
    let steps = 0
    while (now >= this.nextPoseAt && steps < 8) {
      this.beginLook(this.nextPoseAt)
      steps += 1
    }
    if (enabled && now >= this.nextPoseAt)
      this.nextPoseAt = now + this.range(0.6, 1.6)
    return this.resolve(now)
  }

  private beginLook(now: number): void {
    this.resolve(now)
    const choice = this.unit()
    const inspect = this.inspectionCount < 3 && choice < 0.44
    if (inspect) {
      this.targetX = clamp(this.targetX + this.range(-0.23, 0.23), -1, 1)
      this.targetY = clamp(this.targetY + this.range(-0.18, 0.18), -0.7, 0.7)
      this.inspectionCount += 1
    } else if (choice < 0.64) {
      this.targetX = this.range(-0.24, 0.24)
      this.targetY = this.range(-0.18, 0.2)
      this.inspectionCount = 0
    } else {
      let region = Math.min(5, Math.floor(this.unit() * 6))
      if (region === this.previousRegion && this.unit() < 0.75) {
        region = (region + 1 + Math.floor(this.unit() * 5)) % 6
      }
      this.previousRegion = region
      const side = region % 2 === 0 ? -1 : 1
      this.targetX = side * this.range(0.48, 1)
      this.targetY =
        region < 2
          ? this.range(-0.2, 0.22)
          : region < 4
            ? this.range(0.25, 0.7)
            : this.range(-0.65, -0.2)
      this.inspectionCount = 0
    }

    const eccentricity = Math.hypot(this.targetX, this.targetY * 0.8)
    const recruitment = smoothstep((eccentricity - 0.12) / 0.8)
    const headShare = (0.22 + 0.72 * recruitment) * this.range(0.86, 1)
    const x = this.targetX * headShare
    const y = this.targetY * (0.35 + 0.5 * recruitment)
    const headTravel = Math.hypot(x - this.headX.value, y - this.headY.value)
    const eyeTravel = Math.hypot(
      this.targetX - this.gazeX.value,
      this.targetY - this.gazeY.value,
    )
    const eyeDuration =
      EYE_SACCADE_FLOOR_SECONDS +
      Math.min(1.8, eyeTravel) * EYE_SACCADE_SECONDS_PER_UNIT
    // Anime timing: a brisk move into a held pose. The follow-through layer
    // turns the brisk stop into a small overshoot and settle.
    const headDuration = 0.3 + Math.sqrt(headTravel) * this.range(0.34, 0.5)
    const latency = this.range(0.045, 0.13)
    this.gazeX.retarget(now, this.targetX, eyeDuration)
    this.gazeY.retarget(now, this.targetY, eyeDuration)
    this.headX.retarget(now, x, headDuration, latency)
    this.headY.retarget(now, y, headDuration, latency)
    let bodyDuration = 0
    if (!inspect) {
      this.headZ.retarget(
        now,
        this.range(-0.28, 0.28) - x * 0.08,
        headDuration * 1.1,
        latency,
      )
      // Small inspections stay eye/head-led; a broad look recruits the torso.
      // Give its larger travel time instead of accelerating it to catch up.
      const bodyTarget =
        x * this.range(0.3 + recruitment * 0.3, 0.5 + recruitment * 0.22) +
        this.range(-0.07, 0.07)
      bodyDuration = Math.max(
        headDuration * 1.3,
        0.4 + Math.sqrt(Math.abs(bodyTarget - this.body.value)) * 0.62,
      )
      this.body.retarget(now, bodyTarget, bodyDuration, latency + 0.12)
    }
    const dwell = clamp(
      Math.exp(this.range(-0.8, 0.9) + this.range(-0.65, 0.65)),
      0.4,
      4.2,
    )
    this.nextPoseAt =
      now +
      Math.max(
        headDuration + latency,
        bodyDuration + latency + 0.12,
        eyeDuration,
      ) +
      dwell * (inspect ? 0.75 : 1.2)
  }

  private resolve(now: number): Readonly<AmbientPose> {
    this.output.angleX = this.headX.sample(now)
    this.output.angleY = this.headY.sample(now)
    this.output.angleZ = this.headZ.sample(now)
    this.output.body = this.body.sample(now)
    this.output.eyeX = clamp(
      this.gazeX.sample(now) - this.output.angleX * AMBIENT_HEAD_GAZE_SHARE.x,
      -1,
      1,
    )
    this.output.eyeY = clamp(
      this.gazeY.sample(now) - this.output.angleY * AMBIENT_HEAD_GAZE_SHARE.y,
      -1,
      1,
    )
    return this.output
  }

  private range(min: number, max: number): number {
    return min + (max - min) * this.unit()
  }

  private unit(): number {
    const value = this.random()
    return Number.isFinite(value) ? clamp(value, 0, 1 - Number.EPSILON) : 0.5
  }
}

function smoothstep(value: number): number {
  const u = clamp(value, 0, 1)
  return u * u * (3 - 2 * u)
}
function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}
