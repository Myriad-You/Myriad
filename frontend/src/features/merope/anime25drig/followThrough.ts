import type { Anime25DDriver } from './driver'

/**
 * Anime follow-through: a head that stops turning carries on a little past its
 * mark and springs back, instead of halting exactly on it. The spring is driven
 * by the primary pose's acceleration, so a held pose is shown exactly as
 * authored and only arrivals and departures bounce.
 */
export const FOLLOW_THROUGH_KEYS = ['angleX', 'angleY', 'angleZ', 'body'] as const
export type FollowThroughKey = (typeof FOLLOW_THROUGH_KEYS)[number]

export interface FollowThroughChannel {
  hz: number
  damping: number
  /** The overshoot saturates softly into this, in driver units. */
  limit: number
}

export const FOLLOW_THROUGH: Readonly<Record<FollowThroughKey, Readonly<FollowThroughChannel>>> = {
  angleX: { hz: 2.4, damping: 0.28, limit: 0.14 },
  angleY: { hz: 2.4, damping: 0.28, limit: 0.1 },
  angleZ: { hz: 2.2, damping: 0.28, limit: 0.16 },
  // The torso is heavier: slower, and it settles sooner.
  body: { hz: 1.8, damping: 0.34, limit: 0.12 },
}

/** How far each axis may go once the overshoot is added; the motion envelope's limits. */
export type FollowThroughBounds = Readonly<Record<FollowThroughKey, number>>

const MAX_STEP_SECONDS = 1 / 240

export class FollowThroughController {
  private readonly offset: Record<FollowThroughKey, number> = { angleX: 0, angleY: 0, angleZ: 0, body: 0 }
  private readonly velocity: Record<FollowThroughKey, number> = { angleX: 0, angleY: 0, angleZ: 0, body: 0 }

  /**
   * Writes `primary` plus the current overshoot into `output`. `acceleration`
   * is the primary pose's own, in driver units per second squared.
   */
  step(
    primary: Readonly<Anime25DDriver>,
    acceleration: (key: FollowThroughKey) => number,
    elapsedSeconds: number,
    bounds: FollowThroughBounds,
    output: Anime25DDriver,
  ): void {
    Object.assign(output, primary)
    const elapsed = Number.isFinite(elapsedSeconds) ? Math.max(0, elapsedSeconds) : 0
    for (const key of FOLLOW_THROUGH_KEYS) {
      if (!primary.phys) {
        this.offset[key] = 0
        this.velocity[key] = 0
        continue
      }
      const channel = FOLLOW_THROUGH[key]
      const omega = 2 * Math.PI * channel.hz
      const drive = finite(acceleration(key))
      const steps = Math.max(1, Math.ceil(elapsed / MAX_STEP_SECONDS))
      const dt = elapsed / steps
      let offset = this.offset[key]
      let velocity = this.velocity[key]
      for (let step = 0; step < steps; step += 1) {
        // The shown pose is a spring hung on the primary one: it lags a start
        // and overshoots a stop by the primary's own acceleration.
        velocity += (-omega * omega * offset - 2 * channel.damping * omega * velocity - drive) * dt
        offset += velocity * dt
      }
      if (!Number.isFinite(offset) || !Number.isFinite(velocity)) {
        offset = 0
        velocity = 0
      }
      this.offset[key] = offset
      this.velocity[key] = velocity
      // Never pulls the primary pose itself in; only the overshoot is bounded.
      const bound = Math.max(Math.abs(primary[key]), finite(bounds[key]))
      const shown = primary[key] + channel.limit * Math.tanh(offset / channel.limit)
      output[key] = Math.max(-bound, Math.min(bound, shown))
    }
  }
}

function finite(value: number): number {
  return Number.isFinite(value) ? value : 0
}
