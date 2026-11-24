import type { MeropeRigManifest } from '@myriad/figure/rig'
import type { Anime25DPartProfile } from '@myriad/figure/workbench'
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
 * Where the search a preview began stands (MeropeRigPreview.partSearch):
 * still looking, found with what the page lists, or gone (it failed, the
 * portrait changed, or the server restarted). The import itself never reads
 * this: the server puts what it found into the package.
 */
export type StagedPartSearch =
  | { state: 'looking' }
  | { state: 'found', partProfile: Anime25DPartProfile }
  | { state: 'gone' }

const PART_MOTIONS: readonly unknown[] = ['rigid', 'sway', 'dangle', 'flutter']

/** A found profile, when it has what the page lists of each part. */
function listedPartProfile(value: unknown): Anime25DPartProfile | null {
  if (typeof value !== 'object' || value === null) return null
  const { parts } = value as { parts?: unknown }
  if (!Array.isArray(parts)) return null
  const listed = parts.every((part: unknown) => {
    if (typeof part !== 'object' || part === null) return false
    const { name, anchor, motion, material } = part as Record<string, unknown>
    return typeof name === 'string' && Array.isArray(anchor) && PART_MOTIONS.includes(motion) && typeof material === 'string'
  })
  return listed ? value as Anime25DPartProfile : null
}

export async function stagedPartSearch(id: string): Promise<StagedPartSearch> {
  const data = await apiService.get<{ state?: unknown, partProfile?: unknown }>(`${PREFIX}/parts/staged/${encodeURIComponent(id)}`)
  if (data.state === 'looking') return { state: 'looking' }
  const partProfile = data.state === 'found' ? listedPartProfile(data.partProfile) : null
  return partProfile ? { state: 'found', partProfile } : { state: 'gone' }
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
