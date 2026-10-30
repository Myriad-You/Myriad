import type { SpeechArticulation } from '../rig/articulation'
import type { RigMotionPort } from '../rig/motionPort'
import type { MusicMotionSignal } from '../singing/musicSignal'
import type { SingingApply } from './singingApply'
import { restSingingArticulation } from '../singing/singingClock'

export interface SingingRigWrite {
  trackId: string | null
  singing: boolean
  signal: MusicMotionSignal | null
  articulation: SpeechArticulation | null
  restMouth: boolean
  speechActive: boolean | null
}

export function applySingingWrite(
  rig: Pick<
    RigMotionPort,
    | 'setSinging'
    | 'setSingingTrack'
    | 'setMusicSignal'
    | 'setSpeechArticulation'
    | 'setSpeechActive'
  >,
  apply: SingingApply,
  drive: {
    trackId: string | null
    signal: MusicMotionSignal | null
    articulation: SpeechArticulation
  },
): SingingRigWrite {
  const write: SingingRigWrite = {
    trackId: drive.trackId,
    singing: apply.writeGroove,
    signal: apply.writeGroove ? drive.signal : null,
    articulation: apply.writeMouth ? drive.articulation : null,
    restMouth: apply.restMouth,
    speechActive: apply.writeMouth || apply.restMouth ? false : null,
  }
  rig.setSingingTrack(drive.trackId)
  if (apply.writeGroove) {
    rig.setSinging(true)
    rig.setMusicSignal(drive.signal)
  } else {
    rig.setSinging(false)
    rig.setMusicSignal(null)
  }
  if (apply.writeMouth) {
    rig.setSpeechActive(false)
    rig.setSpeechArticulation(drive.articulation)
  } else if (apply.restMouth) {
    rig.setSpeechArticulation(restSingingArticulation())
    rig.setSpeechActive(false)
  }
  return write
}
