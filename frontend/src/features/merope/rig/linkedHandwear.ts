import type { EyeSide, RasterLayer } from './anime25dImportTypes'
import { rasterBounds } from './anime25dRaster'

/**
 * Hands that touch — clasped, gripping a glove, one resting on the other —
 * leave both arms in one connected drawing. The contract still wants a left
 * and a right fragment; the runtime moves touching arms as one piece, so the
 * cut only has to be reasonable, never exact. Each pixel goes to the shoulder
 * it is nearer to when walking inside the drawing, which puts the seam where
 * the two arms meet.
 */

/** Below this, the second side is a stray bit, not an arm: one arm is not two. */
const MIN_SIDE_SHARE = 0.15
const OPAQUE = 128
const DIAGONAL = Math.SQRT2

export interface Anime25DShoulderSeeds {
  left: { x: number; y: number }
  right: { x: number; y: number }
  /** A shoulder is never above the neck; a hand raised beside the face is not one. */
  minY: number
}

/** Where each shoulder sits, from the face and neck drawings, in canvas pixels. */
export function anime25DShoulderSeeds(layers: readonly RasterLayer[]): Anime25DShoulderSeeds | null {
  const face = layers.find((layer) => layer.role === 'face')
  const faceBounds = face ? rasterBounds(face) : null
  if (!face || !faceBounds) return null
  const neck = layers.find((layer) => layer.role === 'neck')
  const neckBounds = neck ? rasterBounds(neck) : null
  const faceWidth = faceBounds.width
  const faceHeight = faceBounds.height
  const centerX = neck && neckBounds
    ? neck.left + neckBounds.x + neckBounds.width / 2
    : face.left + faceBounds.x + faceWidth / 2
  const neckBottom = neck && neckBounds
    ? neck.top + neckBounds.y + neckBounds.height
    : face.top + faceBounds.y + faceHeight * 1.25
  // The same shoulder estimate the arm rig binds its joints from.
  const y = neckBottom + faceHeight * 0.04
  return {
    left: { x: centerX - faceWidth * 0.72, y },
    right: { x: centerX + faceWidth * 0.72, y },
    minY: neckBottom - faceHeight * 0.05,
  }
}

/**
 * Splits one connected drawing of both arms into image-left and image-right
 * fragments, or returns null when it is not two linked arms.
 */
export function splitLinkedHandwear(
  layer: RasterLayer,
  shoulders: Anime25DShoulderSeeds,
  bodyCenterX: number,
): Record<EyeSide, RasterLayer> | null {
  const { width, height, data } = layer
  const opaque = (pixel: number) => data[pixel * 4 + 3] >= OPAQUE
  let total = 0
  let leftOfCenter = 0
  for (let pixel = 0; pixel < width * height; pixel += 1) {
    if (!opaque(pixel)) continue
    total += 1
    if (layer.left + (pixel % width) < bodyCenterX) leftOfCenter += 1
  }
  if (total === 0) return null
  const share = leftOfCenter / total
  if (share < MIN_SIDE_SHARE || share > 1 - MIN_SIDE_SHARE) return null

  const seed = (point: { x: number; y: number }, exclude: number) => {
    let best = -1
    let nearest = Infinity
    for (let y = Math.max(0, Math.ceil(shoulders.minY - layer.top)); y < height; y += 1) {
      for (let x = 0; x < width; x += 1) {
        const pixel = y * width + x
        if (pixel === exclude || !opaque(pixel)) continue
        const distance = Math.hypot(layer.left + x - point.x, layer.top + y - point.y)
        if (distance < nearest) {
          nearest = distance
          best = pixel
        }
      }
    }
    return best
  }
  const leftSeed = seed(shoulders.left, -1)
  const rightSeed = seed(shoulders.right, leftSeed)
  if (leftSeed < 0 || rightSeed < 0) return null

  const owner = geodesicOwners(width, height, opaque, [leftSeed, rightSeed])
  const fragment = (label: number): RasterLayer => {
    const output = { ...layer, data: new Uint8ClampedArray(data) }
    for (let pixel = 0; pixel < width * height; pixel += 1) {
      // Faint pixels no path reached still belong to one side, never to neither.
      const side = owner[pixel] >= 0 ? owner[pixel] : layer.left + (pixel % width) < bodyCenterX ? 0 : 1
      if (side !== label) output.data[pixel * 4 + 3] = 0
    }
    return output
  }
  return { left: fragment(0), right: fragment(1) }
}

/**
 * Multi-source shortest paths inside the drawing (8-connected). Every pixel
 * with any alpha is labelled so antialiased edges travel with their arm.
 */
function geodesicOwners(
  width: number,
  height: number,
  opaque: (pixel: number) => boolean,
  seeds: readonly number[],
): Int8Array {
  const size = width * height
  const distance = new Float64Array(size).fill(Infinity)
  const owner = new Int8Array(size).fill(-1)
  const heap = new MinHeap()
  seeds.forEach((pixel, label) => {
    distance[pixel] = 0
    owner[pixel] = label
    heap.push(pixel, 0)
  })
  while (heap.size > 0) {
    const pixel = heap.popPixel()
    const reached = heap.lastPriority
    if (reached > distance[pixel]) continue
    const x = pixel % width
    const y = (pixel - x) / width
    for (let dy = -1; dy <= 1; dy += 1) {
      const ny = y + dy
      if (ny < 0 || ny >= height) continue
      for (let dx = -1; dx <= 1; dx += 1) {
        const nx = x + dx
        if ((dx === 0 && dy === 0) || nx < 0 || nx >= width) continue
        const next = ny * width + nx
        if (!opaque(next)) continue
        const candidate = reached + (dx !== 0 && dy !== 0 ? DIAGONAL : 1)
        if (candidate < distance[next]) {
          distance[next] = candidate
          owner[next] = owner[pixel]
          heap.push(next, candidate)
        }
      }
    }
  }
  // Translucent rims and specks off the opaque body take their nearest labelled neighbour.
  for (let pass = 0; pass < 3; pass += 1) {
    for (let pixel = 0; pixel < size; pixel += 1) {
      if (owner[pixel] >= 0) continue
      const x = pixel % width
      for (const next of [pixel - 1, pixel + 1, pixel - width, pixel + width]) {
        if (next < 0 || next >= size || Math.abs((next % width) - x) > 1) continue
        if (owner[next] >= 0) {
          owner[pixel] = owner[next]
          break
        }
      }
    }
  }
  return owner
}

class MinHeap {
  size = 0
  lastPriority = 0
  private pixels = new Int32Array(1024)
  private priorities = new Float64Array(1024)

  push(pixel: number, priority: number): void {
    if (this.size === this.pixels.length) {
      const pixels = new Int32Array(this.size * 2)
      const priorities = new Float64Array(this.size * 2)
      pixels.set(this.pixels)
      priorities.set(this.priorities)
      this.pixels = pixels
      this.priorities = priorities
    }
    let index = this.size++
    while (index > 0) {
      const parent = (index - 1) >> 1
      if (this.priorities[parent] <= priority) break
      this.pixels[index] = this.pixels[parent]
      this.priorities[index] = this.priorities[parent]
      index = parent
    }
    this.pixels[index] = pixel
    this.priorities[index] = priority
  }

  popPixel(): number {
    const pixel = this.pixels[0]
    this.lastPriority = this.priorities[0]
    const lastPixel = this.pixels[--this.size]
    const lastPriority = this.priorities[this.size]
    let index = 0
    for (;;) {
      const left = index * 2 + 1
      if (left >= this.size) break
      const right = left + 1
      const child = right < this.size && this.priorities[right] < this.priorities[left] ? right : left
      if (this.priorities[child] >= lastPriority) break
      this.pixels[index] = this.pixels[child]
      this.priorities[index] = this.priorities[child]
      index = child
    }
    this.pixels[index] = lastPixel
    this.priorities[index] = lastPriority
    return pixel
  }
}
