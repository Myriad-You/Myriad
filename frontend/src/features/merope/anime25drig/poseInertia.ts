import type { Anime25DDriver } from './driver'

/**
 * Head and body carry weight. Every source (cues, co-speech, the score, face
 * shifts, hand-offs between them) writes the composed pose; this last stage
 * lets the head and torso follow it through a critically damped spring with
 * a speed cap, so no source and no hand-off can jerk them faster than a body
 * moves. Without it a cue reached a 20° tilt in about 0.15 s (over 200°/s).
 */

/** Natural frequency, rad/s: settles a step in about a fifth of a second. */
const STIFFNESS = 24

/** Speed caps in driver units per second (head angles: 1 unit is 30°). */
const SPEED_CAPS = {
  angleX: 8.5,
  angleY: 7,
  angleZ: 6,
  body: 5,
  torsoTurn: 5,
  bodyLift: 5,
} as const satisfies Partial<Record<keyof Anime25DDriver, number>>

type CarriedKey = keyof typeof SPEED_CAPS
const CARRIED = Object.keys(SPEED_CAPS) as CarriedKey[]

export class Anime25DPoseInertia {
  private readonly position = new Map<CarriedKey, number>()
  private readonly velocity = new Map<CarriedKey, number>()

  /** Replaces the carried channels of `pose` with where the body has got to. */
  step(pose: Anime25DDriver, dt: number): void {
    if (!(dt > 0)) return
    for (const key of CARRIED) {
      const target = pose[key]
      const at = this.position.get(key)
      if (at === undefined) {
        this.position.set(key, target)
        this.velocity.set(key, 0)
        continue
      }
      const v = this.velocity.get(key)!
      const accel = STIFFNESS * STIFFNESS * (target - at) - 2 * STIFFNESS * v
      const cap = SPEED_CAPS[key]
      const next = Math.max(-cap, Math.min(cap, v + accel * dt))
      const moved = at + next * dt
      this.velocity.set(key, next)
      this.position.set(key, moved)
      pose[key] = moved
    }
  }
}
