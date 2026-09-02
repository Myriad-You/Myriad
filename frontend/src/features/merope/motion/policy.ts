import type { MotionChannel, MotionSourceId } from './channels'

export interface MotionChannelPolicy {
  mouth: MotionSourceId
  expression: MotionSourceId
  gaze: MotionSourceId
  headBody: MotionSourceId
}

export const IDLE_MOTION_POLICY: MotionChannelPolicy = {
  mouth: 'idle',
  expression: 'idle',
  gaze: 'idle',
  headBody: 'idle',
}

/** Lease helper: who may *claim* ambient. Visual mix uses occupancy, not this gate. */
export function allowsAmbientMotion(owner: MotionSourceId): boolean {
  return owner === 'idle' || owner === 'ambient'
}

/**
 * Co-speech brows/eyes. Idle keeps current player tests; live speech claims
 * coSpeech so mood and music emphasis yield.
 */
export function allowsCoSpeechExpression(owner: MotionSourceId): boolean {
  return owner === 'idle' || owner === 'coSpeech'
}

/** Co-speech head nods yield to music, performance, and preview. */
export function allowsCoSpeechHead(owner: MotionSourceId): boolean {
  return owner === 'idle' || owner === 'coSpeech'
}

/** Local pointer wins gaze except in an isolated preview scope. */
export function allowsPointerGaze(owner: MotionSourceId): boolean {
  return owner !== 'preview'
}

export function policyFromOwners(
  owners: Record<MotionChannel, MotionSourceId>,
): MotionChannelPolicy {
  return {
    mouth: owners.mouth,
    expression: owners.expression,
    gaze: owners.gaze,
    headBody: owners.headBody,
  }
}
