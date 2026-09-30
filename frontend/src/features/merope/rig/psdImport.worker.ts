import type { Anime25DSourceReference } from './anime25dImportTypes'
import type { AuthoredExpressionReference } from './authoredExpression'
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
    expressions,
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
    const master = await alignSourceMaster(
      sourceMasterUrl,
      psd.width,
      psd.height,
      copy.psdPreviewFailed,
    )
    const expressionReferences = master
      ? await alignExpressions(expressions ?? [], psd.width, psd.height, master.size)
      : []
    const prepared = await prepareAnime25DRigPsd(
      psd,
      sourceMasterAssetId,
      copy,
      (stage) => reply({ stage }),
      sourceGenerationFingerprint,
      master?.reference,
      expressionReferences,
    )
    reply({ prepared })
  } catch (error) {
    reply({
      error: error instanceof Error ? error.message : copy.psdSpecInvalid,
    })
  }
}

interface ImageSize {
  width: number
  height: number
}

async function alignSourceMaster(
  url: string,
  width: number,
  height: number,
  failure: string,
): Promise<{ reference: Anime25DSourceReference; size: ImageSize } | undefined> {
  if (width !== height) return undefined
  const response = await fetch(url)
  if (!response.ok) throw new Error(failure)
  const bitmap = await createImageBitmap(await response.blob())
  try {
    const size = { width: bitmap.width, height: bitmap.height }
    const reference = drawIntoPsd(bitmap, size, width, height)
    if (!reference) throw new Error(failure)
    return { reference, size }
  } finally {
    bitmap.close()
  }
}

/**
 * Expression redraws come back at the model's output size, a few pixels off the
 * portrait's; each is stretched onto the portrait's frame before placement.
 */
async function alignExpressions(
  expressions: NonNullable<RigPsdImportRequest['expressions']>,
  width: number,
  height: number,
  master: ImageSize,
): Promise<AuthoredExpressionReference[]> {
  const aligned = await Promise.all(
    expressions.map(async ({ kind, url }) => {
      try {
        const response = await fetch(url)
        if (!response.ok) return null
        const bitmap = await createImageBitmap(await response.blob())
        try {
          const reference = drawIntoPsd(bitmap, master, width, height)
          return reference ? { kind, ...reference } : null
        } finally {
          bitmap.close()
        }
      } catch {
        // An unreadable redraw only loses its authored parts.
        return null
      }
    }),
  )
  return aligned.filter((reference) => reference !== null)
}

function drawIntoPsd(
  bitmap: ImageBitmap,
  frame: ImageSize,
  width: number,
  height: number,
): Anime25DSourceReference | null {
  const squareEdge = Math.max(frame.width, frame.height)
  const paddingX = Math.floor((squareEdge - frame.width) / 2)
  const paddingY = Math.floor((squareEdge - frame.height) / 2)
  const scaleX = width / squareEdge
  const scaleY = height / squareEdge
  const canvas = new OffscreenCanvas(width, height)
  const context = canvas.getContext('2d')
  if (!context) return null
  context.imageSmoothingEnabled = true
  context.imageSmoothingQuality = 'high'
  context.drawImage(
    bitmap,
    paddingX * scaleX,
    paddingY * scaleY,
    frame.width * scaleX,
    frame.height * scaleY,
  )
  return {
    width,
    height,
    data: context.getImageData(0, 0, width, height).data,
  }
}
