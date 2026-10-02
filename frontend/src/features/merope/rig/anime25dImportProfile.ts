import type { CharacterAssetProfile } from './contract'
import { CHARACTER_ASSET_PROFILES } from './contract'

/** What importing a decomposed PSD keeps, frames and pivots on, per asset profile. */
export interface Anime25DImportProfile {
  readonly profile: CharacterAssetProfile
  /** The stage the content is padded, never stretched, into. */
  readonly canvas: { readonly width: number; readonly height: number }
  /** Layers dropped before rigging. */
  readonly ignoredLayers: ReadonlySet<string>
  /** Layers rigged under another role's name. */
  readonly renamedLayers: Readonly<Record<string, string>>
  /** Whether lower-body clothing frames the stage; a bust's crop cuts it instead. */
  readonly lowerBodyFrames: boolean
  /**
   * Where the upper body leans from, and below which nothing leans: a bust's
   * crop edge, or a standing figure's hips.
   */
  readonly bodyPivot: 'crop' | 'hips'
}

const PROFILES: Record<CharacterAssetProfile, Anime25DImportProfile> = {
  bust: {
    profile: 'bust',
    canvas: CHARACTER_ASSET_PROFILES.bust.portrait.canvas,
    // Feet are always far below the crop.
    ignoredLayers: new Set(['footwear']),
    /**
     * A long garment's visible front is often labelled legwear (a kimono or
     * long skirt over the legs). Kept as lower-body clothing it paints where
     * it was drawn and the portrait crop cuts it; dropped, it bares the flat
     * fill a decomposer paints behind it.
     */
    renamedLayers: { legwear: 'bottomwear' },
    lowerBodyFrames: false,
    bodyPivot: 'crop',
  },
  fullBody: {
    profile: 'fullBody',
    canvas: CHARACTER_ASSET_PROFILES.fullBody.portrait.canvas,
    ignoredLayers: new Set(),
    renamedLayers: {},
    lowerBodyFrames: true,
    bodyPivot: 'hips',
  },
}

export function anime25DImportProfile(
  profile: CharacterAssetProfile,
): Anime25DImportProfile {
  return PROFILES[profile]
}
