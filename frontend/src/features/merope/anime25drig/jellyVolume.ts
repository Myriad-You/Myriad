/**
 * A soft element's own volume. Puffed sleeves and the head itself are not
 * rigid: when what they hang from starts or stops, the mass lags,
 * stretches away from its anchor or squashes into it, bulging sideways as it
 * does so because its area stays the same, and wobbles a few times before
 * it settles. This is what makes a drawing read as soft and bouncy, as opposed
 * to a rigid part that only swings. Hair has no volume of its own; it only
 * trails, which its strand springs already do.
 */
export interface JellyTuning {
  hz: number
  damping: number
  /** Anchor acceleration is read this many times over; a drawing under-reads real motion. */
  gain: number
  /** Stretch (+) or squash (−) along the element, as a share of its length; saturates softly. */
  stretchLimit: number
  /** Sideways lean of the free end, as a share of its length; saturates softly. */
  swayLimit: number
}

// Gains keep an ordinary brisk turn inside the linear range; the limits only
// catch a snap, so a wobble is never flattened against them.
export const HEAD_JELLY: Readonly<JellyTuning> = {
  hz: 3.4,
  damping: 0.22,
  gain: 1.6,
  stretchLimit: 0.03,
  swayLimit: 0,
}

/** A sleeve's swing is the arm pendulum's; the sleeve only adds its volume. */
export const SLEEVE_JELLY: Readonly<JellyTuning> = {
  hz: 3,
  damping: 0.22,
  gain: 4.5,
  stretchLimit: 0.07,
  swayLimit: 0,
}

/** Anchor acceleration above this is a discontinuity, not motion. */
const MAX_ANCHOR_ACCELERATION = 30_000
/** A mass cannot feel a single-frame kick; the drive is smoothed above this. */
const DRIVE_CUTOFF_HZ = 8
const MAX_STEP_SECONDS = 1 / 240

/** Where an element hangs from and which way it extends, in rest pixels. */
export interface JellyElement {
  anchorX: number
  anchorY: number
  /** Unit vector from the anchor toward the free end. */
  axisX: number
  axisY: number
  length: number
  /** Rest y of a canvas cut the element runs into; its edge stays on it. */
  cutY: number | null
}

export class JellyVolume {
  /** Current stretch (+) or squash (−) as a share of the element's length. */
  stretch = 0
  /** Current sideways lean of the free end as a share of the element's length. */
  sway = 0
  private along = 0
  private across = 0
  private alongVelocity = 0
  private acrossVelocity = 0
  private lastX = 0
  private lastY = 0
  private velocityX = 0
  private velocityY = 0
  private accelerationX = 0
  private accelerationY = 0
  private samples = 0

  constructor(private readonly tuning: Readonly<JellyTuning>) {}

  /**
   * `anchorX/Y` is where the anchor is drawn now; `axisX/Y` the element's
   * direction now. The mass is a spring hung on the anchor: it keeps going
   * when the anchor stops and stays behind when the anchor starts.
   */
  step(
    anchorX: number,
    anchorY: number,
    axisX: number,
    axisY: number,
    length: number,
    dt: number,
    dynamic: boolean,
  ): void {
    const elapsed = Number.isFinite(dt) ? Math.max(0, dt) : 0
    if (!dynamic || !Number.isFinite(anchorX) || !Number.isFinite(anchorY) || !(length > 0)) {
      this.reset(anchorX, anchorY)
      return
    }
    if (elapsed <= 0) return
    let accelerationX = 0
    let accelerationY = 0
    const velocityX = (anchorX - this.lastX) / elapsed
    const velocityY = (anchorY - this.lastY) / elapsed
    if (this.samples >= 2) {
      const blend = 1 - Math.exp(-2 * Math.PI * DRIVE_CUTOFF_HZ * elapsed)
      this.accelerationX += (clamp((velocityX - this.velocityX) / elapsed, MAX_ANCHOR_ACCELERATION) - this.accelerationX) * blend
      this.accelerationY += (clamp((velocityY - this.velocityY) / elapsed, MAX_ANCHOR_ACCELERATION) - this.accelerationY) * blend
      accelerationX = this.accelerationX
      accelerationY = this.accelerationY
    }
    if (this.samples >= 1) {
      this.velocityX = velocityX
      this.velocityY = velocityY
    }
    this.samples = Math.min(2, this.samples + 1)
    this.lastX = anchorX
    this.lastY = anchorY
    // The mass is pushed opposite the anchor's acceleration, in the element's own axes.
    const driveAlong = -this.tuning.gain * (accelerationX * axisX + accelerationY * axisY)
    const driveAcross = -this.tuning.gain * (-accelerationX * axisY + accelerationY * axisX)
    const omega = 2 * Math.PI * this.tuning.hz
    const damping = 2 * this.tuning.damping * omega
    const steps = Math.max(1, Math.ceil(elapsed / MAX_STEP_SECONDS))
    const h = elapsed / steps
    for (let step = 0; step < steps; step += 1) {
      this.alongVelocity += (driveAlong - omega * omega * this.along - damping * this.alongVelocity) * h
      this.along += this.alongVelocity * h
      this.acrossVelocity += (driveAcross - omega * omega * this.across - damping * this.acrossVelocity) * h
      this.across += this.acrossVelocity * h
    }
    if (![this.along, this.across, this.alongVelocity, this.acrossVelocity].every(Number.isFinite)) {
      this.reset(anchorX, anchorY)
      return
    }
    this.stretch = softLimit(this.along / length, this.tuning.stretchLimit)
    this.sway = softLimit(this.across / length, this.tuning.swayLimit)
  }

  private reset(anchorX: number, anchorY: number): void {
    this.along = 0
    this.across = 0
    this.alongVelocity = 0
    this.acrossVelocity = 0
    this.velocityX = 0
    this.velocityY = 0
    this.accelerationX = 0
    this.accelerationY = 0
    this.samples = Number.isFinite(anchorX) && Number.isFinite(anchorY) ? 1 : 0
    this.lastX = Number.isFinite(anchorX) ? anchorX : 0
    this.lastY = Number.isFinite(anchorY) ? anchorY : 0
    this.stretch = 0
    this.sway = 0
  }
}

export interface JellyDisplacement {
  x: number
  y: number
}

/**
 * How far the rest point moves under the element's current stretch and sway.
 * Nothing moves at the anchor; the free end takes all of it. Stretching along
 * the element narrows it across and squashing widens it, so its area holds.
 */
export function jellyDisplacement(
  restX: number,
  restY: number,
  element: Readonly<JellyElement>,
  stretch: number,
  sway: number,
  output: JellyDisplacement,
): JellyDisplacement {
  output.x = 0
  output.y = 0
  if ((stretch === 0 && sway === 0) || !(element.length > 0)) return output
  const dx = restX - element.anchorX
  const dy = restY - element.anchorY
  const along = dx * element.axisX + dy * element.axisY
  const across = -dx * element.axisY + dy * element.axisX
  let weight = smoothstep(along / element.length)
  if (element.cutY !== null) {
    // A cut edge stays on the canvas cut; the mass bulges above it instead.
    weight *= smoothstep((element.cutY - restY) / (element.length * CUT_RELEASE))
  }
  if (weight <= 0) return output
  const scale = 1 + stretch * weight
  const alongShift = along * (scale - 1)
  const acrossShift = across * (1 / scale - 1) + sway * weight * element.length
  output.x = alongShift * element.axisX - acrossShift * element.axisY
  output.y = alongShift * element.axisY + acrossShift * element.axisX
  return output
}

/** Share of the element's length above a canvas cut over which it is let go of. */
const CUT_RELEASE = 0.35

function softLimit(value: number, limit: number): number {
  if (!(limit > 0)) return 0
  return limit * Math.tanh(value / limit)
}

function clamp(value: number, limit: number): number {
  return Math.max(-limit, Math.min(limit, value))
}

function smoothstep(value: number): number {
  const t = Math.max(0, Math.min(1, value))
  return t * t * (3 - 2 * t)
}
