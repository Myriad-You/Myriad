import type { PerformanceCue } from '../../../services/agent/types'
import type { BehaviorResource } from '../motion/behaviorResources'
import type { MotionChannel } from '../motion/channels'
import type { Anime25DDriver } from './driver'
import type { PerformanceExpressionOffset } from './performanceExpression'
import { rigChannelsForResources } from '../motion/behaviorResources'
import { IDENTITY_DRIVER } from './driver'

export type CueIntent = PerformanceCue['intent']

export interface PerformanceCueDefinition {
  /** Renderer-neutral body resources this cue's pose writes. Authored here. */
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

/**
 * One factual definition for each semantic cue. Rendering patches, stylized
 * classification and lease occupancy are all derived from this registry.
 */
export const PERFORMANCE_CUE_DEFINITIONS = {
  greet: {
    resources: FACE_TORSO_ARMS,
    driver: (poseAmount) => ({
      body: 0.22 * poseAmount,
      armY: 0.3 * poseAmount,
    }),
    expression: (amount, poseAmount) => ({
      angleZ: -0.11 * poseAmount,
      brow: 0.17 * amount,
    }),
  },
  respond: {
    resources: FACE_TORSO,
    // Acknowledgement is the deterministic default, so it must remain
    // readable even when semantic refinement is unavailable: head leads and
    // the torso follows at lower amplitude.
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
      body: 0.16 * poseAmount,
      armY: 0.22 * poseAmount,
      armPos: 0.34 * poseAmount,
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
    driver: (poseAmount) => ({ body: 0.4 * poseAmount }),
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
      brow: 0.14 * amount,
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

/**
 * Coarse channels are derived, never authored: two hand-kept lists drift, and
 * only `resources` describes what the pose actually writes.
 */
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

/** Exclusive channels this cue can write, projected from its resources. */
export function performanceCueChannels(
  intent: CueIntent,
): readonly MotionChannel[] {
  return CUE_CHANNELS[intent]
}

/**
 * The pose a cue form writes at a given amplitude.
 *
 * A realized behavior carries a form and an amplitude; `PerformanceCue` is the
 * director's wire shape, and its remaining fields (`atMs`, the three envelope
 * durations, `interrupt`) are scheduling, already resolved by the time a body
 * asks for a pose. Taking the two that matter keeps callers from rebuilding a
 * cue just to ask what a form looks like.
 */
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

/**
 * Body motion has a perceptual floor while preserving the director's dynamic
 * range. A selected action must still read at low semantic intensity; the
 * semantic amount itself continues to scale the face without this lift.
 */
export function intentPoseAmount(intensity: number): number {
  const normalized = (intentAmount(intensity) - 0.2) / 1.2
  return 0.72 + normalized * 0.68
}
