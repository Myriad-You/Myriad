import type { PerformanceBaseline } from '../../../services/agent/types'
import type { PerformanceExpressionOffset, PerformanceExpressionTarget } from './performanceExpressionShared'
import { clamp, finiteOrZero, mixEyeOpen, OFFSET_KEYS } from './performanceExpressionShared'

const EXTREME_SOFT_LIMIT_START = 0.8

export function applyPerformanceExpressionOffset(
  target: PerformanceExpressionTarget,
  offset: Readonly<PerformanceExpressionOffset>,
  gaze = 1,
): void {
  target.brow = mixBoundedExpressionChannel(target.brow, offset.brow, -1, 1, 0)
  target.eyeX = mixBoundedExpressionChannel(
    target.eyeX,
    offset.eyeX * gaze,
    -1,
    1,
    0,
  )
  target.eyeY = mixBoundedExpressionChannel(
    target.eyeY,
    offset.eyeY * gaze,
    -1,
    1,
    0,
  )
  target.angleY = mixBoundedExpressionChannel(
    target.angleY,
    offset.angleY,
    -1,
    1,
    0,
  )
  target.angleZ = mixBoundedExpressionChannel(
    target.angleZ,
    offset.angleZ,
    -1,
    1,
    0,
  )
  if (typeof target.body === 'number') {
    target.body = mixBoundedExpressionChannel(
      target.body,
      offset.body,
      -1,
      1,
      0,
    )
  }
  if (typeof target.armY === 'number') {
    target.armY = mixBoundedExpressionChannel(
      target.armY,
      offset.armY,
      -1,
      1,
      0,
    )
  }
  if (typeof target.armPos === 'number') {
    target.armPos = mixBoundedExpressionChannel(
      target.armPos,
      offset.armPos,
      -1,
      1,
      0,
    )
  }
  applyPerformanceExpressionExtras(target, offset)
}

export function applyPerformanceExpressionExtras(
  target: PerformanceExpressionTarget,
  offset: Readonly<PerformanceExpressionOffset>,
  amount = 1,
): void {
  const weight = clamp(amount, 0, 1)
  target.browAngSym = mixBoundedExpressionChannel(
    target.browAngSym,
    offset.browAngSym * weight,
    -1,
    1,
    0,
  )
  target.eyeOpenL = mixEyeOpen(target.eyeOpenL, offset.eyeOpen * weight)
  target.eyeOpenR = mixEyeOpen(target.eyeOpenR, offset.eyeOpen * weight)
  target.eyeDizzy = mixBoundedExpressionChannel(
    target.eyeDizzy,
    offset.eyeDizzy * weight,
    0,
    1,
    0,
  )
  target.eyeSqueeze = mixBoundedExpressionChannel(
    target.eyeSqueeze,
    offset.eyeSqueeze * weight,
    0,
    1,
    0,
  )
  target.eyeCry = mixBoundedExpressionChannel(
    target.eyeCry,
    offset.eyeCry * weight,
    0,
    1,
    0,
  )
  target.mouthForm = mixBoundedExpressionChannel(
    target.mouthForm,
    offset.mouthForm * weight,
    -1,
    1,
    0,
  )
  if (target.eyeSmile !== undefined) {
    target.eyeSmile = mixBoundedExpressionChannel(target.eyeSmile, (offset.eyeSmile ?? 0) * weight, 0, 1, 0)
  }
  if (target.eyeWide !== undefined) {
    target.eyeWide = mixBoundedExpressionChannel(target.eyeWide, (offset.eyeWide ?? 0) * weight, 0, 1, 0)
  }
  target.irisScale = mixBoundedExpressionChannel(
    target.irisScale,
    offset.irisScale * weight,
    0.5,
    1.3,
    1,
  )
  target.anger = mixBoundedExpressionChannel(
    target.anger ?? 0,
    (offset.anger ?? 0) * weight,
    0,
    1,
    0,
  )
  target.speechless = mixBoundedExpressionChannel(
    target.speechless ?? 0,
    (offset.speechless ?? 0) * weight,
    0,
    1,
    0,
  )
  target.maniac = mixBoundedExpressionChannel(
    target.maniac ?? 0,
    (offset.maniac ?? 0) * weight,
    0,
    1,
    0,
  )
  target.silly = mixBoundedExpressionChannel(
    target.silly ?? 0,
    (offset.silly ?? 0) * weight,
    0,
    1,
    0,
  )
  target.lovestruck = mixBoundedExpressionChannel(
    target.lovestruck ?? 0,
    (offset.lovestruck ?? 0) * weight,
    0,
    1,
    0,
  )
}

/** Preserves expression headroom instead of flattening other sources at limits. */
export function mixBoundedExpressionChannel(
  base: number,
  offset: number,
  minimum: number,
  maximum: number,
  neutral: number,
): number {
  const boundedBase = clamp(finiteOrZero(base), minimum, maximum)
  const boundedOffset = finiteOrZero(offset)
  if (boundedOffset === 0) return boundedBase
  const direction = boundedOffset > 0 ? 1 : -1
  const boundary = direction > 0 ? maximum : minimum
  const outwardSpan = Math.abs(boundary - neutral)
  if (outwardSpan <= 0) return boundedBase

  let result = boundedBase
  let remaining = Math.abs(boundedOffset)
  const distanceToNeutral = direction * (neutral - result)
  if (distanceToNeutral > 0) {
    const inward = Math.min(remaining, distanceToNeutral)
    result += direction * inward
    remaining -= inward
  }
  if (remaining <= 0) return clamp(result, minimum, maximum)

  const softStart = neutral + direction * outwardSpan * EXTREME_SOFT_LIMIT_START
  const distanceToSoftStart = direction * (softStart - result)
  if (distanceToSoftStart > 0) {
    const linear = Math.min(remaining, distanceToSoftStart)
    result += direction * linear
    remaining -= linear
  }
  if (remaining <= 0) return clamp(result, minimum, maximum)

  const headroom = Math.max(0, direction * (boundary - result))
  if (headroom <= 0) return clamp(result, minimum, maximum)
  const compressed = (remaining * headroom) / (remaining + headroom)
  return clamp(result + direction * compressed, minimum, maximum)
}

export function writeBaselineOffset(
  output: PerformanceExpressionOffset,
  baseline: PerformanceBaseline,
): void {
  writeZero(output)
  const patches: Record<
    PerformanceBaseline['expression'],
    Partial<PerformanceExpressionOffset>
  > = {
    withdrawn: {
      brow: 0.18,
      browAngSym: -0.92,
      eyeOpen: -0.4,
      mouthForm: -0.82,
      irisScale: -0.075,
    },
    subdued: {
      brow: 0.12,
      browAngSym: -0.66,
      eyeOpen: -0.26,
      mouthForm: -0.55,
      irisScale: -0.04,
    },
    // Eye openness cannot exceed the driver's 1.0
    tense: {
      brow: -0.56,
      browAngSym: 0.82,
      eyeOpen: -0.08,
      mouthForm: -0.48,
      irisScale: -0.1,
    },
    steady: {},
    // Warmth shows in the eyes as much as the mouth.
    warm: { brow: 0.12, mouthForm: 0.15, irisScale: 0.015, eyeSmile: 0.3 },
  }
  Object.assign(output, patches[baseline.expression])
  const posture: Record<
    PerformanceBaseline['posture'],
    Partial<PerformanceExpressionOffset>
  > = {
    closed: { body: -0.16, armY: -0.14 },
    neutral: {},
    open: { body: 0.12, armY: 0.16 },
  }
  Object.assign(output, posture[baseline.posture])
  const poseEnergy = 0.72 + clamp(baseline.motionEnergy, 0.2, 1.4) * 0.36
  output.body *= poseEnergy
  output.armY *= poseEnergy
  output.armPos *= poseEnergy
}

function writeZero(output: PerformanceExpressionOffset): void {
  for (const key of OFFSET_KEYS) output[key] = 0
}
