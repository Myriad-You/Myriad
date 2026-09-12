import type { NeckSurfaceContour } from './neckSurfaceContour'
import type { Anime25DPlaybackAnchors, Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'
import {
  buildNeckSurfaceContour,
  opaquePixelBounds,
} from './neckSurfaceContour'

export interface Anime25DNeckSurface {
  neck: Anime25DPlaybackLayer
  body: Anime25DPlaybackLayer
  fadeStart: number
  fadeEnd: number
  contour: NeckSurfaceContour
}

type ReadPixels = (layer: Anime25DPlaybackLayer) => CroppedLayerPixels | null

export function resolveAnime25DNeckSurface(
  layers: readonly Anime25DPlaybackLayer[],
  anchors: Anime25DPlaybackAnchors,
  readPixels: ReadPixels,
): Anime25DNeckSurface | null {
  if (layers.some((l) => l.role === 'collar-front' || l.role === 'collar-back'))
    return null
  const necks = layers.filter((l) => l.role === 'neck')
  if (necks.length !== 1) return null
  const neck = necks[0]
  const neckPixels = readPixels(neck)
  if (!neckPixels || neckPixels.height < 8) return null
  const bounds = opaquePixelBounds(neckPixels)
  if (!bounds) return null
  const visibleHeight = bounds.bottom - bounds.top
  const visibleWidth = bounds.right - bounds.left
  const candidates: Anime25DNeckSurface[] = []
  for (const body of layers.filter((l) => l.role === 'topwear')) {
    if (
      body.x >= neck.x + neck.w ||
      body.x + body.w <= neck.x ||
      body.y >= neck.y + neck.h
    ) {
      continue
    }
    const bodyPixels = readPixels(body)
    if (!bodyPixels) continue
    const top = Math.max(neck.y, anchors.face.y1)
    const upperEnd = top + Math.max(0, anchors.neckBottom - top) * 0.35
    let upper = 0
    let covered = 0
    let openRows = 0
    let openRun = 0
    const matching = new Uint8Array(neckPixels.height)
    const blending = new Uint8Array(neckPixels.height)
    const supported = new Uint8Array(neckPixels.height)
    const agreement = new Uint8Array(neckPixels.width * neckPixels.height)
    for (let y = bounds.top; y < bounds.bottom; y++) {
      const worldY = neck.y + ((y + 0.5) / neckPixels.height) * neck.h
      let rowPixels = 0
      let opaquePixels = 0
      let matches = 0
      let blendMatches = 0
      let unsupported = 0
      let rowCovered = 0
      for (let x = bounds.left; x < bounds.right; x++) {
        const n = (y * neckPixels.width + x) * 4
        if (neckPixels.pixels[n + 3] < 16) continue
        const worldX = neck.x + ((x + 0.5) / neckPixels.width) * neck.w
        const b = pixelAt(bodyPixels, body, worldX, worldY)
        const alpha = b < 0 ? 0 : bodyPixels.pixels[b + 3]
        if (worldY >= top && worldY <= upperEnd) {
          upper++
          if (alpha >= 16) covered++
        }
        rowPixels++
        if (alpha >= 16) rowCovered++
        if (alpha < 250) unsupported++
        if (neckPixels.pixels[n + 3] < 240 || alpha < 250) continue
        opaquePixels++
        const r = neckPixels.pixels[n] - bodyPixels.pixels[b]
        const g = neckPixels.pixels[n + 1] - bodyPixels.pixels[b + 1]
        const blue = neckPixels.pixels[n + 2] - bodyPixels.pixels[b + 2]
        const match = r * r * 2 + g * g * 4 + blue * blue <= 12 * 12 * 7
        if (r * r * 2 + g * g * 4 + blue * blue <= 24 * 24 * 7) blendMatches++
        agreement[y * neckPixels.width + x] = match ? 2 : 1
        if (match) matches++
      }
      supported[y] = rowPixels === 0 || unsupported === 0 ? 1 : 0
      if (worldY >= top && worldY <= upperEnd && rowPixels >= 4) {
        openRun = rowCovered / rowPixels < 0.2 ? openRun + 1 : 0
        openRows = Math.max(openRows, openRun)
      }
      const usable =
        supported[y] && opaquePixels >= Math.max(4, visibleWidth * 0.2)
      matching[y] = usable && matches / opaquePixels >= 0.85 ? 1 : 0
      // Strong seeds still require the original 12-level agreement. Allow a
      // gradual shadow difference only in the fully supported blend around
      // those seeds; it must not turn a matching stripe into a garment match.
      blending[y] = usable && blendMatches / opaquePixels >= 0.7 ? 1 : 0
    }
    // Even a pale garment that resembles skin cannot qualify through the lower colour test alone.
    const exposedRows =
      (Math.max(0, anchors.neckBottom - top) / neck.h) * neckPixels.height
    const openAperture = openRows >= Math.max(3, exposedRows * 0.12)
    if (
      upper < 24 ||
      (covered / upper >= 0.5 && (!openAperture || covered / upper >= 0.65))
    ) {
      continue
    }
    let end = bounds.bottom - 1
    while (end >= 0 && !matching[end]) end--
    if (end < bounds.top + visibleHeight * 0.85) continue
    let start = end
    while (start > bounds.top + visibleHeight * 0.5 && blending[start - 1])
      start--
    while (end + 1 < bounds.bottom && blending[end + 1]) end++
    if (supported.subarray(start, bounds.bottom).includes(0)) continue
    const strongRows = matching
      .subarray(start, end + 1)
      .reduce((a, b) => a + b, 0)
    if (strongRows < 3 || end - start + 1 < Math.max(4, visibleHeight * 0.08)) {
      continue
    }
    candidates.push({
      neck,
      body,
      fadeStart: (start + 0.5) / neckPixels.height,
      fadeEnd: (end + 0.5) / neckPixels.height,
      contour: buildNeckSurfaceContour(
        neckPixels.width,
        neckPixels.height,
        bounds,
        agreement,
        start,
        end,
      ),
    })
  }
  return candidates.length === 1 ? candidates[0] : null
}

function pixelAt(
  image: CroppedLayerPixels,
  source: Anime25DPlaybackLayer,
  x: number,
  y: number,
): number {
  const px = Math.floor(((x - source.x) / source.w) * image.width)
  const py = Math.floor(((y - source.y) / source.h) * image.height)
  return px < 0 || py < 0 || px >= image.width || py >= image.height
    ? -1
    : (py * image.width + px) * 4
}
