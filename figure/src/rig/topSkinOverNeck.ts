import type { Anime25DSourceReference, RasterLayer } from './anime25dImportTypes'
import { rasterBounds, trimRaster, uniquePartId } from './anime25dRaster'
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
 * Behind the neck the skin reaches this far under what of the top stays:
 * two edges meeting exactly each draw half a pixel there, and a hairline of
 * what is behind shows through along the seam.
 */
const UNDERLAP = 2
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
 * as the head turns the neck moves under a skin that stays.
 *
 * With the portrait, that skin and the rim round it take the portrait's
 * colours, and so does the neck from as high as that skin reached: at rest the throat is the portrait's, no stair, and the skin stays
 * over the neck's foot, which a decomposition often cuts square inside the
 * chest and guesses paler than it. Moving the skin behind the neck instead
 * leaves that paler box to show as the neck turns.
 *
 * Without it the neck is the skin: where the top over it is skin, that goes
 * behind the neck, and the translucent rim around it goes. Behind, it is what
 * shows where the neck moves off it, or the turning neck opens a frame of
 * nothing round its cut.
 *
 * A collar or any garment over the neck, not the skin's colour, stays.
 */
export function dropTopSkinOverNeck(
  layers: RasterLayer[],
  visibleAtRest: (layer: RasterLayer) => boolean,
  reference?: Readonly<Anime25DSourceReference>,
): RasterLayer[] {
  const necks = layers.flatMap((layer, index) => (layer.role === 'neck' && visibleAtRest(layer) ? [index] : []))
  if (necks.length === 0) return layers
  const out = layers.slice()
  const linings: RasterLayer[] = []
  for (const [index, top] of layers.entries()) {
    if (top.role !== 'topwear' || !visibleAtRest(top)) continue
    const under = necks.filter((neck) => neck < index).map((neck) => layers[neck])
    if (under.length === 0) continue
    const split = dropOver(top, under)
    if (!split) continue
    out[index] = reference ? paintPortrait(top, split, reference) : split.front
    linings.push(split.skin)
  }
  if (linings.length === 0) return out
  if (reference) {
    for (const neck of necks) out[neck] = takePortraitSkin(layers[neck], linings, reference)
    return out
  }
  const ids = new Set(layers.map((layer) => layer.id))
  const named = linings.filter(rasterBounds).map((skin) => trimRaster({ ...skin, id: uniquePartId(`${skin.id}-lining`, ids) }))
  return out.toSpliced(necks[0], 0, ...named)
}

/**
 * The top with its skin over the neck, and the rim round it, in the colours
 * the portrait draws there and their own alpha (a rim that fades out stays
 * soft, or the neck turning under it meets a hard edge); where the portrait
 * shows something else there (the edge of a choker over the throat), as the
 * top drew it.
 */
function paintPortrait(top: RasterLayer, split: { front: RasterLayer; skin: RasterLayer }, reference: Readonly<Anime25DSourceReference>): RasterLayer {
  const data = new Uint8ClampedArray(top.data)
  for (let p = 0; p < top.width * top.height; p += 1) {
    const at = p * 4
    if (split.front.data[at + 3] > 0 || top.data[at + 3] === 0 || split.skin.data[at + 3] === 0) continue
    const x = top.left + (p % top.width)
    const y = top.top + Math.floor(p / top.width)
    if (x < 0 || y < 0 || x >= reference.width || y >= reference.height) continue
    const r = (y * reference.width + x) * 4
    if (!sameSkin(reference.data, r, split.skin.data, at)) continue
    data.set(reference.data.subarray(r, r + 3), at)
  }
  return { ...top, data }
}

/** The neck's skin as the portrait draws it, from the top of `skins` down. */
function takePortraitSkin(neck: RasterLayer, skins: RasterLayer[], reference: Readonly<Anime25DSourceReference>): RasterLayer {
  const from = Math.min(...skins.map((skin) => skin.top + (rasterBounds(skin)?.y ?? skin.height)))
  let data: Uint8ClampedArray | null = null
  for (let y = Math.max(0, from - neck.top); y < neck.height; y += 1) {
    const ry = neck.top + y
    if (ry < 0 || ry >= reference.height) continue
    for (let x = 0; x < neck.width; x += 1) {
      const rx = neck.left + x
      const at = (y * neck.width + x) * 4
      if (rx < 0 || rx >= reference.width || neck.data[at + 3] < NECK_ALPHA) continue
      const r = (ry * reference.width + rx) * 4
      if (!sameSkin(reference.data, r, neck.data, at)) continue
      data ??= new Uint8ClampedArray(neck.data)
      data.set(reference.data.subarray(r, r + 3), at)
    }
  }
  return data ? { ...neck, data } : neck
}

/** Whether the colour at `a[i]` is skin like that at `b[j]` (SKIN_NEAR, HUE_NEAR). */
function sameSkin(a: Readonly<Uint8ClampedArray>, i: number, b: Readonly<Uint8ClampedArray>, j: number): boolean {
  if (Math.hypot(a[i] - b[j], a[i + 1] - b[j + 1], a[i + 2] - b[j + 2]) >= SKIN_NEAR) return false
  return Math.hypot(a[i] - a[i + 1] - (b[j] - b[j + 1]), a[i + 1] - a[i + 2] - (b[j + 1] - b[j + 2])) < HUE_NEAR
}

/** The top without its skin over the necks, and that skin; null when it draws none. */
function dropOver(top: RasterLayer, necks: RasterLayer[]): { front: RasterLayer; skin: RasterLayer } | null {
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
    if (!sameSkin(data, p * 4, neck, at)) continue
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
  if (dropped === 0) return null
  const out = new Uint8ClampedArray(data)
  // Behind the neck the skin is whole: a translucent wash there, or a gap
  // where its rim went, lets what is behind show through as the neck moves.
  const skinData = new Uint8ClampedArray(data.length)
  for (let p = 0; p < width * height; p += 1) {
    if (drop[p]) {
      skinData.set(data.subarray(p * 4, p * 4 + 3), p * 4)
      skinData[p * 4 + 3] = 255
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
    if (!near) continue
    out[p * 4 + 3] = 0
    // The rim was a guessed line, not skin: behind, the neck's own skin stands in for it.
    const neck = necks[neckOf[p]].data
    skinData.set(neck.subarray(neckAt[p] * 4, neckAt[p] * 4 + 3), p * 4)
    skinData[p * 4 + 3] = 255
  }
  underlap(skinData, data, width, height)
  return { front: { ...top, data: out }, skin: { ...top, data: skinData } }
}

/** Grows the skin UNDERLAP pixels into the top it was taken from, as opaque. */
function underlap(skin: Uint8ClampedArray, top: Readonly<Uint8ClampedArray>, width: number, height: number): void {
  const had = new Uint8Array(width * height)
  for (let p = 0; p < had.length; p += 1) had[p] = skin[p * 4 + 3] > 0 ? 1 : 0
  for (let p = 0; p < had.length; p += 1) {
    if (had[p] || top[p * 4 + 3] === 0) continue
    const x = p % width
    const y = (p - x) / width
    let near = false
    for (let dy = -UNDERLAP; dy <= UNDERLAP && !near; dy += 1) {
      for (let dx = -UNDERLAP; dx <= UNDERLAP && !near; dx += 1) {
        const qx = x + dx
        const qy = y + dy
        if (qx >= 0 && qy >= 0 && qx < width && qy < height && had[qy * width + qx]) near = true
      }
    }
    if (!near) continue
    skin.set(top.subarray(p * 4, p * 4 + 3), p * 4)
    skin[p * 4 + 3] = 255
  }
}
