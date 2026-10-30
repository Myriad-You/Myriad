import type { Anime25DPlaybackLayer } from './types'
import type { CroppedLayerPixels } from './webglRuntime'

/** Head pieces drawn as a pair either side of the face, which one layer may hold both of. */
const PAIRED_ROLES = new Set(['earwear', 'headwear'])

/**
 * A pair either side of the face in one layer (two earrings, two hair clips)
 * as two layers, one each side: each then hangs from what it is on (an ear,
 * the lock it is clipped to) instead of both from a point between them. Only
 * across a clear transparent gap about the face's middle; a band or a hat
 * that goes across stays whole.
 */
export function splitPairedHeadAccessory(
  source: Anime25DPlaybackLayer,
  art: CroppedLayerPixels | null,
  faceCenterX: number,
): Anime25DPlaybackLayer[] | null {
  if (!PAIRED_ROLES.has(source.role) || source.group !== 'head' || source.side || source.phys || source.fade || !art) return null
  if (art.width < 4 || art.height < 1 || art.pixels.length !== art.width * art.height * 4) return null
  const center = Math.floor((faceCenterX - source.x) / source.w * art.width)
  if (center <= 0 || center >= art.width - 1) return null
  const columns = new Uint32Array(art.width)
  for (let y = 0; y < art.height; y++) { for (let x = 0; x < art.width; x++) {
    if (art.pixels[(y * art.width + x) * 4 + 3] > 0) columns[x]++
  }
}
  if (columns[center]) return null
  let left = center; let right = center
  while (left > 0 && columns[left - 1] === 0) left--
  while (right + 1 < art.width && columns[right + 1] === 0) right++
  // Require a real transparent separation around the face, not a thin chain
  // or a gap between beads in a single earring. Preserve even alpha fringes.
  if (right - left + 1 < Math.max(2, art.width * 0.12)) return null
  if (columns.slice(0, left).reduce((n, v) => n + v, 0) < 16 ||
    columns.slice(right + 1).reduce((n, v) => n + v, 0) < 16) { return null
}
  const cut = Math.floor((left + right + 1) / 2) / art.width
  return ([['L', 0, cut], ['R', cut, 1]] as const).map(([side, start, end]) => ({
    ...source, name: `${source.name}-${side}`, side,
    x: source.x + source.w * start, w: source.w * (end - start),
    atlas: { ...source.atlas, x: source.atlas.x + source.atlas.w * start, w: source.atlas.w * (end - start) },
  }))
}

/**
 * A disconnected neck ornament accidentally included in a headwear layer.
 * Require co-located shape and colour evidence; never match a flower elsewhere.
 */
export function removeDuplicatedNeckComponents(
  source: Anime25DPlaybackLayer,
  art: CroppedLayerPixels,
  targets: readonly {
    layer: Anime25DPlaybackLayer
    image: CroppedLayerPixels
  }[],
  neckTop: number,
): CroppedLayerPixels | null {
  if (
    source.role !== 'headwear' ||
    source.fade ||
    source.phys ||
    !targets.length
  ) {
    return null
}
  const n = art.width * art.height
  if (n <= 0 || art.pixels.length !== n * 4) return null
  const visited = new Uint8Array(n)
    const components: number[][] = []
  for (let start = 0; start < n; start++) {
    if (visited[start] || art.pixels[start * 4 + 3] === 0) continue
    const queue = [start]
    visited[start] = 1
    for (let k = 0; k < queue.length; k++) {
      const p = queue[k]
        const x = p % art.width
        const y = Math.floor(p / art.width)
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          const nx = x + dx
            const ny = y + dy
            const q = ny * art.width + nx
          if (
            nx < 0 ||
            ny < 0 ||
            nx >= art.width ||
            ny >= art.height ||
            visited[q] ||
            art.pixels[q * 4 + 3] === 0
          ) {
            continue
}
          visited[q] = 1
          queue.push(q)
        }
}
    }
    components.push(queue)
  }
  if (components.length < 2) return null
  let output: Uint8ClampedArray | null = null
  for (const component of components) {
    const solid = component.filter((p) => art.pixels[p * 4 + 3] >= 220)
    if (solid.length < 32 || component.length > n * 0.5) continue
    const world = (p: number) => ({
      x: source.x + (((p % art.width) + 0.5) / art.width) * source.w,
      y: source.y + ((Math.floor(p / art.width) + 0.5) / art.height) * source.h,
    })
    if (solid.some((p) => world(p).y < neckTop)) continue
    const match = targets.some(({ layer: b, image }) => {
      if (b.role !== 'neckwear' || b.fade || b.phys) return false
      // Segmentation/repacking can shift the same contour by one texel and
      // repaint antialiasing. Require coverage, colour AND correlated detail,
      // rather than exact RGBA or a broad search for similarly coloured flowers.
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          let covered = 0
            let close = 0
            let errorSum = 0
            let sa = 0
            let sb = 0
            let saa = 0
            let sbb = 0
            let sab = 0
          for (const p of solid) {
            const { x, y } = world(p)
            const bx = Math.floor(((x - b.x) / b.w) * image.width) + dx
              const by = Math.floor(((y - b.y) / b.h) * image.height) + dy
            if (bx < 0 || by < 0 || bx >= image.width || by >= image.height)
              continue
            const q = (by * image.width + bx) * 4
            if (image.pixels[q + 3] < 220) continue
            covered++
            const error = Math.max(
              ...[0, 1, 2].map((c) =>
                Math.abs(art.pixels[p * 4 + c] - image.pixels[q + c]),
              ),
            )
            if (error <= 24) close++
            errorSum += error
            const a =
              (art.pixels[p * 4] +
                art.pixels[p * 4 + 1] +
                art.pixels[p * 4 + 2]) /
              3
            const v =
              (image.pixels[q] + image.pixels[q + 1] + image.pixels[q + 2]) / 3
            sa += a
            sb += v
            saa += a * a
            sbb += v * v
            sab += a * v
          }
          if (
            covered / solid.length < 0.97 ||
            close / solid.length < 0.75 ||
            errorSum / covered > 16
          ) {
            continue
}
          const va = saa - (sa * sa) / covered
            const vb = sbb - (sb * sb) / covered
          // Flat colour overlap is not evidence of a duplicated ornament.
          if (va / covered < 100 || vb / covered < 100) continue
          if ((sab - (sa * sb) / covered) / Math.sqrt(va * vb) >= 0.75)
            return true
        }
}
      return false
    })
    if (!match) continue
    output ??= art.pixels.slice()
    for (const p of component) output[p * 4 + 3] = 0
  }
  return output ? { ...art, pixels: output } : null
}
