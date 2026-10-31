import type { MeropeRigManifest } from '@myriad/figure/rig'
import type { MovingPartsSearch } from './MovingPartsPanel'
import { isAnime25DPlayback } from '@myriad/figure/anime25drig'
import { currentCopy } from '../../../i18n/localeCopy'
import { apiService } from '../../../services/api'
import { fullBodyPath, meropeError, PREFIX, RIG_MUTATION_TIMEOUT_MS } from '../api'
import { composeRestReference } from '../assets/restReference'

/**
 * Looks for a package's moving parts again, in the background (the worn bust,
 * or a full-body set): for a package made before they were looked for, or
 * when that failed. The new package takes over when it is done.
 */
export async function findMovingParts(assetId: string, analysisReference: Blob, outfitId?: string): Promise<void> {
  const body = new FormData()
  body.append('assetId', assetId)
  body.append('analysisReference', analysisReference, 'rig-analysis.png')
  try {
    await apiService.post(`${PREFIX}${outfitId ? fullBodyPath(outfitId) : ''}/parts`, body, { timeout: RIG_MUTATION_TIMEOUT_MS })
  } catch (reason) {
    throw meropeError(reason, currentCopy().merope.rigPartsRefreshFailed)
  }
}

/**
 * Whether a search for a package's moving parts is running on the server (the
 * worn bust's, or a full-body set's): one started here before a reload, or
 * the one an import starts by itself.
 */
export async function movingPartsSearching(outfitId?: string): Promise<boolean> {
  const { searches } = await apiService.get<{ searches: Array<{ outfitId: string | null }> }>(`${PREFIX}/parts`)
  return searches.some((search) => (search.outfitId ?? undefined) === outfitId)
}

/**
 * The search for the figure on stage (the worn bust's package, or a staged
 * full-body set's `outfitId`): started from its package drawn at rest,
 * watched while it runs, and `found` taking up the package it saved.
 */
export function stagedMovingPartsSearch(
  manifest: MeropeRigManifest | null | undefined,
  assetId: string | null | undefined,
  outfitId: string | undefined,
  found: () => Promise<void>,
): MovingPartsSearch {
  return {
    find: async () => {
      const playback = manifest?.anime25dPlayback
      const atlasUrl = manifest?.textures[0]?.url
      if (!assetId || !playback || !isAnime25DPlayback(playback) || !atlasUrl) throw new Error(currentCopy().merope.rigPartsRefreshFailed)
      await findMovingParts(assetId, await composeRestReference(playback, atlasUrl), outfitId)
    },
    searching: () => movingPartsSearching(outfitId),
    found,
  }
}
