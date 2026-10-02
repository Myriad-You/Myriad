import type { SiteFace } from '../api'
import type { WardrobeItem } from '../persona/wardrobe'
import { useEffect, useState } from 'react'
import { getFullBodyFace } from '../api'
import { isFullBodyItem } from '../persona/wardrobe'

/** A full figure the stage can play: its package and the portrait it falls back to. */
export interface StagedFullBody {
  manifest: NonNullable<SiteFace['manifest']>
  portraitUrl: string | null
}

/**
 * Which of the wardrobe's full-body sets the stage plays instead of the
 * bust, if any. Only a set with a saved figure can be staged.
 */
export function useFullBodyStage(items: readonly WardrobeItem[]) {
  const sets = items.filter((item) => isFullBodyItem(item) && item.rigAssetId)
  const [chosenId, setChosenId] = useState<string | null>(null)
  const chosen = sets.find((item) => item.id === chosenId) ?? null
  const [figure, setFigure] = useState<{
    key: string
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
          staged: face.manifest
            ? { manifest: face.manifest, portraitUrl: face.portraitUrl }
            : null,
        })
      })
      .catch(() => {
        if (!cancelled) setFigure({ key, staged: null })
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
    /** The staged set's figure once loaded; null plays the bust. */
    staged: chosen && figure?.key === key ? figure.staged : null,
  }
}
