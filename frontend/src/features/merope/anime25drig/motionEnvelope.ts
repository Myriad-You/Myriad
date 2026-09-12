import type { MeropeRigManifest } from '../rig/types'
import type { Anime25DDriver } from './driver'
import type { Anime25DPlaybackLayer } from './types'

export interface Anime25DMotionEnvelopeAxis {
  startsAt: number
  limit: number
}

export interface Anime25DMotionEnvelopeProfile {
  highCollar: boolean
  armMotion: boolean
  pitch: Anime25DMotionEnvelopeAxis
  torso: Anime25DMotionEnvelopeAxis
  rigidArm: Anime25DMotionEnvelopeAxis
}

export interface Anime25DMotionEnvelopeResult {
  clippedEnergy: number
  transferredEnergy: number
}

export const ANIME25D_MOTION_ENVELOPE_PROBES = [
  { id: 'turn-left', driver: { angleX: -1 } },
  { id: 'turn-right', driver: { angleX: 1 } },
  { id: 'pitch-up', driver: { angleY: 1 } },
  { id: 'pitch-down', driver: { angleY: -1 } },
  {
    id: 'full-left',
    driver: {
      angleX: -1,
      angleY: 1,
      angleZ: -1,
      eyeX: -1,
      eyeY: 0.6,
      body: -1,
      armY: 1,
      armPos: -1,
    },
  },
  {
    id: 'full-right',
    driver: {
      angleX: 1,
      angleY: -1,
      angleZ: 1,
      eyeX: 1,
      eyeY: -0.6,
      body: 1,
      armY: 1,
      armPos: 1,
    },
  },
] as const satisfies readonly {
  id: string
  driver: Readonly<Partial<Anime25DDriver>>
}[]

export type Anime25DMotionEnvelopeProbeId =
  (typeof ANIME25D_MOTION_ENVELOPE_PROBES)[number]['id']

interface MotionEnvelopePlayback {
  layers: readonly Pick<Anime25DPlaybackLayer, 'role'>[]
}
type MotionEnvelopeManifest = Pick<MeropeRigManifest, 'outfitProfile'>

export function deriveAnime25DMotionEnvelopeProfile(
  playback: Readonly<MotionEnvelopePlayback>,
  manifest?: Readonly<MotionEnvelopeManifest>,
): Anime25DMotionEnvelopeProfile {
  const highCollar = playback.layers.some(
    (layer) => layer.role === 'collar-back' || layer.role === 'collar-front',
  )
  const armMotion = playback.layers.some((layer) => layer.role === 'handwear')
  const torsoLimit = clamp(
    finiteOr(manifest?.outfitProfile?.torsoTwistScale, 1),
    0.25,
    1,
  )
  const rigidArmLimit = armMotion
    ? clamp(
        finiteOr(manifest?.outfitProfile?.secondaryMotionScale, 1),
        0.2,
        1,
      )
    : 0
  return {
    highCollar,
    armMotion,
    pitch: axisEnvelope(highCollar ? 0.56 : 1, highCollar ? 0.8 : 1),
    torso: axisEnvelope(torsoLimit * 0.78, torsoLimit),
    rigidArm: axisEnvelope(rigidArmLimit * 0.78, rigidArmLimit),
  }
}

export function projectAnime25DMotionEnvelope(
  target: Anime25DDriver,
  profile: Readonly<Anime25DMotionEnvelopeProfile>,
  result: Anime25DMotionEnvelopeResult,
): Anime25DMotionEnvelopeResult {
  result.clippedEnergy = 0
  result.transferredEnergy = 0

  const originalPitch = finite(target.angleY)
  const safePitch = softLimitSigned(originalPitch, profile.pitch)
  const residual = originalPitch - safePitch
  target.angleY = safePitch
  transferPitchResidual(target, residual, profile.armMotion, result)

  if (profile.armMotion) {
    const originalArmY = finite(target.armY)
    const safeArmY = softLimitSigned(originalArmY, profile.rigidArm)
    target.armY = safeArmY
    transferArmResidual(target, originalArmY - safeArmY, true, result)

    const originalArmPos = finite(target.armPos)
    const safeArmPos = softLimitSigned(originalArmPos, profile.rigidArm)
    target.armPos = safeArmPos
    transferArmResidual(target, originalArmPos - safeArmPos, false, result)
  }

  const originalBody = finite(target.body)
  const safeBody = softLimitSigned(originalBody, profile.torso)
  target.body = safeBody
  transferTorsoResidual(target, originalBody - safeBody, result)
  return result
}

function transferPitchResidual(
  target: Anime25DDriver,
  residual: number,
  armMotion: boolean,
  result: Anime25DMotionEnvelopeResult,
): void {
  const amount = Math.abs(residual)
  if (amount <= 1e-6) return
  noteTransfer(result, amount)
  const side = preferredSide(target)
  target.angleX = addBounded(target.angleX, side * amount * 0.34)
  target.angleZ = addBounded(target.angleZ, side * amount * 0.2)
  target.eyeX = addBounded(target.eyeX, side * amount * 0.16)
  target.eyeY = addBounded(target.eyeY, Math.sign(residual) * amount * 0.12)
  target.body = addBounded(target.body, Math.sign(residual) * amount * 0.26)
  if (armMotion) {
    target.armY = addBounded(target.armY, amount * 0.24)
    target.armPos = addBounded(target.armPos, side * amount * 0.18)
  }
}

function transferArmResidual(
  target: Anime25DDriver,
  residual: number,
  vertical: boolean,
  result: Anime25DMotionEnvelopeResult,
): void {
  const amount = Math.abs(residual)
  if (amount <= 1e-6) return
  noteTransfer(result, amount)
  const side = preferredSide(target)
  const direction = Math.sign(residual) || 1
  target.angleX = addBounded(
    target.angleX,
    direction * amount * (vertical ? 0.18 : 0.3),
  )
  target.angleZ = addBounded(
    target.angleZ,
    side * amount * (vertical ? 0.28 : 0.16),
  )
  target.body = addBounded(target.body, direction * amount * 0.24)
  target.eyeX = addBounded(target.eyeX, side * amount * 0.08)
}

function transferTorsoResidual(
  target: Anime25DDriver,
  residual: number,
  result: Anime25DMotionEnvelopeResult,
): void {
  const amount = Math.abs(residual)
  if (amount <= 1e-6) return
  noteTransfer(result, amount)
  const direction = Math.sign(residual) || 1
  const side = preferredSide(target)
  target.angleX = addBounded(target.angleX, direction * amount * 0.42)
  target.angleZ = addBounded(target.angleZ, side * amount * 0.3)
  target.eyeX = addBounded(target.eyeX, side * amount * 0.14)
  target.eyeY = addBounded(target.eyeY, -direction * amount * 0.08)
}

function noteTransfer(
  result: Anime25DMotionEnvelopeResult,
  amount: number,
): void {
  result.clippedEnergy += amount
  result.transferredEnergy += amount
}

const SIDE_BLEND = 0.08

function preferredSide(target: Readonly<Anime25DDriver>): number {
  const hint = target.angleZ * 0.7 + target.angleX * 0.3
  return clamp(finite(hint) / SIDE_BLEND, -1, 1)
}

function softLimitSigned(
  value: number,
  envelope: Readonly<Anime25DMotionEnvelopeAxis>,
): number {
  const limit = clamp(finite(envelope.limit), 0, 1)
  if (limit <= 0) return 0
  const startsAt = clamp(finite(envelope.startsAt), 0, limit)
  const sign = value < 0 ? -1 : 1
  const amount = clamp(Math.abs(finite(value)), 0, 1)
  if (amount <= startsAt) return value
  if (limit >= 1 - 1e-6) return sign * amount

  const inputSpan = Math.max(1e-6, 1 - startsAt)
  const outputSpan = Math.max(1e-6, limit - startsAt)
  const progress = clamp((amount - startsAt) / inputSpan, 0, 1)
  const exponent = inputSpan / outputSpan
  const eased = 1 - (1 - progress) ** exponent
  return sign * (startsAt + outputSpan * eased)
}

function axisEnvelope(
  startsAt: number,
  limit: number,
): Anime25DMotionEnvelopeAxis {
  const safeLimit = clamp(finite(limit), 0, 1)
  return {
    startsAt: clamp(finite(startsAt), 0, safeLimit),
    limit: safeLimit,
  }
}

function addBounded(value: number, offset: number): number {
  return clamp(finite(value) + finite(offset), -1, 1)
}

function finite(value: number): number {
  return Number.isFinite(value) ? value : 0
}

function finiteOr(value: number | undefined, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
