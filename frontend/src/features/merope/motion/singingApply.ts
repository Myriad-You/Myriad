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
  release: boolean
  /** Keep singing=true / signal so the player groove continues. */
  writeGroove: boolean
  writeMouth: boolean
  /** Rest the mouth without releasing groove. */
  restMouth: boolean
}

export function resolveSingingApply(input: SingingApplyInput): SingingApply {
  const mouthOurs = input.mouthOwner === 'music'
  // Ownership controls visible contribution in the player's pose gate, not
  // the lifetime of its music oscillator. Keep evidence flowing during a
  // touch/performance takeover so the weighted groove can continue/recover.
  const keepGroove = input.headBodyOwner !== 'preview'
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
      writeGroove: keepGroove,
      writeMouth: false,
      restMouth: mouthOurs,
    }
  }
  return {
    release: false,
    writeGroove: keepGroove,
    writeMouth: mouthOurs,
    restMouth: false,
  }
}
