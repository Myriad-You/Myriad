import type { RasterLayer } from './anime25dImportTypes'
import type { ContentBounds } from './psdReconciliation'

/**
 * What hangs from an ear is thin (a bar or chain a few pixels wide, cut into
 * pieces by the strands in front of it): a region on an earring's drawing, or
 * below an ear where an earring hangs, is held to the bare pixel floor (regionFloor), not
 * the share of the portrait that keeps specks elsewhere out. A sliver along
 * the ear's own outline is neither.
 */
const EAR_ACCESSORY_REACH = 3
const EAR_ROLE = /^ears(-|$)/
const EARWEAR_ROLE = /^earwear(-|$)/

/** Pixels in bounds, each within EAR_ACCESSORY_REACH and whether covered or not. */
export interface EarAccessoryMasks {
  /** On an earring's drawing. */
  onEarring: Uint8Array
  /** Below an ear's lowest pixel in the same column, down as far as the ear is tall. */
  belowEar: Uint8Array
}

export function earAccessoryMasks(layers: readonly RasterLayer[], bounds: ContentBounds): EarAccessoryMasks {
  const { x0, y0, width, height } = bounds
  const masks = {
    onEarring: new Uint8Array(width * height),
    belowEar: new Uint8Array(width * height),
  }
  const reach = EAR_ACCESSORY_REACH
  const mark = (mask: Uint8Array, x: number, y: number) => {
    for (let row = Math.max(0, y - reach); row <= Math.min(height - 1, y + reach); row += 1) {
      for (let column = Math.max(0, x - reach); column <= Math.min(width - 1, x + reach); column += 1) {
        mask[row * width + column] = 1
      }
    }
  }
  for (const layer of layers) {
    const earring = EARWEAR_ROLE.test(layer.role)
    if (!earring && !EAR_ROLE.test(layer.role)) continue
    for (let localX = 0; localX < layer.width; localX += 1) {
      let lowest = -1
      for (let localY = 0; localY < layer.height; localY += 1) {
        if (layer.data[(localY * layer.width + localX) * 4 + 3] < 128) continue
        if (earring) mark(masks.onEarring, layer.left + localX - x0, layer.top + localY - y0)
        lowest = localY
      }
      if (earring || lowest < 0) continue
      const x = layer.left + localX - x0
      const bottom = layer.top + lowest - y0
      for (let row = bottom + reach + 1; row <= Math.min(height - 1, bottom + layer.height); row += 1) {
        for (let column = Math.max(0, x - reach); column <= Math.min(width - 1, x + reach); column += 1) {
          if (row >= 0) masks.belowEar[row * width + column] = 1
        }
      }
    }
  }
  return masks
}
