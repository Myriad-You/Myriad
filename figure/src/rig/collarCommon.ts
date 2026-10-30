import type { RasterLayer } from './anime25dImportTypes'

export const ALPHA_COMPONENT_THRESHOLD = 16
export const HIGH_COLLAR_UPPER_FRACTION = 0.35
export const COLLAR_REFERENCE_REAR = 1
export const COLLAR_REFERENCE_FRONT = 2

export interface CollarColorMask {
  left: number
  top: number
  width: number
  height: number
  data: Uint8Array
}

export type CollarReferenceMask = CollarColorMask

export function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

export function copyRasterPixel(
  source: Uint8ClampedArray,
  target: Uint8ClampedArray,
  pixel: number,
  alpha: number,
): void {
  target[pixel] = source[pixel]
  target[pixel + 1] = source[pixel + 1]
  target[pixel + 2] = source[pixel + 2]
  target[pixel + 3] = Math.round(alpha)
}

export function cropRasterPixels(
  source: Uint8ClampedArray,
  sourceWidth: number,
  left: number,
  top: number,
  width: number,
  height: number,
): Uint8ClampedArray {
  const output = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    const sourceStart = ((top + y) * sourceWidth + left) * 4
    const sourceEnd = sourceStart + width * 4
    output.set(source.subarray(sourceStart, sourceEnd), y * width * 4)
  }
  return output
}

export function perceptualColorDistance(
  left: Uint8ClampedArray,
  leftPixel: number,
  right: Uint8ClampedArray,
  rightPixel: number,
): number {
  const red = left[leftPixel] - right[rightPixel]
  const green = left[leftPixel + 1] - right[rightPixel + 1]
  const blue = left[leftPixel + 2] - right[rightPixel + 2]
  return red * red * 2 + green * green * 4 + blue * blue
}

export function rasterPixelIndex(
  layer: RasterLayer,
  canvasX: number,
  canvasY: number,
): number {
  const x = canvasX - Math.round(layer.left)
  const y = canvasY - Math.round(layer.top)
  if (x < 0 || y < 0 || x >= layer.width || y >= layer.height) return -1
  return (y * layer.width + x) * 4
}

export function rasterAlphaAt(layer: RasterLayer, canvasX: number, canvasY: number) {
  const pixel = rasterPixelIndex(layer, canvasX, canvasY)
  return pixel < 0 ? 0 : layer.data[pixel + 3]
}
