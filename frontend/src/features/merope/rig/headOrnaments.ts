import type { RasterLayer } from './anime25dImportTypes'
import { components } from './psdRepairMasks'

/** Drawn: a pixel of the art. */
const DRAWN = 16
/** Over the piece: an opaque pixel of a layer it would move past. */
const OPAQUE = 128
/** A piece this many pixels or more; smaller ones are not worth a guess. */
const MIN_PIECE = 150
/** Hanging from the accessory: within this many pixels of it. */
const TOUCH = 4

/**
 * A ribbon tied to a hair clip can come out of a decomposition in the top:
 * a piece of the top's layer apart from the garment, hanging from the clip.
 * The clip rides the head and the top the body, so as the head turns the
 * ribbon stays behind, and the crumbs of it the clip and the hair kept hang
 * in the air beside it. Such a piece goes to the accessory it hangs from,
 * where nothing between the two layers is drawn over it: at rest the picture
 * is the same, and the ribbon goes where the clip goes.
 */
export function hangTopPiecesOnHeadwear(layers: RasterLayer[], visibleAtRest: (layer: RasterLayer) => boolean): RasterLayer[] {
  const clips = layers.flatMap((layer, index) => (layer.role === 'headwear' && visibleAtRest(layer) ? [index] : []))
  if (clips.length === 0) return layers
  const out = layers.slice()
  for (const [index, top] of layers.entries()) {
    if (top.role !== 'topwear' || !visibleAtRest(top)) continue
    const drawn = new Uint8Array(top.width * top.height)
    for (let p = 0; p < drawn.length; p += 1) drawn[p] = top.data[p * 4 + 3] > DRAWN ? 1 : 0
    const pieces = components(drawn, top.width, top.height).sort((a, b) => b.length - a.length)
    // The largest piece is the garment.
    for (const piece of pieces.slice(1)) {
      if (piece.length < MIN_PIECE) break
      const clip = clips.find((c) => hangsFrom(out[index], piece, out[c]) && clearBetween(out, index, c, out[index], piece, visibleAtRest))
      if (clip === undefined) continue
      const [topAfter, clipAfter] = move(out[index], piece, out[clip], clip > index)
      out[index] = topAfter
      out[clip] = clipAfter
    }
  }
  return out
}

function alphaAt(layer: RasterLayer, x: number, y: number): number {
  const col = x - layer.left
  const row = y - layer.top
  if (col < 0 || row < 0 || col >= layer.width || row >= layer.height) return 0
  return layer.data[(row * layer.width + col) * 4 + 3]
}

function hangsFrom(top: RasterLayer, piece: readonly number[], clip: RasterLayer): boolean {
  for (const p of piece) {
    const x = (p % top.width) + top.left
    const y = Math.floor(p / top.width) + top.top
    for (let dy = -TOUCH; dy <= TOUCH; dy += 1) {
      for (let dx = -TOUCH; dx <= TOUCH; dx += 1) {
        if (alphaAt(clip, x + dx, y + dy) > DRAWN) return true
      }
    }
  }
  return false
}

/** Whether no layer the piece would move past draws over it at rest. */
function clearBetween(layers: RasterLayer[], from: number, to: number, top: RasterLayer, piece: readonly number[], visibleAtRest: (layer: RasterLayer) => boolean): boolean {
  const between = layers.slice(Math.min(from, to) + 1, Math.max(from, to)).filter(visibleAtRest)
  return piece.every((p) => {
    const x = (p % top.width) + top.left
    const y = Math.floor(p / top.width) + top.top
    return between.every((layer) => alphaAt(layer, x, y) < OPAQUE)
  })
}

/** The top without the piece, and the clip with it drawn in (under the clip when the clip lies above). */
function move(top: RasterLayer, piece: readonly number[], clip: RasterLayer, clipAbove: boolean): [RasterLayer, RasterLayer] {
  let left = clip.left
  let upper = clip.top
  let right = clip.left + clip.width
  let bottom = clip.top + clip.height
  for (const p of piece) {
    const x = (p % top.width) + top.left
    const y = Math.floor(p / top.width) + top.top
    left = Math.min(left, x)
    upper = Math.min(upper, y)
    right = Math.max(right, x + 1)
    bottom = Math.max(bottom, y + 1)
  }
  const width = right - left
  const data = new Uint8ClampedArray(width * (bottom - upper) * 4)
  for (let row = 0; row < clip.height; row += 1) {
    const start = row * clip.width * 4
    data.set(clip.data.subarray(start, start + clip.width * 4), ((row + clip.top - upper) * width + clip.left - left) * 4)
  }
  const topData = new Uint8ClampedArray(top.data)
  for (const p of piece) {
    const x = (p % top.width) + top.left
    const y = Math.floor(p / top.width) + top.top
    const at = ((y - upper) * width + x - left) * 4
    const over = clipAbove ? data.slice(at, at + 4) : top.data.slice(p * 4, p * 4 + 4)
    const under = clipAbove ? top.data.slice(p * 4, p * 4 + 4) : data.slice(at, at + 4)
    data.set(composite(over, under), at)
    topData[p * 4 + 3] = 0
  }
  return [{ ...top, data: topData }, { ...clip, left, top: upper, width, height: bottom - upper, data }]
}

/** `over` drawn on `under`, straight alpha. */
function composite(over: Uint8ClampedArray, under: Uint8ClampedArray): number[] {
  const a = over[3] / 255
  const b = under[3] / 255
  const alpha = a + b * (1 - a)
  if (alpha === 0) return [0, 0, 0, 0]
  return [0, 1, 2].map((c) => Math.round((over[c] * a + under[c] * b * (1 - a)) / alpha)).concat(Math.round(alpha * 255))
}
