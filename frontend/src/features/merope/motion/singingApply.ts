import type { SingingPlaybackGap } from '../singing/singingHold'
import type { MotionSourceId } from './channels'

export interface SingingApplyInput {
  gap: SingingPlaybackGap
  holdExpired: boolean
  audioPaused: boolean
  mouthOwner: MotionSourceId
  headBodyOwner: MotionSourceId
}

export interface SingingApply {
  /** Drop the music lease and clear groove on this rig. */
  release: boolean
  /** Keep singing=true / signal so the player groove continues. */
  writeGroove: boolean
  /** Write visemes onto the shared mouth path. */
  writeMouth: boolean
  /** Rest the mouth without releasing groove. */
  restMouth: boolean
}

/**
 * Channel-aware singing writes. Speech may own the mouth while music
 * still drives head/body. Pause rests the mouth; a real stop releases.
 */
export function resolveSingingApply(input: SingingApplyInput): SingingApply {
  const mouthOurs = input.mouthOwner === 'music'
  const bodyOurs = input.headBodyOwner === 'music'
  if (input.gap === 'stop' || (input.gap === 'hold' && input.holdExpired)) {
    return {
      release: true,
      writeGroove: false,
      writeMouth: false,
      restMouth: mouthOurs,
    }
  }
  if (input.gap === 'hold' || input.audioPaused) {
    return {
      release: false,
      writeGroove: bodyOurs,
      writeMouth: false,
      restMouth: mouthOurs,
    }
  }
  return {
    release: false,
    writeGroove: bodyOurs,
    writeMouth: mouthOurs,
    restMouth: false,
  }
}
