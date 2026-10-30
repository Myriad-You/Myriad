import type { RasterLayer } from './anime25dImportTypes'
import { insideDistance } from '../anime25drig/pixelMasks'
import { trimRaster } from './anime25dRaster'

const LOCK = /^front-hair-\d+$/
/** Drawn: a lock's own pixel. */
const DRAWN = 16
/**
 * How far a lock reaches under its neighbour above, and over how much of that
 * it fades out, as shares of the front hair's width: turned, neighbouring
 * locks part by up to about this much (h08: 18–50 px on 800). A hard end
 * would cut across the strands above and show, once they part, as a torn edge.
 */
export const LOCK_UNDERLAP_SHARE = 0.05
const LOCK_UNDERLAP_FADE_SHARE = 0.02
const LOCK_UNDERLAP_MIN = 4

/**
 * Front hair cut into locks ('front-hair-N') is a partition: every pixel is
 * one lock's, the cut a hard line between two. Once the locks move apart (a
 * swing, a lock's own turn key) or are merely sampled off the pixel grid,
 * the cut opens and the skin under the fringe shows through as a pale seam.
 * Each lock is therefore drawn on a way under the locks above it, with
 * their own pixels, and where the cut opens the hair under it fills the gap —
 * as a hand-cut rig paints under its seams. Where the lock above is opaque it
 * hides that at rest; where it is translucent (the soft edges along its
 * strands, between which a copy under opaque pixels alone would show as
 * teeth) the pixel's cover is shared between the two, the one above keeping
 * what the one under it does not give, so the pair draws it as before.
 * Nothing at rest changes.
 */
export function underlapHairLocks(layers: RasterLayer[]): RasterLayer[] {
  const locks = layers.flatMap((layer, index) => (layer.role === 'front-hair' && LOCK.test(layer.id) ? [index] : []))
  if (locks.length < 2) return layers
  const left = Math.min(...locks.map((index) => layers[index].left))
  const right = Math.max(...locks.map((index) => layers[index].left + layers[index].width))
  const top = Math.min(...locks.map((index) => layers[index].top))
  const bottom = Math.max(...locks.map((index) => layers[index].top + layers[index].height))
  const reach = Math.max(LOCK_UNDERLAP_MIN, Math.round((right - left) * LOCK_UNDERLAP_SHARE))
  const fade = Math.max(1, (right - left) * LOCK_UNDERLAP_FADE_SHARE)
  // One frame round all the locks, grown by the reach.
  const frame = { left: left - reach, top: top - reach, width: right - left + reach * 2, height: bottom - top + reach * 2 }
  const size = frame.width * frame.height
  const datas = locks.map((index) => placed(layers[index], frame))
  // Which lock draws each pixel (the topmost), and how far each lock lies from it.
  const owner = new Int16Array(size).fill(-1)
  for (const [rank, data] of datas.entries()) {
    for (let p = 0; p < size; p += 1) {
      if (data[p * 4 + 3] > 0) owner[p] = rank
    }
  }
  const away = datas.map((data) => {
    const undrawn = new Uint8Array(size)
    for (let p = 0; p < size; p += 1) undrawn[p] = data[p * 4 + 3] > DRAWN ? 0 : 1
    return insideDistance(undrawn, frame.width, frame.height)
  })
  let added = 0
  for (let p = 0; p < size; p += 1) {
    const above = owner[p]
    if (above <= 0) continue
    // The lock under it within reach that is nearest gets a share of it.
    let under = -1
    let weight = 0
    for (let rank = 0; rank < above; rank += 1) {
      const w = Math.min(1, (reach - away[rank][p]) / fade)
      if (w > weight) {
        weight = w
        under = rank
      }
    }
    if (under < 0 || datas[under][p * 4 + 3] > DRAWN) continue
    const from = datas[above]
    const alpha = from[p * 4 + 3] / 255
    const take = alpha * weight
    if (take <= 0) continue
    datas[under].set(from.subarray(p * 4, p * 4 + 3), p * 4)
    datas[under][p * 4 + 3] = Math.round(take * 255)
    // What it keeps: drawn over the share given, the pair covers as it did alone.
    if (take < 0.999) from[p * 4 + 3] = Math.round(((alpha - take) / (1 - take)) * 255)
    added += 1
  }
  if (added === 0) return layers
  const out = layers.slice()
  for (const [rank, index] of locks.entries()) out[index] = trimRaster({ ...layers[index], ...frame, data: datas[rank] })
  return out
}

/** The layer's pixels in `frame`. */
function placed(layer: RasterLayer, frame: { left: number; top: number; width: number; height: number }): Uint8ClampedArray {
  const data = new Uint8ClampedArray(frame.width * frame.height * 4)
  for (let y = 0; y < layer.height; y += 1) {
    const start = y * layer.width * 4
    data.set(layer.data.subarray(start, start + layer.width * 4), ((y + layer.top - frame.top) * frame.width + layer.left - frame.left) * 4)
  }
  return data
}
