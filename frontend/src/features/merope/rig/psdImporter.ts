import type { PreparedAnime25DRigImport } from './anime25dImporter'
import type { AuthoredExpressionKind } from './authoredExpression'
import type { CharacterAssetProfile } from './contract'
import type { DetectedSkeleton } from './skeleton'
import { API_URL } from '../../../config'
import { currentCopy } from '../../../i18n/localeCopy'
import { anime25DImportCopy } from './anime25dImportCopy'
import { importRigPsdInWorker } from './psdImportClient'
import { placeTurnKeyforms, turnKeyformsFor } from './turnKeyformImport'

const MAX_PSD_BYTES = 32 * 1024 * 1024

export type PreparedRigPsdImport = PreparedAnime25DRigImport

export function sourceMasterFetchUrl(source: string, pageUrl: string, apiUrl = API_URL): string {
  return new URL(source, apiUrl ? new URL(apiUrl, pageUrl).href : pageUrl).href
}

export async function prepareRigPsdImport(
  file: File,
  sourceMasterAssetId: string,
  onStage?: (stage: 'validated' | 'packing') => void,
  sourceGenerationFingerprint?: string,
  signal?: AbortSignal,
  expressions: ReadonlyArray<{ kind: AuthoredExpressionKind; url: string }> = [],
  profile: CharacterAssetProfile = 'bust',
  skeleton?: DetectedSkeleton,
): Promise<PreparedRigPsdImport> {
  signal?.throwIfAborted()
  if (!sourceMasterAssetId) throw new Error(currentCopy().merope.psdNeedAsset)
  if (file.size <= 0 || file.size > MAX_PSD_BYTES) {
    throw new Error(currentCopy().merope.psdTooLarge)
  }
  const buffer = await file.arrayBuffer()
  signal?.throwIfAborted()
  const prepared = await importRigPsdInWorker(
    {
      buffer,
      sourceMasterAssetId,
      // Resolve against the page, not the worker chunk URL.
      sourceMasterUrl: sourceMasterFetchUrl(sourceMasterAssetId, document.baseURI),
      sourceGenerationFingerprint,
      expressions: expressions.map(({ kind, url }) => ({
        kind,
        url: sourceMasterFetchUrl(url, document.baseURI),
      })),
      copy: anime25DImportCopy(),
      profile,
      ...(skeleton ? { skeleton } : {}),
    },
    signal,
    onStage,
  )
  // Keys measured on this decomposition ride into the playback, cut as it is.
  const keys = turnKeyformsFor(file)
  const turnKeyforms = keys && placeTurnKeyforms(keys, prepared.documentFrame, prepared.documentFrame)
  if (turnKeyforms && prepared.source.anime25dPlayback) prepared.source.anime25dPlayback.turnKeyforms = turnKeyforms
  return prepared
}
