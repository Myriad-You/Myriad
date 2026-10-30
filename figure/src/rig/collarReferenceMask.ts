import type { Anime25DSourceReference, RasterLayer } from './anime25dImportTypes'
import type { CollarReferenceMask } from './collarCommon'
import { ALPHA_COMPONENT_THRESHOLD, COLLAR_REFERENCE_FRONT, COLLAR_REFERENCE_REAR, perceptualColorDistance, rasterPixelIndex } from './collarCommon'

const COLLAR_REFERENCE_REAR_CONFIDENCE = 0.08
const COLLAR_REFERENCE_FRONT_CONFIDENCE = 0.15
const COLLAR_REFERENCE_REAR_MATCH_DISTANCE = 32 * 32 * 7
const COLLAR_REFERENCE_FRONT_MATCH_DISTANCE = 48 * 48 * 7
/**
 * A collar in front of the neck is cloth, not skin: where the garment is
 * drawn in the neck's own colours (a decomposition paints the open chest
 * into the shirt), the portrait matching it says nothing about which is in
 * front, and the neck keeps it.
 */
const COLLAR_REFERENCE_FRONT_MATERIAL_DISTANCE = 24 * 24 * 7
/** How near the neck's outline a front collar piece must reach: it comes in from outside the neck. */
const COLLAR_FRONT_OUTLINE_REACH = 3

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
        confidence >= COLLAR_REFERENCE_FRONT_CONFIDENCE &&
        perceptualColorDistance(neck.data, neckPixel, topwear.data, topwearPixel) >
          COLLAR_REFERENCE_FRONT_MATERIAL_DISTANCE
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
  dropInnerFrontPieces(filled, width, height, (x, y) => {
    const pixel = rasterPixelIndex(neck, left + x, exposedTop + y)
    return pixel >= 0 && neck.data[pixel + 3] >= ALPHA_COMPONENT_THRESHOLD
  })
  return { left, top: exposedTop, width, height, data: filled }
}

/**
 * Front pieces wholly inside the neck are not a collar in front of it: a
 * collar comes in over the neck from its sides or its foot. A chain, a
 * shading line or the shirt's skin the portrait happens to match there are
 * islands, and go behind the neck (rear) instead of moving with the body
 * in front of it.
 */
function dropInnerFrontPieces(
  data: Uint8Array,
  width: number,
  height: number,
  isNeck: (x: number, y: number) => boolean,
): void {
  const reach = COLLAR_FRONT_OUTLINE_REACH
  const nearOutline = (x: number, y: number) => {
    if (y >= height - reach || x < reach || x >= width - reach) return true
    for (let dy = -reach; dy <= reach; dy += 1) {
      for (let dx = -reach; dx <= reach; dx += 1) {
        if (!isNeck(x + dx, y + dy)) return true
      }
    }
    return false
  }
  const seen = new Uint8Array(width * height)
  const stack: number[] = []
  for (let start = 0; start < data.length; start += 1) {
    if (data[start] !== COLLAR_REFERENCE_FRONT || seen[start]) continue
    const piece: number[] = []
    let reaches = false
    seen[start] = 1
    stack.push(start)
    while (stack.length > 0) {
      const pixel = stack.pop()!
      piece.push(pixel)
      const x = pixel % width
      const y = (pixel - x) / width
      if (!reaches && nearOutline(x, y)) reaches = true
      for (const [nx, ny] of [[x - 1, y], [x + 1, y], [x, y - 1], [x, y + 1]]) {
        if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue
        const next = ny * width + nx
        if (seen[next] || data[next] !== COLLAR_REFERENCE_FRONT) continue
        seen[next] = 1
        stack.push(next)
      }
    }
    if (!reaches) {
      for (const pixel of piece) data[pixel] = COLLAR_REFERENCE_REAR
    }
  }
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
