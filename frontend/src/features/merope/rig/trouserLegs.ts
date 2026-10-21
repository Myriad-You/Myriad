import type { RasterLayer } from './anime25dImportTypes'
import { trimRaster, uniquePartId } from './anime25dRaster'

/** Alpha above which a pixel is drawn. */
const DRAWN_ALPHA = 16
/** Long trousers: open between the legs over at least this share of their height. */
const TROUSER_LEGS_SHARE = 0.35
/** The seat fades out over the legs over this share of the trousers' height. */
const TROUSER_SEAT_OVERLAP = 0.06
/** At the trousers' sides the legs start this share of the way down to the crotch: by the hips they turn about. */
const TROUSER_HIP_DEPTH = 0.3
/** Half the width of the column the legs' gap is looked for in. */
const TROUSER_GAP_HALF_WIDTH = 2

/**
 * A figure in long trousers can arrive with them as one lower garment down to
 * the shoes and no legs: then nothing of it walks, and the shoes step out
 * from under trousers that stay. Below a line from the crotch (where the legs
 * part) up to the hips at the sides, the trousers are the legs; the seat stays
 * the lower garment, fading out over the legs' tops so a moving leg shows no
 * edge. A skirt, closed between the legs, is left as it is.
 */
export function splitTrouserLegsIfNeeded(layers: RasterLayer[]): RasterLayer[] {
  if (layers.some((layer) => layer.role === 'legwear')) return layers
  const trousers = layers.find((layer) => layer.role === 'bottomwear' && !layer.side)
  const shoes = layers.filter((layer) => layer.role === 'footwear')
  if (!trousers || shoes.length === 0) return layers
  if (trousers.top + trousers.height < Math.min(...shoes.map((shoe) => shoe.top))) return layers
  const middle = alphaMiddleX(trousers)
  if (middle === null) return layers
  const column = Math.round(middle - trousers.left)
  const open = (y: number) => {
    for (let x = column - TROUSER_GAP_HALF_WIDTH; x <= column + TROUSER_GAP_HALF_WIDTH; x += 1) {
      if (x < 0 || x >= trousers.width) continue
      if (trousers.data[(y * trousers.width + x) * 4 + 3] > DRAWN_ALPHA) return false
    }
    return true
  }
  let crotch = trousers.height
  while (crotch > 0 && open(crotch - 1)) crotch -= 1
  if (trousers.height - crotch < TROUSER_LEGS_SHARE * trousers.height) return layers
  const overlap = Math.max(1, Math.round(TROUSER_SEAT_OVERLAP * trousers.height))
  const half = Math.max(column, trousers.width - column, 1)
  // Where the legs start in each column: the crotch in the middle, the hips at the sides.
  const cut = (x: number) => crotch - crotch * (1 - TROUSER_HIP_DEPTH) * Math.min(1, Math.abs(x - column) / half)
  const legsData = new Uint8ClampedArray(trousers.data)
  const seatData = new Uint8ClampedArray(trousers.data)
  for (let y = 0; y < trousers.height; y += 1) {
    for (let x = 0; x < trousers.width; x += 1) {
      const alpha = (y * trousers.width + x) * 4 + 3
      const below = y - cut(x)
      if (below < -overlap) legsData[alpha] = 0
      if (below > 0) seatData[alpha] = Math.round(seatData[alpha] * Math.max(0, 1 - below / overlap))
    }
  }
  const usedIds = new Set(layers.map((layer) => layer.id))
  const legs: RasterLayer = {
    ...trimRaster({ ...trousers, data: legsData }),
    id: uniquePartId('legwear', usedIds),
    role: 'legwear',
    sourceName: 'legwear',
  }
  const seat = trimRaster({ ...trousers, data: seatData })
  const at = layers.indexOf(trousers)
  return layers.toSpliced(at, 1, legs, seat)
}

/** The alpha-weighted middle column of a layer, in canvas pixels. */
function alphaMiddleX(layer: RasterLayer): number | null {
  let weight = 0
  let sum = 0
  for (let y = 0; y < layer.height; y += 1) {
    for (let x = 0; x < layer.width; x += 1) {
      const alpha = layer.data[(y * layer.width + x) * 4 + 3]
      if (alpha <= DRAWN_ALPHA) continue
      weight += alpha
      sum += alpha * x
    }
  }
  return weight > 0 ? layer.left + sum / weight : null
}
