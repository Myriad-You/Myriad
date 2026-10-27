import type { EyeSide, RasterLayer } from './anime25dImportTypes'

/** The seam keeps within this share of the layer's width either side of the middle. */
const SEAM_REACH = 0.15
/** What a column's distance from the middle costs, against an opaque pixel's 1, at SEAM_REACH. */
const SEAM_OFF_MIDDLE = 0.05
/** What moving a column costs: through where the two pieces touch the cut goes straight, not in a wedge. */
const SEAM_STEP = 0.02

/**
 * Keeps the part of a layer on one side of a seam running down it near a
 * canvas column. A straight cut at the legs' middle slices whichever shoe
 * reaches across it, and the sliver rides the other leg: as the two move
 * apart a line opens down that shoe's edge. The seam, one column a row and
 * moving at most one column from row to row, goes through as few opaque
 * pixels as it can (seam carving), so it finds the gap between two shoes
 * touching only at their soles; through one solid piece it stays straight
 * down the middle.
 */
export function splitRasterAlongSeam(
  source: RasterLayer,
  canvasX: number,
  side: EyeSide,
): RasterLayer {
  const { width, height } = source
  const middle = canvasX - source.left - 0.5
  const reach = Math.max(2, Math.round(width * SEAM_REACH))
  const low = Math.max(0, Math.min(width - 1, Math.round(middle) - reach))
  const high = Math.max(low, Math.min(width - 1, Math.round(middle) + reach))
  const band = high - low + 1
  const cost = new Float32Array(band * height)
  const from = new Int8Array(band * height)
  for (let y = 0; y < height; y += 1) {
    for (let c = 0; c < band; c += 1) {
      const x = low + c
      const own = source.data[(y * width + x) * 4 + 3] / 255 + (SEAM_OFF_MIDDLE * Math.abs(x - middle)) / reach
      let best = 0
      let step = 0
      if (y > 0) {
        best = Infinity
        for (let d = -1; d <= 1; d += 1) {
          const previous = c + d
          if (previous < 0 || previous >= band) continue
          const value = cost[(y - 1) * band + previous] + (d === 0 ? 0 : SEAM_STEP)
          if (value < best) {
            best = value
            step = d
          }
        }
      }
      cost[y * band + c] = own + best
      from[y * band + c] = step
    }
  }
  // Back up from the cheapest end: column `seam[y]` and what lies right of it go right.
  const seam = new Int32Array(height)
  let c = 0
  for (let k = 1; k < band; k += 1) {
    if (cost[(height - 1) * band + k] < cost[(height - 1) * band + c]) c = k
  }
  for (let y = height - 1; y >= 0; y -= 1) {
    seam[y] = low + c
    c += from[y * band + c]
  }
  const output = { ...source, data: new Uint8ClampedArray(source.data) }
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      if ((x < seam[y]) !== (side === 'left')) {
        output.data[(y * width + x) * 4 + 3] = 0
      }
    }
  }
  return output
}
