import type { PerformanceCue } from '../../../services/agent/types'
import type { BehaviorResource } from '../motion/behaviorResources'
import type { MotionChannel } from '../motion/channels'
import type { Anime25DDriver } from './driver'
import type { PerformanceExpressionOffset } from './performanceExpression'
import { rigChannelsForResources } from '../motion/behaviorResources'
import { IDENTITY_DRIVER } from './driver'

export type CueIntent = PerformanceCue['intent']

export interface PerformanceCueDefinition {
  resources: readonly BehaviorResource[]
  sticker?: true
  driver: (amount: number) => Partial<Anime25DDriver>
  expression: (
    amount: number,
    poseAmount: number,
  ) => Partial<PerformanceExpressionOffset>
}

const FACE = ['face.expression'] as const
const FACE_GAZE_HEAD = ['face.expression', 'face.gaze', 'body.head'] as const
const FACE_TORSO = ['face.expression', 'body.head', 'body.torso'] as const
const FACE_TORSO_ARMS = [
  'face.expression',
  'body.head',
  'body.torso',
  'body.arm.left',
  'body.arm.right',
] as const
const FACE_GAZE_TORSO = [
  'face.expression',
  'face.gaze',
  'body.head',
  'body.torso',
] as const
const FACE_TORSO_ARMS_BUST = [...FACE_TORSO_ARMS, 'secondary.bust'] as const

export const PERFORMANCE_CUE_DEFINITIONS = {
  greet: {
    resources: FACE_TORSO_ARMS,
    driver: (poseAmount) => ({
      body: 0.22 * poseAmount * bodyParticipation(poseAmount),
      armY: 0.3 * poseAmount * bodyParticipation(poseAmount),
    }),
    expression: (amount, poseAmount) => ({
      angleZ: -0.11 * poseAmount,
      brow: 0.17 * amount,
    }),
  },
  respond: {
    resources: FACE_TORSO,
    driver: (poseAmount) => ({ body: 0.2 * poseAmount }),
    expression: (amount, poseAmount) => ({
      angleY: -0.12 * poseAmount,
      angleZ: -0.045 * poseAmount,
      brow: 0.15 * amount,
    }),
  },
  question: {
    resources: FACE_TORSO,
    driver: (poseAmount) => ({ body: 0.18 * poseAmount }),
    expression: (amount, poseAmount) => ({
      angleZ: 0.15 * poseAmount,
      brow: 0.26 * amount,
      eyeOpen: 0.05 * amount,
    }),
  },
  delight: {
    resources: FACE_TORSO_ARMS_BUST,
    driver: (poseAmount) => ({
      body: 0.16 * poseAmount * bodyParticipation(poseAmount),
      armY: 0.22 * poseAmount * bodyParticipation(poseAmount),
      armPos: 0.34 * poseAmount * bodyParticipation(poseAmount),
      bust: IDENTITY_DRIVER.bust + 0.26 * poseAmount,
    }),
    expression: (amount, poseAmount) => ({
      angleY: -0.12 * poseAmount,
      brow: 0.22 * amount,
      eyeOpen: -0.025 * amount,
      eyeSqueeze: 0.84 * amount,
      mouthForm: 0.18 * amount,
    }),
  },
  emphasize: {
    resources: FACE_TORSO,
    driver: (poseAmount) => ({
      body: 0.4 * poseAmount * bodyParticipation(poseAmount),
    }),
    expression: (amount, poseAmount) => ({
      angleY: 0.12 * poseAmount,
      brow: 0.2 * amount,
    }),
  },
  listen: {
    resources: FACE_TORSO,
    driver: (poseAmount) => ({ body: 0.12 * poseAmount }),
    expression: (amount, poseAmount) => ({
      angleY: 0.09 * poseAmount,
      brow: 0.12 * amount,
    }),
  },
  notify: {
    resources: FACE_TORSO,
    driver: (poseAmount) => ({ body: 0.32 * poseAmount }),
    expression: (amount, poseAmount) => ({
      angleZ: -0.11 * poseAmount,
      brow: 0.22 * amount,
      eyeOpen: 0.055 * amount,
    }),
  },
  think: {
    resources: FACE_GAZE_HEAD,
    driver: () => ({}),
    expression: (amount, poseAmount) => ({
      angleZ: -0.2 * poseAmount,
      brow: 0.24 * amount,
      browAngSym: -0.18 * amount,
      eyeOpen: -0.16 * amount,
      irisScale: -0.06 * amount,
      mouthForm: -0.16 * amount,
      eyeX: 0.5 * amount,
      eyeY: -0.36 * amount,
    }),
  },
  dizzy: {
    resources: FACE,
    sticker: true,
    driver: () => ({}),
    expression: () => ({ eyeDizzy: 1 }),
  },
  cry: {
    resources: FACE,
    sticker: true,
    driver: () => ({}),
    expression: (amount) => ({
      brow: 0.2 * amount,
      browAngSym: -0.3 * amount,
      eyeCry: 1,
      mouthForm: -0.12 * amount,
    }),
  },
  angry: {
    resources: FACE_TORSO,
    sticker: true,
    driver: (poseAmount) => ({ body: 0.22 * poseAmount }),
    expression: (amount) => ({ anger: amount }),
  },
  speechless: {
    resources: FACE_GAZE_TORSO,
    sticker: true,
    driver: (poseAmount) => ({ body: -0.18 * poseAmount }),
    expression: (amount) => ({ speechless: amount }),
  },
  maniac: {
    resources: FACE_GAZE_TORSO,
    sticker: true,
    driver: (poseAmount) => ({ body: 0.17 * poseAmount }),
    expression: (amount) => ({ maniac: amount }),
  },
  silly: {
    resources: FACE_TORSO,
    sticker: true,
    driver: (poseAmount) => ({ body: -0.15 * poseAmount }),
    expression: (amount) => ({ silly: amount }),
  },
  lovestruck: {
    resources: FACE_GAZE_TORSO,
    sticker: true,
    driver: (poseAmount) => ({ body: -0.14 * poseAmount }),
    expression: (amount) => ({ lovestruck: amount }),
  },
} satisfies Record<CueIntent, PerformanceCueDefinition>

/** Coarse channels are derived, never authored */
const CUE_CHANNELS = Object.fromEntries(
  Object.entries(PERFORMANCE_CUE_DEFINITIONS).map(([intent, definition]) => [
    intent,
    Object.freeze(rigChannelsForResources(definition.resources)),
  ]),
) as Record<CueIntent, readonly MotionChannel[]>

export function performanceCueDefinition(
  intent: CueIntent,
): PerformanceCueDefinition {
  return PERFORMANCE_CUE_DEFINITIONS[intent]
}

export function performanceCueChannels(
  intent: CueIntent,
): readonly MotionChannel[] {
  return CUE_CHANNELS[intent]
}

export function intentExpressionPatch(
  intent: CueIntent,
  intensity: number,
): Partial<PerformanceExpressionOffset> {
  const definition = performanceCueDefinition(intent)
  const amount = intentAmount(intensity)
  const poseAmount = intentPoseAmount(intensity)
  const driver = definition.driver(poseAmount)
  return {
    ...definition.expression(amount, poseAmount),
    ...(typeof driver.body === 'number' ? { body: driver.body } : {}),
    ...(typeof driver.armY === 'number' ? { armY: driver.armY } : {}),
    ...(typeof driver.armPos === 'number' ? { armPos: driver.armPos } : {}),
    ...(typeof driver.bust === 'number'
      ? { bust: driver.bust - IDENTITY_DRIVER.bust }
      : {}),
  }
}

export function cueIsSticker(intent: CueIntent): boolean {
  return performanceCueDefinition(intent).sticker === true
}

function intentAmount(intensity: number): number {
  return Math.max(0.2, Math.min(1.4, intensity))
}

export function intentPoseAmount(intensity: number): number {
  const normalized = (intentAmount(intensity) - 0.2) / 1.2
  return 0.72 + normalized * 0.68
}

/** Strong greetings/joy/emphasis recruit the body, not extra head pitch or face. */
function bodyParticipation(poseAmount: number): number {
  const t = Math.max(0, Math.min(1, (poseAmount - 0.9) / 0.5))
  return 1 + 0.5 * t * t * (3 - 2 * t)
}
