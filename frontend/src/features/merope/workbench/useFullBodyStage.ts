import type { SiteFace } from '../api'
import type { WardrobeItem } from '../persona/wardrobe'
import { useEffect, useState } from 'react'
import { getFullBodyFace } from '../api'
import { isFullBodyItem } from '../persona/wardrobe'

/** A full figure the stage can play: its package and the portrait it falls back to. */
export interface StagedFullBody {
  manifest: NonNullable<SiteFace['manifest']>
  portraitUrl: string | null
  /** The package this figure is, for edits that rewrite it. */
  assetId: string | null
}

/**
 * Which of the wardrobe's full-body sets the stage plays instead of the
 * bust, if any. Only a set with a saved figure can be staged; the bust stays
 * the default whichever full body is worn.
 */
export function useFullBodyStage(
  items: readonly WardrobeItem[],
  wornId: string | null,
) {
  // The worn set comes first when the stage is cycled.
  const sets = items
    .filter((item) => isFullBodyItem(item) && item.rigAssetId)
    .sort((a, b) => Number(b.id === wornId) - Number(a.id === wornId))
  const [chosenId, setChosenId] = useState<string | null>(null)
  const chosen = sets.find((item) => item.id === chosenId) ?? null
  const [figure, setFigure] = useState<{
    key: string
    id: string
    staged: StagedFullBody | null
  } | null>(null)
  // A new picture or a new figure for the chosen set is loaded afresh.
  const key = chosen ? `${chosen.id}\n${chosen.rigAssetId}` : ''

  const loadId = chosen?.id ?? null
  useEffect(() => {
    if (!loadId) return
    let cancelled = false
    getFullBodyFace(loadId)
      .then((face) => {
        if (cancelled) return
        setFigure({
          key,
          id: loadId,
          staged: face.manifest
            ? { manifest: face.manifest, portraitUrl: face.portraitUrl, assetId: face.assetId }
            : null,
        })
      })
      .catch(() => {
        if (!cancelled) setFigure({ key, id: loadId, staged: null })
      })
    return () => {
      cancelled = true
    }
  }, [key, loadId])

  return {
    /** The full-body sets with a figure to stage. */
    sets,
    /** The staged set's id; null stages the bust. */
    stagedId: chosen?.id ?? null,
    stage: setChosenId,
    /**
     * The staged set's figure once loaded; null plays the bust. A set's new
     * figure replaces its old one only once it has loaded.
     */
    staged: chosen && figure?.id === chosen.id ? figure.staged : null,
  }
}
