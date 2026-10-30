import type { Anime25DImportCopy } from './anime25dImportCopy'
import type { RasterLayer } from './anime25dImportTypes'
import { missingAnime25DRequiredCapabilities } from './anime25dCapabilities'
import { formatTemplate } from './formatTemplate'

export function validateAnime25DCharacterLayers(
  layers: readonly RasterLayer[],
  copy: Anime25DImportCopy,
): void {
  const missing = missingAnime25DRequiredCapabilities(layers)
  if (missing.length > 0) {
    throw new Error(
      formatTemplate(copy.anime25dContractMissing, {
        missing: missing.join(', '),
      }),
    )
  }
}
