import type { Anime25DPlaybackDocument, PoseCorrection } from '@myriad/figure/anime25drig'
import type { PoseCorrectionV7 } from '@myriad/figure/workbench'
import { poseCorrectionsToV7 } from '@myriad/figure/workbench'

/**
 * Corrections as a package of one version keys their axes: a version 7
 * package's as PoseCorrectionV7, a version 8 package's as PoseCorrection. The
 * save endpoints check them against the axes of the package they go into.
 */
export type StoredPoseCorrections = PoseCorrectionV7[] | PoseCorrection[]

/**
 * A copy of the editor's corrections (in the axes the player and the
 * workbench read) keyed the way `playback`'s own version keys them, for
 * saving into it.
 */
export function storedPoseCorrections(
  playback: Pick<Anime25DPlaybackDocument, 'version'>,
  corrections: readonly PoseCorrection[],
): StoredPoseCorrections {
  const copy = structuredClone([...corrections])
  return playback.version === 7 ? poseCorrectionsToV7(copy) : copy
}
