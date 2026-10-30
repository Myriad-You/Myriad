/**
 * A look carried a long way, head and eyes together, usually takes a blink
 * with it, starting as the look sets off; a glance does not. Blinks timed
 * only by a clock miss this, and a big turn with wide-open eyes reads as
 * mechanical.
 */

/** How far the eyes look for a full `eyeX`/`eyeY`, in `angleX`/`angleY` units. */
const EYE_SHARE = 0.5
/** The gaze is on its way above this speed (units per second) and has arrived below the other. */
const MOVING_SPEED = 0.6
const ARRIVED_SPEED = 0.25
/** A look that has travelled this far is a long one. */
export const GAZE_BLINK_TRAVEL = 0.32
/** Most long looks blink; some do not. */
export const GAZE_BLINK_CHANCE = 0.75
/** Eyes that have just opened from a blink do not shut again at once. */
const BLINK_REST_SECONDS = 0.9

export class GazeShiftBlink {
  private lastX = Number.NaN
  private lastY = Number.NaN
  private moving = false
  private travel = 0
  private decided = false
  private sinceBlink = Number.POSITIVE_INFINITY

  /**
   * Follows the gaze (head turn plus eyes) one frame on; true when this look
   * should start a blink now.
   */
  step(
    angleX: number,
    angleY: number,
    eyeX: number,
    eyeY: number,
    dt: number,
    blinking: boolean,
    random: () => number,
  ): boolean {
    const x = finite(angleX) + EYE_SHARE * finite(eyeX)
    const y = finite(angleY) + EYE_SHARE * finite(eyeY)
    const step = Number.isFinite(dt) ? Math.max(0, dt) : 0
    this.sinceBlink = blinking ? 0 : this.sinceBlink + step
    const first = !Number.isFinite(this.lastX)
    const moved = first ? 0 : Math.hypot(x - this.lastX, y - this.lastY)
    this.lastX = x
    this.lastY = y
    if (!(step > 0)) return false
    const speed = moved / step
    if (!this.moving) {
      if (speed < MOVING_SPEED) return false
      this.moving = true
      this.travel = 0
      this.decided = false
    } else if (speed < ARRIVED_SPEED) {
      this.moving = false
      return false
    }
    this.travel += moved
    if (this.decided || this.travel < GAZE_BLINK_TRAVEL) return false
    this.decided = true
    return this.sinceBlink >= BLINK_REST_SECONDS && random() < GAZE_BLINK_CHANCE
  }
}

function finite(value: number): number {
  return Number.isFinite(value) ? value : 0
}
