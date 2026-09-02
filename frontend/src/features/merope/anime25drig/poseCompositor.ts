import type { PoseChannelWeights, PoseKey } from './poseArbitration'
import { mixBoundedExpressionChannel } from './performanceExpression'
import { POSE_KEYS, poseChannelWeight } from './poseArbitration'

export type OccupancyOffset = Record<PoseKey, number>

export interface OccupancyLayer {
  offset: Readonly<Partial<OccupancyOffset>>
  /** Per-channel weight from `resolvePoseGate`, not a single scalar. */
  weights: Readonly<PoseChannelWeights>
}

export function zeroOccupancyOffset(): OccupancyOffset {
  const offset = {} as OccupancyOffset
  for (const key of POSE_KEYS) offset[key] = 0
  return offset
}

/**
 * Rest plus channel-weighted offsets.
 *
 * Every source that contends for the pose contributes one layer here instead
 * of mutating the driver in turn, so what the character does is decided by the
 * lease table and the situation rather than by the order the player happens to
 * call things in. High occupancy compresses toward the bound; it never drops a
 * layer outright.
 */
export function composeOccupancyOffsets(
  layers: readonly OccupancyLayer[],
  output: OccupancyOffset = zeroOccupancyOffset(),
): OccupancyOffset {
  for (const key of POSE_KEYS) output[key] = 0
  for (const layer of layers) {
    accumulateOccupancyOffset(output, layer.offset, layer.weights)
  }
  return output
}

/** Adds one already-sampled layer into a reusable composition buffer. */
export function accumulateOccupancyOffset(
  output: OccupancyOffset,
  offset: Readonly<Partial<OccupancyOffset>>,
  weights: Readonly<PoseChannelWeights>,
): void {
  for (const key of POSE_KEYS) {
    const amount = clamp01(poseChannelWeight(weights, key))
    if (amount <= 0) continue
    const value = offset[key]
    if (!value) continue
    output[key] = mixBoundedExpressionChannel(
      output[key],
      value * amount,
      -1,
      1,
      0,
    )
  }
}

/** Adds one channel without constructing a temporary one-field layer. */
export function accumulatePoseChannel(
  output: OccupancyOffset,
  key: PoseKey,
  value: number,
  weights: Readonly<PoseChannelWeights>,
): void {
  const amount = clamp01(poseChannelWeight(weights, key))
  if (amount <= 0 || !value) return
  output[key] = mixBoundedExpressionChannel(
    output[key],
    value * amount,
    -1,
    1,
    0,
  )
}

function clamp01(value: number): number {
  if (!Number.isFinite(value)) return 0
  return Math.max(0, Math.min(1, value))
}
