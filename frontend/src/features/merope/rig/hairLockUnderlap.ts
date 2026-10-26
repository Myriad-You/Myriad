import type { RasterLayer } from './anime25dImportTypes'
import { trimRaster } from './anime25dRaster'

const LOCK = /^front-hair-\d+$/
/** Drawn: a lock's own pixel. */
const DRAWN = 16
/** Opaque: a pixel of the lock above that hides what is under it at rest. */
const OPAQUE = 250
/** How far a lock reaches under its neighbour above, as a share of the front hair's width. */
export const LOCK_UNDERLAP_SHARE = 0.015
const LOCK_UNDERLAP_MIN = 4

/**
 * Front hair cut into locks ('front-hair-N') is a partition: every pixel is
 * one lock's, the cut a hard line between two. Once the locks move apart (a
 * swing, a lock's own turn key) or are merely sampled off the pixel grid,
 * the cut opens and the skin under the fringe shows through as a pale seam.
 * Each lock is therefore drawn on a little way under the locks above it,
 * with their own pixels: at rest the lock above hides it (only where that
 * one is opaque, so nothing at rest changes), and where the cut opens the
 * hair under it fills the gap — as a hand-cut rig paints under its seams.
 */
export function underlapHairLocks(layers: RasterLayer[]): RasterLayer[] {
  const locks = layers.flatMap((layer, index) => (layer.role === 'front-hair' && LOCK.test(layer.id) ? [index] : []))
  if (locks.length < 2) return layers
  const left = Math.min(...locks.map((index) => layers[index].left))
  const right = Math.max(...locks.map((index) => layers[index].left + layers[index].width))
  const reach = Math.max(LOCK_UNDERLAP_MIN, Math.round((right - left) * LOCK_UNDERLAP_SHARE))
  const out = layers.slice()
  for (const [rank, index] of locks.entries()) {
    const above = locks.slice(rank + 1).map((upper) => layers[upper])
    if (above.length > 0) out[index] = underlapOne(layers[index], above, reach)
  }
  return out
}

function alphaAt(layer: RasterLayer, x: number, y: number): number {
  const lx = x - layer.left
  const ly = y - layer.top
  if (lx < 0 || ly < 0 || lx >= layer.width || ly >= layer.height) return 0
  return layer.data[(ly * layer.width + lx) * 4 + 3]
}

/** The lock with the opaque pixels of the locks above it within `reach` of its own. */
function underlapOne(lock: RasterLayer, above: RasterLayer[], reach: number): RasterLayer {
  const left = lock.left - reach
  const top = lock.top - reach
  const width = lock.width + reach * 2
  const height = lock.height + reach * 2
  // Its drawn pixels grown by `reach` (a square), in the grown frame: rows, then columns.
  const drawn = new Uint8Array(width * height)
  for (let y = 0; y < lock.height; y += 1) {
    for (let x = 0; x < lock.width; x += 1) {
      if (lock.data[(y * lock.width + x) * 4 + 3] > DRAWN) drawn[(y + reach) * width + x + reach] = 1
    }
  }
  const across = new Uint8Array(width * height)
  for (let y = 0; y < height; y += 1) {
    let last = -Infinity
    for (let x = 0; x < width; x += 1) {
      if (drawn[y * width + x]) last = x
      if (x - last <= reach) across[y * width + x] = 1
    }
    last = Infinity
    for (let x = width - 1; x >= 0; x -= 1) {
      if (drawn[y * width + x]) last = x
      if (last - x <= reach) across[y * width + x] = 1
    }
  }
  const near = new Uint8Array(width * height)
  for (let x = 0; x < width; x += 1) {
    let last = -Infinity
    for (let y = 0; y < height; y += 1) {
      if (across[y * width + x]) last = y
      if (y - last <= reach) near[y * width + x] = 1
    }
    last = Infinity
    for (let y = height - 1; y >= 0; y -= 1) {
      if (across[y * width + x]) last = y
      if (last - y <= reach) near[y * width + x] = 1
    }
  }
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < lock.height; y += 1) {
    const start = y * lock.width * 4
    data.set(lock.data.subarray(start, start + lock.width * 4), ((y + reach) * width + reach) * 4)
  }
  let added = 0
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const at = y * width + x
      if (!near[at] || data[at * 4 + 3] > DRAWN) continue
      for (const upper of above) {
        if (alphaAt(upper, left + x, top + y) < OPAQUE) continue
        const from = ((top + y - upper.top) * upper.width + (left + x - upper.left)) * 4
        data.set(upper.data.subarray(from, from + 4), at * 4)
        added += 1
        break
      }
    }
  }
  return added === 0 ? lock : trimRaster({ ...lock, left, top, width, height, data })
}
