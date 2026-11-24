import type { MeropeRigManifest } from '@myriad/figure/rig'
import type { RigMode } from './useRigImport'
import { useEffect, useRef } from 'react'

/**
 * The mode the active rig was made in, for the portrait shown: enhanced when
 * generated keys (the head's, a figure's body's), expressions or gesture arms went into it, plain otherwise. null when
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
  if (enhancement) return enhancement.turn || enhancement.body || enhancement.expressions || enhancement.arms ? 'enhanced' : 'plain'
  // Imported before the enhancement was recorded: turn keys alone tell.
  return playback.turnKeyforms ? 'enhanced' : 'plain'
}

/**
 * Opens the rig panel on the mode the active rig was made in: when the panel
 * appears, and when that mode becomes known or changes. Not while a split
 * runs. Its progress shows in the enhanced mode only, and the tags that
 * switch modes are off until it ends: a panel opened on a plain rig while a
 * split runs (one picked up after a reload, or begun before the panel was
 * left) would hide it behind the plain mode with no way to it. The end of a
 * split does not open the panel anew: the mode stays where the split was.
 */
export function useOpeningRigMode(
  activeMode: RigMode | null,
  splitting: boolean,
  setMode: (mode: RigMode) => void,
): void {
  const splittingRef = useRef(splitting)
  splittingRef.current = splitting
  useEffect(() => {
    if (activeMode && !splittingRef.current) setMode(activeMode)
  }, [activeMode, setMode])
}
