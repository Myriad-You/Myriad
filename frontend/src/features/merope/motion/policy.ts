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

export function allowsAmbientMotion(owner: MotionSourceId): boolean {
  return owner === 'idle' || owner === 'ambient'
}

export function allowsCoSpeechExpression(owner: MotionSourceId): boolean {
  return owner === 'idle' || owner === 'coSpeech'
}

export function allowsCoSpeechHead(owner: MotionSourceId): boolean {
  return owner === 'idle' || owner === 'coSpeech'
}

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
