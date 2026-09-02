import type { SpeechArticulation } from '../rig/articulation'
import type { RigMotionPort } from '../rig/motionPort'
import type { SingingSpectrumDrive } from '../singing/singingGroove'
import type { SingingApply } from './singingApply'
import { restSingingArticulation } from '../singing/singingClock'

export interface SingingRigWrite {
  trackId: string | null
  singing: boolean
  spectrum: SingingSpectrumDrive | null
  articulation: SpeechArticulation | null
  restMouth: boolean
  speechActive: boolean | null
}

/**
 * Apply channel-gated singing to one mounted rig. Speech-owned mouth is
 * left untouched so visemes and occupancy stay with the speech controller.
 */
export function applySingingWrite(
  rig: Pick<
    RigMotionPort,
    | 'setSinging'
    | 'setSingingTrack'
    | 'setSingingSpectrum'
    | 'setSpeechArticulation'
    | 'setSpeechActive'
  >,
  apply: SingingApply,
  drive: {
    trackId: string | null
    spectrum: SingingSpectrumDrive | null
    articulation: SpeechArticulation
  },
): SingingRigWrite {
  const write: SingingRigWrite = {
    trackId: drive.trackId,
    singing: apply.writeGroove,
    spectrum: apply.writeGroove ? drive.spectrum : null,
    articulation: apply.writeMouth ? drive.articulation : null,
    restMouth: apply.restMouth,
    speechActive: apply.writeMouth ? true : apply.restMouth ? false : null,
  }
  rig.setSingingTrack(drive.trackId)
  if (apply.writeGroove) {
    rig.setSinging(true)
    rig.setSingingSpectrum(drive.spectrum)
  } else {
    rig.setSinging(false)
    rig.setSingingSpectrum(null)
  }
  if (apply.writeMouth) {
    rig.setSpeechActive(true)
    rig.setSpeechArticulation(drive.articulation)
  } else if (apply.restMouth) {
    rig.setSpeechArticulation(restSingingArticulation())
    rig.setSpeechActive(false)
  }
  return write
}
