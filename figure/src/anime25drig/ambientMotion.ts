import { MinimumJerkMotion } from '../math/minimumJerk'

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

/**
 * Share of a look's travel that is not covered by the move itself but glided
 * through slowly over the hold that follows, so a held look keeps settling
 * instead of parking dead still until the next one.
 */
export const LOOK_GLIDE_SHARE = { min: 0.1, max: 0.16 } as const

/**
 * How her looks go while she talks, fitted to people talking on camera (three
 * interviews, 790 s, eye line measured frame by frame): about 44–59 visible
 * head moves a minute, about 2–4° each, held about 0.4–0.6 s, the tilt
 * wandering 12–16° within ten seconds. A talker's tilt walks on from where it
 * is rather than starting over (`walk`); hers stays within about 9° of upright
 * (`reach`), wandering about 10° within ten seconds. The next look does not
 * wait for the tilt or the body to arrive. The idle pace stays slow.
 */
export const SPEAKING_LOOK = { duration: 0.5, dwell: 0.08, roll: 2.2, turn: 0.4, walk: 0.75, reach: 0.68 } as const

/**
 * A speaker's gaze around her turn (Kendon 1967 on): she looks away as she
 * starts, while she finds her words, and back at them as she finishes,
 * handing the turn over, and holds it there a moment.
 */
export const TURN_GAZE = { awayX: [0.35, 0.6], awayY: [-0.32, -0.1], handOverHold: 1.6 } as const

export const EYE_SACCADE_FLOOR_SECONDS = 0.17
export const EYE_SACCADE_SECONDS_PER_UNIT = 0.05

export class AmbientMotionController {
  private readonly gazeX = new MinimumJerkMotion()
  private readonly gazeY = new MinimumJerkMotion()
  private readonly headX = new MinimumJerkMotion()
  private readonly headY = new MinimumJerkMotion()
  private readonly headZ = new MinimumJerkMotion()
  private readonly body = new MinimumJerkMotion()
  // The slow remainder of each look, played across its whole hold.
  private readonly glideX = new MinimumJerkMotion()
  private readonly glideY = new MinimumJerkMotion()
  private readonly glideZ = new MinimumJerkMotion()
  private readonly glideBody = new MinimumJerkMotion()
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

  private speaking = false
  /** The turn edge the next look answers: starting to speak, or done. */
  private turn: 'start' | 'end' | null = null

  sample(timeSeconds: number, enabled: boolean, speaking = false): Readonly<AmbientPose> {
    if (speaking !== this.speaking && this.initialized && this.enabled && enabled) {
      this.turn = speaking ? 'start' : 'end'
      const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
      this.nextPoseAt = Math.min(this.nextPoseAt, now + (speaking ? 0.05 : 0.15))
    }
    this.speaking = speaking
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
        this.glideX.retarget(now, 0, 0.65)
        this.glideY.retarget(now, 0, 0.65)
        this.glideZ.retarget(now, 0, 0.65)
        this.glideBody.retarget(now, 0, 0.85)
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
    const turn = this.turn
    this.turn = null
    const choice = this.unit()
    const inspect = !turn && this.inspectionCount < 3 && choice < 0.44
    if (turn === 'start') {
      this.targetX = (this.unit() < 0.5 ? -1 : 1) * this.range(TURN_GAZE.awayX[0], TURN_GAZE.awayX[1])
      this.targetY = this.range(TURN_GAZE.awayY[0], TURN_GAZE.awayY[1])
      this.inspectionCount = 0
    } else if (turn === 'end') {
      this.targetX = this.range(-0.05, 0.05)
      this.targetY = this.range(-0.04, 0.06)
      this.inspectionCount = 0
    } else if (inspect) {
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
    const talk = this.speaking
    const x = this.targetX * headShare * (talk ? SPEAKING_LOOK.turn : 1)
    const y = this.targetY * (0.35 + 0.5 * recruitment)
    const headTravel = Math.hypot(x - this.output.angleX, y - this.output.angleY)
    const eyeTravel = Math.hypot(
      this.targetX - this.gazeX.value,
      this.targetY - this.gazeY.value,
    )
    const eyeDuration =
      EYE_SACCADE_FLOOR_SECONDS +
      Math.min(1.8, eyeTravel) * EYE_SACCADE_SECONDS_PER_UNIT
    const headDuration = (0.36 + Math.sqrt(headTravel) * this.range(0.42, 0.6)) * (talk ? SPEAKING_LOOK.duration : 1)
    const latency = this.range(0.045, 0.13)
    this.gazeX.retarget(now, this.targetX, eyeDuration)
    this.gazeY.retarget(now, this.targetY, eyeDuration)
    const glideShare = this.range(LOOK_GLIDE_SHARE.min, LOOK_GLIDE_SHARE.max)
    const glideX = (x - this.output.angleX) * glideShare
    const glideY = (y - this.output.angleY) * glideShare
    // The axes do not start and stop together: the turn leads, the nod comes
    // a touch later and slower, the tilt later still, and the torso last.
    const pitchLag = this.range(0.04, 0.12)
    const rollLag = this.range(0.12, 0.26)
    this.headX.retarget(now, clamp(x - glideX, -1, 1), headDuration, latency)
    this.headY.retarget(now, clamp(y - glideY, -1, 1), headDuration * 1.12, latency + pitchLag)
    let bodyDuration = 0
    let glideZ = Number.NaN
    let glideBody = Number.NaN
    if (!inspect || talk) {
      const fresh = this.range(-0.34, 0.34) * (talk ? SPEAKING_LOOK.roll : 1)
      const walk = talk ? SPEAKING_LOOK.walk : 0
      // A talker's tilt walks, but stays within a person's reach.
      const reach = talk ? SPEAKING_LOOK.reach : 1
      const roll =
        clamp((this.output.angleZ + x * 0.09) * walk + fresh * Math.sqrt(1 - walk * walk), -reach, reach) - x * 0.09
      glideZ = (roll - this.output.angleZ) * glideShare
      this.headZ.retarget(now, clamp(roll - glideZ, -1, 1), headDuration * (talk ? 1 : 1.35), latency + rollLag)
      // Small inspections stay eye/head-led; a broad look recruits the torso.
      // Give its larger travel time instead of accelerating it to catch up.
      const bodyTarget =
        x * this.range(0.3 + recruitment * 0.3, 0.5 + recruitment * 0.22) +
        this.range(-0.07, 0.07)
      bodyDuration = Math.max(
        headDuration * 1.3,
        0.5 + Math.sqrt(Math.abs(bodyTarget - this.output.body)) * 0.8,
      )
      glideBody = (bodyTarget - this.output.body) * glideShare
      this.body.retarget(now, clamp(bodyTarget - glideBody, -1, 1), bodyDuration, latency + rollLag + 0.06)
    }
    const dwell = clamp(
      Math.exp(this.range(-0.8, 0.9) + this.range(-0.65, 0.65)),
      0.4,
      4.2,
    )
    this.nextPoseAt =
      now +
      Math.max(
        headDuration * 1.12 + latency + pitchLag,
        inspect || talk ? 0 : headDuration * 1.35 + latency + rollLag,
        talk ? 0 : bodyDuration + latency + rollLag + 0.06,
        eyeDuration,
      ) +
      (turn === 'end' ? TURN_GAZE.handOverHold : dwell * (inspect ? 0.75 : 1.2) * (talk ? SPEAKING_LOOK.dwell : 1))
    // The remainder outlasts the look, so it is still settling when the next
    // look takes over from it and the head never parks.
    const glideDuration = (this.nextPoseAt - now) * 1.3
    this.glideX.retarget(now, glideX, glideDuration, latency)
    this.glideY.retarget(now, glideY, glideDuration, latency + pitchLag)
    if (Number.isFinite(glideZ)) this.glideZ.retarget(now, glideZ, glideDuration, latency + rollLag)
    if (Number.isFinite(glideBody)) this.glideBody.retarget(now, glideBody, glideDuration, latency + rollLag + 0.06)
  }

  private resolve(now: number): Readonly<AmbientPose> {
    this.output.angleX = clamp(this.headX.sample(now) + this.glideX.sample(now), -1, 1)
    this.output.angleY = clamp(this.headY.sample(now) + this.glideY.sample(now), -1, 1)
    this.output.angleZ = clamp(this.headZ.sample(now) + this.glideZ.sample(now), -1, 1)
    this.output.body = clamp(this.body.sample(now) + this.glideBody.sample(now), -1, 1)
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
