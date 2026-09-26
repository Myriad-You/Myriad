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

  /** The filtered pose's own acceleration, in driver units per second squared. */
  accelerationOf(key: PoseKey): number {
    return this.acceleration[key]
  }

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
      // Only jerk changes when a new goal arrives
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

export const MIN_RESPONSE_SCALE = 0.75
export const MAX_RESPONSE_SCALE = 1.35

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
  weight: number
  quality: Readonly<BehaviorQuality> | null
}

/** The response filter runs on the blended result, so it cannot take a manner per source. */
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
  // Sources never sum to the whole pose
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
