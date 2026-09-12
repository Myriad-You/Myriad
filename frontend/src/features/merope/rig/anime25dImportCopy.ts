import { currentCopy } from '../../../i18n/localeCopy'

export function anime25DImportCopy() {
  const copy = currentCopy().merope
  return {
    anime25dMissingFace: copy.anime25dMissingFace,
    anime25dMissingLayer: copy.anime25dMissingLayer,
    anime25dContractMissing: copy.anime25dContractMissing,
    anime25dPartCount: copy.anime25dPartCount,
    anime25dBoneLimit: copy.anime25dBoneLimit,
    anime25dLayerTooWide: copy.anime25dLayerTooWide,
    anime25dAtlasOverflow: copy.anime25dAtlasOverflow,
    rigAtlasFailed: copy.rigAtlasFailed,
    canvasUnsupported: copy.canvasUnsupported,
    psdPreviewFailed: copy.psdPreviewFailed,
    psdTooLarge: copy.psdTooLarge,
    psdLayerCountInvalid: copy.psdLayerCountInvalid,
    psdSpecInvalid: copy.psdSpecInvalid,
  }
}

export type Anime25DImportCopy = ReturnType<typeof anime25DImportCopy>
