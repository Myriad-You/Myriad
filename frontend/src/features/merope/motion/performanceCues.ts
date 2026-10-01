import type { PerformanceCue } from '../../../services/agent/types'
import type { BehaviorResource } from './behaviorResources'
import type { MotionChannel } from './channels'
import { rigChannelsForResources } from './behaviorResources'

export type CueIntent = PerformanceCue['intent']

/**
 * What a cue occupies, and whether it is a sticker face: all that planning
 * needs of it. How the cue moves her is the runtime's
 * (`anime25drig/performanceCueDefinitions`).
 */
export interface PerformanceCueFootprint {
  resources: readonly BehaviorResource[]
  sticker?: true
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

export const PERFORMANCE_CUE_FOOTPRINTS = {
  greet: { resources: FACE_TORSO_ARMS },
  respond: { resources: FACE_TORSO },
  question: { resources: FACE_TORSO },
  delight: { resources: FACE_TORSO_ARMS_BUST },
  emphasize: { resources: FACE_TORSO },
  listen: { resources: FACE_TORSO },
  notify: { resources: FACE_TORSO },
  think: { resources: FACE_GAZE_HEAD },
  dizzy: { resources: FACE, sticker: true },
  cry: { resources: FACE, sticker: true },
  angry: { resources: FACE_TORSO, sticker: true },
  speechless: { resources: FACE_GAZE_TORSO, sticker: true },
  maniac: { resources: FACE_GAZE_TORSO, sticker: true },
  silly: { resources: FACE_TORSO, sticker: true },
  lovestruck: { resources: FACE_GAZE_TORSO, sticker: true },
} satisfies Record<CueIntent, PerformanceCueFootprint>

/** Coarse channels are derived, never authored */
const CUE_CHANNELS = Object.fromEntries(
  Object.entries(PERFORMANCE_CUE_FOOTPRINTS).map(([intent, footprint]) => [
    intent,
    Object.freeze(rigChannelsForResources(footprint.resources)),
  ]),
) as Record<CueIntent, readonly MotionChannel[]>

export function performanceCueFootprint(intent: CueIntent): PerformanceCueFootprint {
  return PERFORMANCE_CUE_FOOTPRINTS[intent]
}

export function performanceCueChannels(
  intent: CueIntent,
): readonly MotionChannel[] {
  return CUE_CHANNELS[intent]
}

export function cueIsSticker(intent: CueIntent): boolean {
  return performanceCueFootprint(intent).sticker === true
}
