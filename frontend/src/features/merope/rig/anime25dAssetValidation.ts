import type { Anime25DImportCopy } from './anime25dImportCopy'
import type { RasterLayer } from './anime25dImportTypes'
import { missingAnime25DRequiredCapabilities } from './anime25dCapabilities'

export function validateAnime25DCharacterLayers(
  layers: readonly RasterLayer[],
  copy: Anime25DImportCopy,
): void {
  const missing = missingAnime25DRequiredCapabilities(layers)
  if (missing.length > 0) {
    throw new Error(
      copy.anime25dContractMissing.replace('{missing}', missing.join(', ')),
    )
  }
}
