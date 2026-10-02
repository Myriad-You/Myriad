import type { SiteFace } from '../api'
import { useCallback, useEffect, useState } from 'react'
import { getFullBodyFace } from '../api'

/** A full figure the stage can play: its package and the portrait it falls back to. */
export interface StagedFullBody {
  manifest: NonNullable<SiteFace['manifest']>
  portraitUrl: string | null
}

/**
 * The worn outfit's optional full figure and whether the stage plays it
 * instead of the bust. `outfitKey` changes with the bust the figure is drawn
 * from, which voids it.
 */
export function useFullBodyStage(outfitKey: string) {
  const [figure, setFigure] = useState<StagedFullBody | null>(null)
  const [staging, setStaging] = useState(false)

  const reload = useCallback(async () => {
    try {
      const face = await getFullBodyFace()
      setFigure(
        face.manifest
          ? { manifest: face.manifest, portraitUrl: face.portraitUrl }
          : null,
      )
    } catch {
      setFigure(null)
    }
  }, [])

  useEffect(() => {
    void reload()
  }, [outfitKey, reload])

  return {
    /** The playable full figure, if the worn outfit has one. */
    figure,
    /** The full figure while the stage plays it; otherwise the bust is staged. */
    staged: staging ? figure : null,
    staging: staging && figure !== null,
    setStaging,
    reload,
  }
}
