import type { RasterLayer } from './anime25dImportTypes'
import { rasterBounds, trimRaster, uniquePartId } from './anime25dRaster'

const ALPHA_COMPONENT_THRESHOLD = 16

/**
 * A piece of a skirt seen through the gap between the legs (its lining, a
 * tab of the underskirt) is at least this tall, as a share of the legs'
 * height; the fringe of a pleat tip there is shorter and stays in front.
 */
const LINING_MIN_DROP = 0.01
/** It may lie over this share of a leg's width (the thigh's edge it hangs against). */
const LINING_LEG_OVERLAP = 0.25

/**
 * What a figure's skirt shows between its legs below the hem is behind them,
 * but the decomposition draws it on the skirt's layer, in front. At rest the
 * legs leave that gap open and nothing differs; once the legs move across it
 * (a turn), it would hang over a thigh. A skirt pixel whose row of skirt stays
 * inside the gap (never reaching a leg) belongs to such a piece; pieces tall
 * enough go to a layer of their own just behind the legs.
 */
export function tuckLiningBehindLegs(layers: RasterLayer[]): RasterLayer[] {
  const skirt = layers.find((layer) => layer.role === 'bottomwear' && !layer.side)
  const legL = layers.find((layer) => layer.role === 'legwear' && layer.side === 'left')
  const legR = layers.find((layer) => layer.role === 'legwear' && layer.side === 'right')
  if (!skirt || !legL || !legR) return layers
  const boundsL = canvasBounds(legL)
  const boundsR = canvasBounds(legR)
  if (!boundsL || !boundsR) return layers
  const legHeight = Math.max(boundsL.bottom, boundsR.bottom) - Math.min(boundsL.top, boundsR.top)
  const alpha = (layer: RasterLayer, x: number, y: number): number => {
    const lx = x - layer.left
    const ly = y - layer.top
    if (lx < 0 || ly < 0 || lx >= layer.width || ly >= layer.height) return 0
    return layer.data[(ly * layer.width + lx) * 4 + 3]
  }
  const { width, height } = skirt
  const drawn = (col: number, row: number) => skirt.data[(row * width + col) * 4 + 3] > ALPHA_COMPONENT_THRESHOLD
  // Per row, each run of skirt that stays between the legs: it may lie over
  // a thigh's edge, but the skirt's front reaches well across a leg.
  const inside = new Uint8Array(width * height)
  for (let row = 0; row < height; row += 1) {
    const y = skirt.top + row
    let gapLeft = -Infinity
    for (let x = boundsL.right - 1; x >= boundsL.left; x -= 1) {
      if (alpha(legL, x, y) > ALPHA_COMPONENT_THRESHOLD) { gapLeft = x; break }
    }
    let gapRight = Infinity
    for (let x = boundsR.left; x < boundsR.right; x += 1) {
      if (alpha(legR, x, y) > ALPHA_COMPONENT_THRESHOLD) { gapRight = x; break }
    }
    if (!Number.isFinite(gapLeft) || !Number.isFinite(gapRight)) continue
    let outerL = gapLeft
    for (let x = boundsL.left; x < gapLeft; x += 1) {
      if (alpha(legL, x, y) > ALPHA_COMPONENT_THRESHOLD) { outerL = x; break }
    }
    let outerR = gapRight
    for (let x = boundsR.right - 1; x > gapRight; x -= 1) {
      if (alpha(legR, x, y) > ALPHA_COMPONENT_THRESHOLD) { outerR = x; break }
    }
    const reachL = gapLeft - (gapLeft - outerL) * LINING_LEG_OVERLAP
    const reachR = gapRight + (outerR - gapRight) * LINING_LEG_OVERLAP
    for (let col = 0; col < width;) {
      if (!drawn(col, row)) { col += 1; continue }
      const start = col
      while (col < width && drawn(col, row)) col += 1
      const x0 = skirt.left + start
      const x1 = skirt.left + col - 1
      // A run between the legs, not merely beside them.
      if (x0 >= reachL && x1 <= reachR && x1 > gapLeft && x0 < gapRight) inside.fill(1, row * width + start, row * width + col)
    }
  }
  // Tall enough pieces of those runs, joined across rows.
  const piece = new Int32Array(width * height).fill(-1)
  const keep = new Uint8Array(width * height)
  const stack: number[] = []
  let kept = false
  for (let seed = 0; seed < inside.length; seed += 1) {
    if (!inside[seed] || piece[seed] >= 0) continue
    const members: number[] = []
    piece[seed] = seed
    stack.push(seed)
    let top = height
    let bottom = -1
    while (stack.length) {
      const at = stack.pop()!
      members.push(at)
      const row = Math.floor(at / width)
      const col = at - row * width
      top = Math.min(top, row)
      bottom = Math.max(bottom, row)
      for (const next of [col > 0 ? at - 1 : -1, col < width - 1 ? at + 1 : -1, at - width, at + width]) {
        if (next >= 0 && next < inside.length && inside[next] && piece[next] < 0) {
          piece[next] = seed
          stack.push(next)
        }
      }
    }
    if (bottom - top + 1 >= legHeight * LINING_MIN_DROP) {
      for (const at of members) keep[at] = 1
      kept = true
    }
  }
  if (!kept) return layers
  const front = { ...skirt, data: new Uint8ClampedArray(skirt.data) }
  const lining = { ...skirt, data: new Uint8ClampedArray(skirt.data.length) }
  for (let at = 0; at < keep.length; at += 1) {
    if (!keep[at]) continue
    lining.data.set(skirt.data.subarray(at * 4, at * 4 + 4), at * 4)
    front.data[at * 4 + 3] = 0
  }
  lining.id = uniquePartId(`${skirt.id}-lining`, new Set(layers.map((layer) => layer.id)))
  const output: RasterLayer[] = []
  const behind = Math.min(layers.indexOf(legL), layers.indexOf(legR))
  layers.forEach((layer, index) => {
    if (index === behind) output.push(trimRaster(lining))
    if (layer !== skirt) output.push(layer)
    else if (rasterBounds(front)) output.push(front)
  })
  return output
}

function canvasBounds(layer: RasterLayer): { left: number; top: number; right: number; bottom: number } | null {
  const bounds = rasterBounds(layer)
  return bounds && {
    left: layer.left + bounds.x,
    top: layer.top + bounds.y,
    right: layer.left + bounds.x + bounds.width,
    bottom: layer.top + bounds.y + bounds.height,
  }
}
