import type { MeropeRigManifest } from '@myriad/figure/rig'
import type { RigMode } from './useRigImport'

/**
 * The mode the active rig was made in, for the portrait shown: enhanced when
 * generated keys (the head's, a figure's body's) or expressions went into it, plain otherwise. null when
 * there is no rig for this portrait yet (its rig is of another portrait, or none).
 */
export function activeRigMode(
  manifest: Pick<MeropeRigManifest, 'sourceMasterAssetId' | 'anime25dPlayback'> | null | undefined,
  portrait: string | null | undefined,
): RigMode | null {
  if (!manifest || !portrait || manifest.sourceMasterAssetId !== portrait) return null
  const playback = manifest.anime25dPlayback
  if (!playback || typeof playback !== 'object') return null
  const enhancement = playback.enhancement
  if (enhancement) return enhancement.turn || enhancement.body || enhancement.expressions ? 'enhanced' : 'plain'
  // Imported before the enhancement was recorded: turn keys alone tell.
  return playback.turnKeyforms ? 'enhanced' : 'plain'
}
