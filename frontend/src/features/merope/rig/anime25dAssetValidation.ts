import type { RasterLayer } from './anime25dImportTypes'
import { currentCopy } from '../../../i18n/localeCopy'
import { missingAnime25DRequiredCapabilities } from './anime25dCapabilities'

export function validateAnime25DCharacterLayers(
  layers: readonly RasterLayer[],
): void {
  const missing = missingAnime25DRequiredCapabilities(layers)
  if (missing.length > 0) {
    throw new Error(
      currentCopy().merope.anime25dContractMissing.replace(
        '{missing}',
        missing.join(', '),
      ),
    )
  }
}
