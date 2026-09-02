import type { Anime25DDriver } from './driver'

export const CONTINUOUS_POSE_KEYS = [
  'angleX',
  'angleY',
  'angleZ',
  'body',
  'armY',
  'armPos',
  'eyeX',
  'eyeY',
] as const
type PoseKey = (typeof CONTINUOUS_POSE_KEYS)[number]
type Pose = Pick<Anime25DDriver, PoseKey>

const RESPONSE_HZ: Readonly<Pose> = {
  angleX: 9.5,
  angleY: 9.5,
  angleZ: 9.5,
  body: 7,
  armY: 8,
  armPos: 8,
  eyeX: 16,
  eyeY: 16,
}

/**
 * The actual displayed pose is the initial condition of every following step.
 * Position alone is insufficient: resetting velocity/acceleration on replacement
 * creates a visible corner. Carry all three across ALL sources, including idle
 * and pointer. This replaces (does not follow) the old first-order filter.
 * Three coincident real poles yield an exact, non-oscillating response with C2
 * continuity, even for discontinuous goals. No queued transition or reset pose.
 * Matching source velocity is also the key transition constraint in Bollo 2018:
 * https://media.gdcvault.com/gdc2018/presentations/bollo_david_inertialization_high_performance.pdf
 */
export class PoseResponseController {
  private readonly velocity: Pose = {
    angleX: 0,
    angleY: 0,
    angleZ: 0,
    body: 0,
    armY: 0,
    armPos: 0,
    eyeX: 0,
    eyeY: 0,
  }

  private readonly acceleration: Pose = {
    angleX: 0,
    angleY: 0,
    angleZ: 0,
    body: 0,
    armY: 0,
    armPos: 0,
    eyeX: 0,
    eyeY: 0,
  }

  step(current: Pose, target: Readonly<Pose>, elapsedSeconds: number): void {
    const dt = Number.isFinite(elapsedSeconds) ? Math.max(0, elapsedSeconds) : 0
    if (dt === 0) return
    for (const key of CONTINUOUS_POSE_KEYS) {
      const omega = 2 * Math.PI * RESPONSE_HZ[key]
      const goal = Number.isFinite(target[key]) ? target[key] : current[key]
      // Exact solution of x''' = w³(goal-x) - 3w²x' - 3wx'' for this step.
      // Only jerk changes when a new goal arrives; x, x' and x'' are retained.
      const offset = current[key] - goal
      const c1 = this.velocity[key] + omega * offset
      const c2 =
        this.acceleration[key] +
        2 * omega * this.velocity[key] +
        omega * omega * offset
      const polynomial = offset + c1 * dt + (c2 * dt * dt) / 2
      const derivative = c1 + c2 * dt
      const decay = Math.exp(-omega * dt)
      current[key] = goal + polynomial * decay
      this.velocity[key] = (derivative - omega * polynomial) * decay
      this.acceleration[key] =
        (c2 - 2 * omega * derivative + omega * omega * polynomial) * decay
    }
  }
}

export function isContinuousPoseKey(key: keyof Anime25DDriver): key is PoseKey {
  return (CONTINUOUS_POSE_KEYS as readonly string[]).includes(key)
}
