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
