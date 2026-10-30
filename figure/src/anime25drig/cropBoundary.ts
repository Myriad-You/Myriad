import type { Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'

interface CropMesh {
  source: Anime25DPlaybackLayer
  rest: Float32Array
  deformed: Float32Array
}

export interface CropBoundary {
  host: CropMesh
  edge: number[]
  weights: Float32Array
  ownEdge: number[]
  columns: Uint16Array
  deltas: Float32Array
}

// A bounding-box match alone also catches natural hair tips. Require a broad
// opaque run reaching the actual image edge, not an inset, tapered silhouette.
function hasCutEdge(image: CroppedLayerPixels): boolean {
  const { width, height, pixels } = image
  if (height < 4) return false
  for (const y of [height - 1, height - 3]) {
    let run = 0
    let longest = 0
    for (let x = 0; x < width; x++) {
      run = pixels[(y * width + x) * 4 + 3] >= 128 ? run + 1 : 0
      longest = Math.max(longest, run)
    }
    if (longest < Math.max(24, width * 0.06)) return false
  }
  return true
}

/** Bind only shared canvas cuts, never ordinary layer/garment boundaries. */
export function bindCropBoundary(
  layer: CropMesh,
  host: CropMesh,
  image: CroppedLayerPixels | null,
  hostImage: CroppedLayerPixels | null,
  contentBottom: number,
): CropBoundary | null {
  const source = layer.source
  const bottom = source.y + source.h
  if (
    layer === host ||
    !['back-hair', 'front-hair', 'handwear'].includes(source.role) ||
    Math.abs(bottom - contentBottom) > 0.5 ||
    Math.abs(host.source.y + host.source.h - bottom) > 0.5 ||
    !image ||
    !hostImage ||
    !hasCutEdge(image) ||
    !hasCutEdge(hostImage)
  ) {
    return null
  }
  const edge: number[] = []
  for (let i = 0; i < host.rest.length; i += 2) {
    if (Math.abs(host.rest[i + 1] - bottom) < 0.5) edge.push(i)
  }
  edge.sort((a, b) => host.rest[a] - host.rest[b])
  if (edge.length < 2) return null
  const ownEdge: number[] = []
  for (let i = 0; i < layer.rest.length; i += 2) {
    if (Math.abs(layer.rest[i + 1] - bottom) < 0.5) ownEdge.push(i)
  }
  if (ownEdge.length < 2) return null
  const band = Math.min(source.h, host.source.h) * 0.4
  const weights = new Float32Array(layer.rest.length / 2)
  const columns = new Uint16Array(weights.length)
  for (let i = 0; i < weights.length; i++) {
    const t = Math.max(0, Math.min(1, 1 - (bottom - layer.rest[i * 2 + 1]) / band))
    weights[i] = t * t * (3 - 2 * t)
    let distance = Infinity
    for (let j = 0; j < ownEdge.length; j++) {
      const d = Math.abs(layer.rest[ownEdge[j]] - layer.rest[i * 2])
      if (d < distance) {
        distance = d
        columns[i] = j
      }
    }
  }
  return {
    host, edge, weights, ownEdge, columns,
    deltas: new Float32Array(ownEdge.length),
  }
}

/** Run on fresh local geometry, after contacts/physics and before attachments. */
export function applyCropBoundary(binding: CropBoundary, vertices: Float32Array): boolean {
  const { host, edge, weights, ownEdge, columns, deltas } = binding
  const points = host.deformed
  // A folded host is not a valid y(x) boundary; do not propagate its inversion.
  for (let j = 1; j < edge.length; j++) {
    if (points[edge[j]] <= points[edge[j - 1]]) return false
  }
  // Sample every column before modifying any vertices: no in-place feedback.
  for (let i = 0; i < ownEdge.length; i++) {
    const index = ownEdge[i]
    const x = vertices[index]
    let j = 1
    while (j < edge.length - 1 && points[edge[j]] < x) j++
    const a = edge[j - 1]
    const b = edge[j]
    const t = (x - points[a]) / (points[b] - points[a])
    const target = points[a + 1] + t * (points[b + 1] - points[a + 1])
    deltas[i] = target - vertices[index + 1]
  }
  let changed = false
  for (let i = 0; i < weights.length; i++) {
    const weight = weights[i]
    if (!weight) continue
    // Translate the lower band by its edge mismatch rather than flattening it.
    const y = vertices[i * 2 + 1]
    const next = Math.fround(y + deltas[columns[i]] * weight)
    if (next !== y) {
      vertices[i * 2 + 1] = next
      changed = true
    }
  }
  return changed
}
