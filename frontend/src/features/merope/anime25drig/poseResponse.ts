import type { BehaviorQuality } from '../motion/behavior'
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

  /**
   * `responseScale` is the director's manner, applied to the rig's bandwidth.
   * Stepping it between frames is safe: only jerk changes, exactly as it does
   * when a new goal arrives, so x, x' and x'' stay continuous across the
   * change and no smoothing of the scale itself is needed.
   */
  step(
    current: Pose,
    target: Readonly<Pose>,
    elapsedSeconds: number,
    responseScale = 1,
  ): void {
    const dt = Number.isFinite(elapsedSeconds) ? Math.max(0, elapsedSeconds) : 0
    if (dt === 0) return
    const scale = Number.isFinite(responseScale)
      ? clamp(responseScale, MIN_RESPONSE_SCALE, MAX_RESPONSE_SCALE)
      : 1
    for (const key of CONTINUOUS_POSE_KEYS) {
      const omega = 2 * Math.PI * RESPONSE_HZ[key] * scale
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

/**
 * The band a delivery's manner may move the rig's bandwidth through.
 *
 * Deliberately narrow. `RESPONSE_HZ` is what this body can do; quality is what
 * the director asked for, and a pose that outran the secondary physics would
 * read as a different rig rather than a different mood.
 */
export const MIN_RESPONSE_SCALE = 0.75
export const MAX_RESPONSE_SCALE = 1.35

/**
 * Manner of delivery as a multiplier on pose bandwidth.
 *
 * The planner already publishes eight quality dimensions per behavior and the
 * merge boundary scales them again by motion style, but until this the whole
 * vector stopped at amplitude and envelope pacing: `restrained` and `open`
 * reached the same pose at exactly the same speed, and a forceful `emphasize`
 * settled as gently as a `listen` nod. Four dimensions have a defensible
 * reading here and only those are used — extent, rebound, asymmetry and
 * density describe the shape of the pose, not how fast it is approached, and
 * are already spent where they belong.
 */
export function poseResponseScale(
  quality: Readonly<BehaviorQuality>,
): number {
  const tempo = clamp(finiteOr(quality.tempo, 1), 0.45, 1.7)
  const directness = clamp(finiteOr(quality.directness, 0.72), 0.2, 1.4)
  const fluidity = clamp(finiteOr(quality.fluidity, 0.8), 0.2, 1.4)
  const power = clamp(finiteOr(quality.power, 1), 0.2, 1.6)
  return clamp(
    1 +
      (tempo - 1) * 0.34 +
      (directness - 0.72) * 0.3 +
      (power - 1) * 0.16 -
      (fluidity - 0.8) * 0.28,
    MIN_RESPONSE_SCALE,
    MAX_RESPONSE_SCALE,
  )
}

export interface PoseResponseCandidate {
  /** How much of the pose this source is currently responsible for. */
  weight: number
  quality: Readonly<BehaviorQuality> | null
}

/**
 * One scale for the composed pose.
 *
 * The response filter runs on the blended result, so it cannot take a manner
 * per source. Scales are averaged by how much of the pose each source is
 * actually carrying — averaging the quality vectors themselves would invent a
 * delivery nobody authored.
 */
export function resolvePoseResponseScale(
  candidates: readonly PoseResponseCandidate[],
): number {
  let total = 0
  let weighted = 0
  for (const candidate of candidates) {
    if (!candidate.quality) continue
    const weight = clamp(finiteOr(candidate.weight, 0), 0, 1)
    if (weight <= 0) continue
    total += weight
    weighted += weight * poseResponseScale(candidate.quality)
  }
  if (total <= 0) return 1
  // Sources never sum to the whole pose; what they do not claim is idle
  // motion, which has no authored manner and stays at the rig's own rate.
  const share = Math.min(1, total)
  return 1 + (weighted / total - 1) * share
}

function clamp(value: number, minimum: number, maximum: number): number {
  if (!Number.isFinite(value)) return minimum
  return Math.max(minimum, Math.min(maximum, value))
}

function finiteOr(value: number | undefined, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback
}
