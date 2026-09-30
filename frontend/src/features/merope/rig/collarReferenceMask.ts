import type { Anime25DSourceReference, RasterLayer } from './anime25dImportTypes'
import type { CollarReferenceMask } from './collarCommon'
import { ALPHA_COMPONENT_THRESHOLD, COLLAR_REFERENCE_FRONT, COLLAR_REFERENCE_REAR, perceptualColorDistance, rasterPixelIndex } from './collarCommon'

const COLLAR_REFERENCE_REAR_CONFIDENCE = 0.08
const COLLAR_REFERENCE_FRONT_CONFIDENCE = 0.15
const COLLAR_REFERENCE_REAR_MATCH_DISTANCE = 32 * 32 * 7
const COLLAR_REFERENCE_FRONT_MATCH_DISTANCE = 48 * 48 * 7

export function buildCollarReferenceMask(
  reference: Anime25DSourceReference,
  neck: RasterLayer,
  topwear: RasterLayer,
  exposedTop: number,
  exposedBottom: number,
): CollarReferenceMask {
  const left = Math.ceil(neck.left)
  const right = Math.floor(neck.left + neck.width)
  const width = right - left
  const height = exposedBottom - exposedTop + 1
  const raw = new Uint8Array(width * height)
  for (let y = exposedTop; y <= exposedBottom; y += 1) {
    for (let x = left; x < right; x += 1) {
      if (x < 0 || y < 0 || x >= reference.width || y >= reference.height) {
        continue
      }
      const referencePixel = (y * reference.width + x) * 4
      if (reference.data[referencePixel + 3] < ALPHA_COMPONENT_THRESHOLD) {
        continue
      }
      const neckPixel = rasterPixelIndex(neck, x, y)
      const topwearPixel = rasterPixelIndex(topwear, x, y)
      if (
        neckPixel < 0 ||
        topwearPixel < 0 ||
        neck.data[neckPixel + 3] < ALPHA_COMPONENT_THRESHOLD ||
        topwear.data[topwearPixel + 3] < ALPHA_COMPONENT_THRESHOLD
      ) {
        continue
      }
      const neckDistance = perceptualColorDistance(
        reference.data,
        referencePixel,
        neck.data,
        neckPixel,
      )
      const topwearDistance = perceptualColorDistance(
        reference.data,
        referencePixel,
        topwear.data,
        topwearPixel,
      )
      const confidence =
        Math.abs(neckDistance - topwearDistance) /
        (neckDistance + topwearDistance + 1)
      const pixel = (y - exposedTop) * width + x - left
      if (
        neckDistance < topwearDistance &&
        neckDistance <= COLLAR_REFERENCE_REAR_MATCH_DISTANCE &&
        confidence >= COLLAR_REFERENCE_REAR_CONFIDENCE
      ) {
        raw[pixel] = COLLAR_REFERENCE_REAR
      } else if (
        topwearDistance < neckDistance &&
        topwearDistance <= COLLAR_REFERENCE_FRONT_MATCH_DISTANCE &&
        confidence >= COLLAR_REFERENCE_FRONT_CONFIDENCE
      ) {
        raw[pixel] = COLLAR_REFERENCE_FRONT
      }
    }
  }

  const cleaned = new Uint8Array(raw.length)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const pixel = y * width + x
      const value = raw[pixel]
      if (value === 0) continue
      const neighbours = collarReferenceNeighbourCounts(
        raw,
        width,
        height,
        x,
        y,
      )
      if (
        (value === COLLAR_REFERENCE_REAR && neighbours.rear >= 3) ||
        (value === COLLAR_REFERENCE_FRONT && neighbours.front >= 3)
      ) {
        cleaned[pixel] = value
      }
    }
  }
  const filled = new Uint8Array(cleaned)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const pixel = y * width + x
      if (cleaned[pixel] !== 0) continue
      const neighbours = collarReferenceNeighbourCounts(
        cleaned,
        width,
        height,
        x,
        y,
      )
      if (neighbours.front >= 6 && neighbours.rear <= 1) {
        filled[pixel] = COLLAR_REFERENCE_FRONT
      } else if (neighbours.rear >= 6 && neighbours.front <= 1) {
        filled[pixel] = COLLAR_REFERENCE_REAR
      }
    }
  }
  return { left, top: exposedTop, width, height, data: filled }
}

function collarReferenceNeighbourCounts(
  data: Uint8Array,
  width: number,
  height: number,
  centerX: number,
  centerY: number,
): { rear: number; front: number } {
  let rear = 0
  let front = 0
  for (
    let y = Math.max(0, centerY - 1);
    y <= Math.min(height - 1, centerY + 1);
    y += 1
  ) {
    for (
      let x = Math.max(0, centerX - 1);
      x <= Math.min(width - 1, centerX + 1);
      x += 1
    ) {
      const value = data[y * width + x]
      if (value === COLLAR_REFERENCE_REAR) rear += 1
      else if (value === COLLAR_REFERENCE_FRONT) front += 1
    }
  }
  return { rear, front }
}

export function collarReferenceMaskAt(
  mask: CollarReferenceMask | undefined,
  x: number,
  y: number,
): number {
  if (!mask) return 0
  const localX = x - mask.left
  const localY = y - mask.top
  if (
    localX < 0 ||
    localX >= mask.width ||
    localY < 0 ||
    localY >= mask.height
  ) {
    return 0
  }
  return mask.data[localY * mask.width + localX]
}
