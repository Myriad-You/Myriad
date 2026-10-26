import type { RasterLayer } from './anime25dImportTypes'
import { closeMask, components } from './psdRepairMasks'

/** The neck under it: opaque. */
const NECK_ALPHA = 250
/** The top's pixel is the neck's skin when its colour is this close to the neck's under it (RGB distance)… */
const SKIN_NEAR = 45
/** …and its hue too (R−G, G−B): a pale shirt is as light as skin, not as warm. */
const HUE_NEAR = 16
/** The translucent rim around that skin (the line a decomposition guesses where it ends) goes this far. */
const RIM = 3
/**
 * The skin is one sheet: pieces of it (closed over this many pixels) a fifth
 * the size of the largest or less are a garment's own shading that happens
 * to match the skin, and stay.
 */
const SHEET_CLOSING = 2
const SHEET_SHARE = 0.2

/**
 * A decomposition can draw the skin of the chest and throat into the top as
 * well as into the neck: a translucent wash of skin over the neck, a guessed
 * line where it ends, and (once a cut by the picture has taken pieces out of
 * it) a stepped edge where the wash stops and the neck shows, all of it a
 * shade off the neck. At rest it shows as a grey stair across the throat, and
 * as the head turns the neck moves under a skin that stays. The neck is the
 * skin: where the top over it is skin, that goes, and so does the translucent
 * rim around it. A collar or any garment over the neck, not the skin's
 * colour, stays.
 */
export function dropTopSkinOverNeck(layers: RasterLayer[], visibleAtRest: (layer: RasterLayer) => boolean): RasterLayer[] {
  const necks = layers.flatMap((layer, index) => (layer.role === 'neck' && visibleAtRest(layer) ? [index] : []))
  if (necks.length === 0) return layers
  const out = layers.slice()
  for (const [index, top] of layers.entries()) {
    if (top.role !== 'topwear' || !visibleAtRest(top)) continue
    const under = necks.filter((neck) => neck < index).map((neck) => layers[neck])
    if (under.length > 0) out[index] = dropOver(top, under)
  }
  return out
}

function dropOver(top: RasterLayer, necks: RasterLayer[]): RasterLayer {
  const { width, height, data } = top
  // Per top pixel over an opaque neck: where in that neck (-1 none).
  const neckAt = new Int32Array(width * height).fill(-1)
  const neckOf = new Int16Array(width * height)
  for (const [n, neck] of necks.entries()) {
    const x0 = Math.max(0, neck.left - top.left)
    const y0 = Math.max(0, neck.top - top.top)
    const x1 = Math.min(width, neck.left + neck.width - top.left)
    const y1 = Math.min(height, neck.top + neck.height - top.top)
    for (let y = y0; y < y1; y += 1) {
      for (let x = x0; x < x1; x += 1) {
        const at = (y + top.top - neck.top) * neck.width + x + top.left - neck.left
        if (neck.data[at * 4 + 3] < NECK_ALPHA) continue
        neckAt[y * width + x] = at
        neckOf[y * width + x] = n
      }
    }
  }
  const skin = new Uint8Array(width * height)
  for (let p = 0; p < width * height; p += 1) {
    if (neckAt[p] < 0 || data[p * 4 + 3] === 0) continue
    const neck = necks[neckOf[p]].data
    const at = neckAt[p] * 4
    const [r, g, b] = [data[p * 4], data[p * 4 + 1], data[p * 4 + 2]]
    if (Math.hypot(r - neck[at], g - neck[at + 1], b - neck[at + 2]) >= SKIN_NEAR) continue
    if (Math.hypot(r - g - (neck[at] - neck[at + 1]), g - b - (neck[at + 1] - neck[at + 2])) >= HUE_NEAR) continue
    skin[p] = 1
  }
  const pieces = components(closeMask(skin, width, height, SHEET_CLOSING), width, height)
  const largest = Math.max(0, ...pieces.map((piece) => piece.length))
  const drop = new Uint8Array(width * height)
  let dropped = 0
  for (const piece of pieces) {
    if (piece.length < largest * SHEET_SHARE) continue
    for (const p of piece) {
      if (!skin[p]) continue
      drop[p] = 1
      dropped += 1
    }
  }
  if (dropped === 0) return top
  const out = new Uint8ClampedArray(data)
  for (let p = 0; p < width * height; p += 1) {
    if (drop[p]) {
      out[p * 4 + 3] = 0
      continue
    }
    if (neckAt[p] < 0 || data[p * 4 + 3] === 0 || data[p * 4 + 3] >= NECK_ALPHA) continue
    const x = p % width
    const y = (p - x) / width
    let near = false
    for (let dy = -RIM; dy <= RIM && !near; dy += 1) {
      for (let dx = -RIM; dx <= RIM && !near; dx += 1) {
        const qx = x + dx
        const qy = y + dy
        if (qx >= 0 && qy >= 0 && qx < width && qy < height && drop[qy * width + qx]) near = true
      }
    }
    if (near) out[p * 4 + 3] = 0
  }
  return { ...top, data: out }
}
