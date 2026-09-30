import type { Anime25DSourceReference, RasterLayer } from './anime25dImportTypes'
import type { Anime25DPsdAnalysis } from './psdReconciliation'
import { MATCH_DISTANCE, rgbDistance } from './psdReconciliation'

const RECOVER_FRINGE = 1

export function alphaAt(layer: RasterLayer, x: number, y: number): number {
  const localX = x - layer.left
  const localY = y - layer.top
  if (
    localX < 0 ||
    localY < 0 ||
    localX >= layer.width ||
    localY >= layer.height
  ) {
    return 0
  }
  return layer.data[(localY * layer.width + localX) * 4 + 3]
}

/** Neighbours within `radius` of a region, each paired with its nearest member. */
export function fringe(
  members: readonly number[],
  width: number,
  height: number,
  radius: number,
): Map<number, number> {
  const ring = new Map<number, number>()
  for (const member of members) {
    const x = member % width
    const y = Math.floor(member / width)
    for (let dy = -radius; dy <= radius; dy += 1) {
      for (let dx = -radius; dx <= radius; dx += 1) {
        const nx = x + dx
        const ny = y + dy
        if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue
        const next = ny * width + nx
        if (!ring.has(next)) ring.set(next, member)
      }
    }
  }
  return ring
}

/** Recovers anti-aliased edges of art that sits on the flat backdrop. */
export function addAntialiasedRim(
  analysis: Readonly<Anime25DPsdAnalysis>,
  members: readonly number[],
  reference: Readonly<Anime25DSourceReference>,
  pixels: Map<number, readonly [number, number, number, number]>,
): void {
  const { bounds, background } = analysis
  if (!background.known) return
  for (const [target] of fringe(
    members,
    bounds.width,
    bounds.height,
    RECOVER_FRINGE,
  )) {
    if (analysis.alpha[target] >= 0.5) continue
    const x = bounds.x0 + (target % bounds.width)
    const y = bounds.y0 + Math.floor(target / bounds.width)
    const key = y * reference.width + x
    if (pixels.has(key)) continue
    const offset = key * 4
    if (reference.data[offset + 3] < 250) continue
    const coverage = Math.min(
      1,
      rgbDistance(reference.data, offset, background.color) / MATCH_DISTANCE,
    )
    if (coverage < 0.05) continue
    const unmix = (channel: number) =>
      Math.round(
        (reference.data[offset + channel] -
          background.color[channel] * (1 - coverage)) /
          coverage,
      )
    pixels.set(key, [unmix(0), unmix(1), unmix(2), Math.round(coverage * 255)])
  }
}

export function meanCoveredDistance(
  analysis: Readonly<Anime25DPsdAnalysis>,
  members: readonly number[],
  visible: readonly RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): number {
  const { bounds } = analysis
  let sum = 0
  for (const target of members) {
    const layer = visible[analysis.covered[target]]
    const x = bounds.x0 + (target % bounds.width)
    const y = bounds.y0 + Math.floor(target / bounds.width)
    const offset = ((y - layer.top) * layer.width + (x - layer.left)) * 4
    const referenceOffset = (y * reference.width + x) * 4
    sum += Math.hypot(
      layer.data[offset] - reference.data[referenceOffset],
      layer.data[offset + 1] - reference.data[referenceOffset + 1],
      layer.data[offset + 2] - reference.data[referenceOffset + 2],
    )
  }
  return members.length > 0 ? sum / members.length : 0
}

/** Square-kernel morphological closing, separable per axis. */
export function closeMask(
  mask: Uint8Array,
  width: number,
  height: number,
  radius: number,
): Uint8Array {
  const dilated = sweep(
    sweep(mask, width, height, radius, 1, true),
    width,
    height,
    radius,
    width,
    true,
  )
  return sweep(
    sweep(dilated, width, height, radius, 1, false),
    width,
    height,
    radius,
    width,
    false,
  )
}

/** One axis of a dilation (`grow`) or erosion; `step` 1 is x, `width` is y. */
function sweep(
  mask: Uint8Array,
  width: number,
  height: number,
  radius: number,
  step: number,
  grow: boolean,
): Uint8Array {
  const output = new Uint8Array(mask.length)
  const length = step === 1 ? width : height
  const lines = step === 1 ? height : width
  for (let line = 0; line < lines; line += 1) {
    const start = step === 1 ? line * width : line
    for (let position = 0; position < length; position += 1) {
      let value = grow ? 0 : 1
      for (let offset = -radius; offset <= radius; offset += 1) {
        const other = position + offset
        // Outside counts as set when eroding so edges are not eaten away.
        const sample =
          other < 0 || other >= length
            ? grow
              ? 0
              : 1
            : mask[start + other * step]
        if (grow ? sample : !sample) {
          value = grow ? 1 : 0
          break
        }
      }
      output[start + position * step] = value
    }
  }
  return output
}

/** Fills enclosed gaps, e.g. hair seen through a birdcage, up to `maxArea`. */
export function fillSmallHoles(
  mask: Uint8Array,
  width: number,
  height: number,
  maxArea: number,
): void {
  const inverse = new Uint8Array(mask.length)
  for (let index = 0; index < mask.length; index += 1) {
    inverse[index] = mask[index] ? 0 : 1
  }
  for (const hole of components(inverse, width, height)) {
    if (hole.length > maxArea) continue
    if (
      hole.some((target) => {
        const x = target % width
        const y = Math.floor(target / width)
        return x === 0 || y === 0 || x === width - 1 || y === height - 1
      })
    ) {
      continue
    }
    for (const target of hole) mask[target] = 1
  }
}

export function components(
  mask: Uint8Array,
  width: number,
  height: number,
): number[][] {
  const visited = new Uint8Array(mask.length)
  const result: number[][] = []
  const stack: number[] = []
  for (let start = 0; start < mask.length; start += 1) {
    if (!mask[start] || visited[start]) continue
    const members: number[] = []
    visited[start] = 1
    stack.push(start)
    while (stack.length > 0) {
      const target = stack.pop()!
      members.push(target)
      const x = target % width
      const y = Math.floor(target / width)
      for (const next of [
        x > 0 ? target - 1 : -1,
        x < width - 1 ? target + 1 : -1,
        y > 0 ? target - width : -1,
        y < height - 1 ? target + width : -1,
      ]) {
        if (next < 0 || visited[next] || !mask[next]) continue
        visited[next] = 1
        stack.push(next)
      }
    }
    result.push(members)
  }
  return result
}
