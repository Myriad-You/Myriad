import type { Layer, Psd } from 'ag-psd'
import { initializeCanvas, readPsd } from 'ag-psd'

export const MAX_PSD_BYTES = 32 * 1024 * 1024
const MAX_EDGE = 2048
const MAX_LAYERS = 64
const MAX_PIXELS = 32 * 1024 * 1024

export function validateRigPsdHeader(buffer: ArrayBuffer): void {
  if (buffer.byteLength < 26 || buffer.byteLength > MAX_PSD_BYTES)
    throw new Error('psdSpecInvalid')
  const view = new DataView(buffer)
  const height = view.getUint32(14)
  const width = view.getUint32(18)
  if (
    view.getUint32(0) !== 0x38425053 ||
    view.getUint16(4) !== 1 ||
    width < 256 ||
    height < 256 ||
    width > MAX_EDGE ||
    height > MAX_EDGE ||
    view.getUint16(22) !== 8 ||
    view.getUint16(24) !== 3
  ) {
    throw new Error('psdSpecInvalid')
  }
}

export function validateRigPsdStructure(psd: Psd): void {
  let count = 0
  let pixels = 0
  let visible = 0
  function visit(layers: Layer[], depth: number, parentVisible: boolean): void {
    if (depth > 32) throw new Error('psdLayerCountInvalid')
    for (const layer of layers) {
      if (++count > 256) throw new Error('psdLayerCountInvalid')
      const shown = parentVisible && !layer.hidden && layer.opacity !== 0
      const width = Math.max(0, (layer.right ?? 0) - (layer.left ?? 0))
      const height = Math.max(0, (layer.bottom ?? 0) - (layer.top ?? 0))
      if (
        !Number.isSafeInteger(width) ||
        !Number.isSafeInteger(height) ||
        width > MAX_EDGE ||
        height > MAX_EDGE
      ) {
        throw new Error('psdSpecInvalid')
      }
      pixels += width * height
      if (pixels > MAX_PIXELS) throw new Error('psdTooLarge')
      if (layer.children) visit(layer.children, depth + 1, shown)
      else if (shown && width && height) visible += 1
    }
  }
  visit(psd.children ?? [], 0, true)
  if (!visible || visible > MAX_LAYERS) throw new Error('psdLayerCountInvalid')
}

export function decodeRigPsd(buffer: ArrayBuffer): Psd {
  validateRigPsdHeader(buffer)
  const options = {
    useImageData: true,
    skipCompositeImageData: true,
    skipThumbnail: true,
    skipLinkedFilesData: true,
    totalMemoryLimit: 128 * 1024 * 1024,
  }
  const metadata = readPsd(buffer, { ...options, skipLayerImageData: true })
  validateRigPsdStructure(metadata)
  initializeCanvas(
    () => {
      throw new Error('Canvas decoding is not allowed in the PSD worker')
    },
    (width, height) => ({
      width,
      height,
      data: new Uint8ClampedArray(width * height * 4),
      colorSpace: 'srgb',
    }),
  )
  return readPsd(buffer, options)
}
