import type { Anime25DSourceReference, RasterLayer } from './anime25dImportTypes'
import { rasterBounds, trimRaster, uniquePartId } from './anime25dRaster'
import { alphaAt, closeMask, components } from './psdRepairMasks'

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

/** Over a body part, the portrait shows the part where its layer is this many times closer to it than the garment's… */
const SHOWN_RATIO = 3
/**
 * …and the garment is further off than art drawn where it belongs is
 * (~15-18 RGB): magenta lining on brown stockings measured 26-67.
 */
const SHOWN_MIN_DISTANCE = 24
/**
 * A piece found so goes on over the part where the garment is off by less (a
 * patch at 22-34 by the knee), and where the part is closer but not by
 * SHOWN_RATIO (a grey collar band over the neck's shadow: neck 12 off, band 22).
 */
const SHOWN_GROW_DISTANCE = 12
/** Closes pinholes where the garment's colour happens to come close to the part's. */
const SHOWN_CLOSING = 3
const OPAQUE_ALPHA = 128
/** The garment's anti-aliased rim around a piece goes with it, or lingers as a ghost outline. */
const SHOWN_RIM = 2
const SHOWN_RIM_ALPHA = 160

interface ShownPart {
  role: RasterLayer['role']
  garments: readonly RasterLayer['role'][]
  /** A piece is at least this share of the part drawn; specks of it stay. */
  minShare: number
}

/**
 * A full figure's coat or skirt drawn on across a leg the portrait shows in
 * front of it: the lining a long coat opens on, painted over a thigh. There
 * the leg's own layer has the portrait's colours and the garment's does not,
 * yet dark red on dark stockings is too close for repair's reveal to see.
 * Like a skirt's lining between the legs, that art goes to a layer of its own
 * just behind the legs: at rest the leg shows, and as the leg moves, the
 * lining it hung against is still there behind it.
 */
export function tuckGarmentOffLegsByReference(
  layers: RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
  visibleAtRest: (layer: RasterLayer) => boolean,
): RasterLayer[] {
  return tuckGarmentOffPart(layers, reference, visibleAtRest, { role: 'legwear', garments: ['topwear', 'bottomwear'], minShare: 0.005 })
}

/**
 * The same for a top drawn over the neck the portrait shows bare: the back
 * band of an open collar, which a full figure's tiles can leave in front of
 * the neck in grey (a stitch took the neck from one decomposition and the
 * top from another). Behind the neck it is the collar's back. A collar the
 * import split into front and back is left as split.
 */
export function tuckGarmentOffNeckByReference(
  layers: RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
  visibleAtRest: (layer: RasterLayer) => boolean,
): RasterLayer[] {
  if (layers.some((layer) => layer.role === 'collar-front' || layer.role === 'collar-back')) return layers
  return tuckGarmentOffPart(layers, reference, visibleAtRest, { role: 'neck', garments: ['topwear'], minShare: 0.02 })
}

function tuckGarmentOffPart(
  layers: RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
  visibleAtRest: (layer: RasterLayer) => boolean,
  part: ShownPart,
): RasterLayer[] {
  const parts = layers.filter((layer) => layer.role === part.role && visibleAtRest(layer))
  if (!parts.length) return layers
  const behind = Math.min(...parts.map((layer) => layers.indexOf(layer)))
  const garments = layers.filter((layer, index) =>
    part.garments.includes(layer.role) && index > behind && visibleAtRest(layer))
  if (!garments.length) return layers
  const { width: canvasWidth, height: canvasHeight, data: portrait } = reference
  // The topmost opaque layer at rest, per portrait pixel.
  const top = new Int16Array(canvasWidth * canvasHeight).fill(-1)
  let partArea = 0
  layers.forEach((layer, index) => {
    if (!visibleAtRest(layer)) return
    for (let row = Math.max(0, -layer.top); row < Math.min(layer.height, canvasHeight - layer.top); row += 1) {
      for (let col = Math.max(0, -layer.left); col < Math.min(layer.width, canvasWidth - layer.left); col += 1) {
        if (layer.data[(row * layer.width + col) * 4 + 3] < OPAQUE_ALPHA) continue
        top[(layer.top + row) * canvasWidth + layer.left + col] = index
        if (layer.role === part.role) partArea += 1
      }
    }
  })
  const distance = (data: Uint8ClampedArray, offset: number, at: number) => Math.hypot(
    data[offset] - portrait[at],
    data[offset + 1] - portrait[at + 1],
    data[offset + 2] - portrait[at + 2],
  )
  const linings: RasterLayer[] = []
  const fronts = new Map<RasterLayer, RasterLayer>()
  const ids = new Set(layers.map((layer) => layer.id))
  for (const garment of garments) {
    const index = layers.indexOf(garment)
    const { width, height } = garment
    // Where the garment shows over the part, and where the portrait has the
    // part: surely, or near enough to go with a sure piece.
    const over = new Uint8Array(width * height)
    const shown = new Uint8Array(width * height)
    const near = new Uint8Array(width * height)
    for (let row = Math.max(0, -garment.top); row < Math.min(height, canvasHeight - garment.top); row += 1) {
      for (let col = Math.max(0, -garment.left); col < Math.min(width, canvasWidth - garment.left); col += 1) {
        const x = garment.left + col
        const y = garment.top + row
        if (top[y * canvasWidth + x] !== index) continue
        const at = (y * canvasWidth + x) * 4
        if (portrait[at + 3] < 250) continue
        let under: RasterLayer | null = null
        for (let below = index - 1; below >= 0 && !under; below -= 1) {
          const layer = layers[below]
          if (layer.role === part.role && visibleAtRest(layer) && alphaAt(layer, x, y) >= OPAQUE_ALPHA) under = layer
        }
        if (!under) continue
        const target = row * width + col
        over[target] = 1
        const garmentOff = distance(garment.data, target * 4, at)
        const partOff = distance(under.data, ((y - under.top) * under.width + x - under.left) * 4, at)
        if (partOff * SHOWN_RATIO > garmentOff) {
          if (partOff < garmentOff && garmentOff > SHOWN_GROW_DISTANCE) near[target] = 1
          continue
        }
        if (garmentOff > SHOWN_GROW_DISTANCE) near[target] = 1
        if (garmentOff > SHOWN_MIN_DISTANCE) shown[target] = 1
      }
    }
    const keep = new Uint8Array(width * height)
    let kept = false
    for (const piece of components(closeMask(near, width, height, SHOWN_CLOSING), width, height)) {
      if (piece.filter((target) => shown[target]).length < partArea * part.minShare) continue
      const members = piece.filter((target) => over[target])
      for (const target of members) keep[target] = 1
      kept = true
    }
    if (!kept) continue
    for (let target = 0; target < keep.length; target += 1) {
      if (keep[target] !== 1) continue
      const col = target % width
      const row = Math.floor(target / width)
      for (let dy = -SHOWN_RIM; dy <= SHOWN_RIM; dy += 1) {
        for (let dx = -SHOWN_RIM; dx <= SHOWN_RIM; dx += 1) {
          const c = col + dx
          const r = row + dy
          if (c < 0 || r < 0 || c >= width || r >= height) continue
          const next = r * width + c
          const alpha = garment.data[next * 4 + 3]
          if (!keep[next] && alpha > 0 && alpha < SHOWN_RIM_ALPHA) keep[next] = 2
        }
      }
    }
    const front = { ...garment, data: new Uint8ClampedArray(garment.data) }
    const lining = { ...garment, data: new Uint8ClampedArray(garment.data.length) }
    for (let target = 0; target < keep.length; target += 1) {
      if (!keep[target]) continue
      lining.data.set(garment.data.subarray(target * 4, target * 4 + 4), target * 4)
      front.data[target * 4 + 3] = 0
    }
    lining.id = uniquePartId(`${garment.id}-lining`, ids)
    linings.push(trimRaster(lining))
    fronts.set(garment, front)
  }
  if (!linings.length) return layers
  const output: RasterLayer[] = []
  layers.forEach((layer, index) => {
    if (index === behind) output.push(...linings)
    const front = fronts.get(layer)
    if (!front) output.push(layer)
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
