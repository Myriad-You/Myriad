import type { RigPsdImportReply, RigPsdImportRequest } from './psdImportClient'
import { prepareAnime25DRigPsd } from './anime25dImporter'
import { decodeRigPsd } from './psdDecode'

const reply = (data: RigPsdImportReply) => globalThis.postMessage(data)

globalThis.onmessage = async (event: MessageEvent<RigPsdImportRequest>) => {
  const {
    buffer,
    sourceMasterAssetId,
    sourceMasterUrl,
    sourceGenerationFingerprint,
    copy,
  } = event.data
  try {
    // The worker cannot read the page's saved locale.
    const psd = (() => {
      try {
        return decodeRigPsd(buffer)
      } catch (error) {
        const code = error instanceof Error ? error.message : ''
        throw new Error(
          code === 'psdTooLarge'
            ? copy.psdTooLarge
            : code === 'psdLayerCountInvalid'
              ? copy.psdLayerCountInvalid
              : copy.psdSpecInvalid,
        )
      }
    })()
    const sourceReference = await alignSourceMaster(
      sourceMasterUrl,
      psd.width,
      psd.height,
      copy.psdPreviewFailed,
    )
    const prepared = await prepareAnime25DRigPsd(
      psd,
      sourceMasterAssetId,
      copy,
      (stage) => reply({ stage }),
      sourceGenerationFingerprint,
      sourceReference,
    )
    reply({ prepared })
  } catch (error) {
    reply({
      error: error instanceof Error ? error.message : copy.psdSpecInvalid,
    })
  }
}

async function alignSourceMaster(
  url: string,
  width: number,
  height: number,
  failure: string,
) {
  if (width !== height) return undefined
  const response = await fetch(url)
  if (!response.ok) throw new Error(failure)
  const bitmap = await createImageBitmap(await response.blob())
  try {
    const squareEdge = Math.max(bitmap.width, bitmap.height)
    const paddingX = Math.floor((squareEdge - bitmap.width) / 2)
    const paddingY = Math.floor((squareEdge - bitmap.height) / 2)
    const scaleX = width / squareEdge
    const scaleY = height / squareEdge
    const canvas = new OffscreenCanvas(width, height)
    const context = canvas.getContext('2d')
    if (!context) throw new Error(failure)
    context.imageSmoothingEnabled = true
    context.imageSmoothingQuality = 'high'
    context.drawImage(
      bitmap,
      paddingX * scaleX,
      paddingY * scaleY,
      bitmap.width * scaleX,
      bitmap.height * scaleY,
    )
    return {
      width,
      height,
      data: context.getImageData(0, 0, width, height).data,
    }
  } finally {
    bitmap.close()
  }
}
