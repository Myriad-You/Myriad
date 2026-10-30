import type { Anime25DSourceReference, RasterLayer } from './anime25dImportTypes'
import type { Anime25DPsdAnalysis } from './psdReconciliation'
import type { RecoveredPiece } from './psdRepairLayers'
import { MATCH_DISTANCE, rgbDistance } from './psdReconciliation'
import { closeMask, fringe } from './psdRepairMasks'

type GarmentRole = 'bottomwear' | 'legwear'

/**
 * What a garment hides where it was dropped: drawn there by the
 * decomposition (it paints what lies behind a garment too), shown in its
 * place. Back hair falls behind a skirt; legs and shoes go under it.
 */
const HIDDEN_BY: Record<GarmentRole, ReadonlySet<string>> = {
  bottomwear: new Set(['back-hair', 'legwear', 'footwear']),
  legwear: new Set(['back-hair']),
}

/**
 * A garment-sized part of the portrait no layer drew, below the head: the
 * garment it is, or null when no garment explains it. Where it touches the
 * lower garment or the legs and its colours carry on across the seam, it is
 * the rest of that garment, drawn only in part, and joins that layer (legs
 * hanging from a skirt touch it too, but change colour there). Otherwise,
 * below the top's middle it is the lower garment, below the lower garment the
 * legs'. Most of it must lie over nothing or over what that garment hides, so
 * a print the top missed stays a reported mismatch.
 */
export function droppedGarment(
  analysis: Readonly<Anime25DPsdAnalysis>,
  shown: readonly number[],
  visible: readonly RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): Pick<RecoveredPiece, 'group' | 'neighbour' | 'owner' | 'role'> | null {
  const { bounds } = analysis
  const top = spanOf(visible, 'topwear')
  if (!top) return null
  const middle =
    bounds.y0 + shown.reduce((sum, target) => sum + Math.floor(target / bounds.width), 0) / shown.length
  if (middle < (top.top + top.bottom) / 2) return null
  const continued = continuedLayers(analysis, shown, visible, reference)
  const lower = spanOf(visible, 'bottomwear')
  const role: GarmentRole = continued.bottomwear >= 0
    ? 'bottomwear'
    : continued.legwear >= 0 || (lower && middle > lower.bottom)
      ? 'legwear'
      : 'bottomwear'
  const hidden = HIDDEN_BY[role]
  const over = shown.filter((target) => {
    const index = analysis.top[target]
    return index < 0 || hidden.has(visible[index].role)
  }).length
  if (over * 2 < shown.length) return null
  const owner = continued[role]
  return {
    group: 'body',
    neighbour: owner >= 0 ? owner : visible.findLastIndex((layer) => layer.role === 'topwear'),
    owner,
    role,
  }
}

/** How far across the seam a dropped part is compared with the layer it may continue. */
const GARMENT_TOUCH = 3

/** What the outfit is made of: a dropped piece touching one of these may be the rest of it. */
const WORN = new Set(['topwear', 'bottomwear', 'legwear', 'handwear', 'footwear'])

/**
 * The garment layer a dropped piece carries on: of the layers it touches
 * whose roles `of` accepts, the one it touches most along its rim where the
 * portrait's colours either side of the seam match on average. -1 for none.
 */
function carriedOn(
  analysis: Readonly<Anime25DPsdAnalysis>,
  shown: readonly number[],
  visible: readonly RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
  of: (role: string) => boolean,
): number {
  const { bounds } = analysis
  const pixel = (target: number) => {
    const x = bounds.x0 + (target % bounds.width)
    const y = bounds.y0 + Math.floor(target / bounds.width)
    return (y * reference.width + x) * 4
  }
  const inside = new Set(shown)
  const seams = new Map<number, { pairs: number; distance: number }>()
  for (const [target, member] of fringe(shown, bounds.width, bounds.height, GARMENT_TOUCH)) {
    const index = analysis.top[target]
    if (inside.has(target) || index < 0 || !of(visible[index].role)) continue
    const from = pixel(member)
    const color = [reference.data[from], reference.data[from + 1], reference.data[from + 2]] as const
    const seam = seams.get(index) ?? { pairs: 0, distance: 0 }
    seam.pairs += 1
    seam.distance += rgbDistance(reference.data, pixel(target), color)
    seams.set(index, seam)
  }
  let best = -1
  let pairs = 0
  for (const [index, seam] of seams) {
    if (seam.distance / seam.pairs > MATCH_DISTANCE || seam.pairs <= pairs) continue
    best = index
    pairs = seam.pairs
  }
  return best
}

/** Of each garment kind, the layer the part carries on (carriedOn), or -1. */
function continuedLayers(
  analysis: Readonly<Anime25DPsdAnalysis>,
  shown: readonly number[],
  visible: readonly RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): Record<GarmentRole, number> {
  return {
    bottomwear: carriedOn(analysis, shown, visible, reference, (role) => role === 'bottomwear'),
    legwear: carriedOn(analysis, shown, visible, reference, (role) => role === 'legwear'),
  }
}

/**
 * A small piece no layer drew that carries on a garment it touches: its
 * frayed edge where a decomposition stopped short (notches along a hakama
 * the importer had lifted as loose objects that moved on their own). It
 * joins that garment; null when it carries on none.
 */
export function garmentEdge(
  analysis: Readonly<Anime25DPsdAnalysis>,
  shown: readonly number[],
  visible: readonly RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): Pick<RecoveredPiece, 'group' | 'neighbour' | 'owner' | 'role'> | null {
  const owner = carriedOn(analysis, shown, visible, reference, (role) => WORN.has(role))
  if (owner < 0) return null
  return { group: visible[owner].group, neighbour: owner, owner, role: visible[owner].role }
}

function spanOf(visible: readonly RasterLayer[], role: string): { top: number; bottom: number } | null {
  const layers = visible.filter((layer) => layer.role === role)
  if (layers.length === 0) return null
  return {
    top: Math.min(...layers.map((layer) => layer.top)),
    bottom: Math.max(...layers.map((layer) => layer.top + layer.height)),
  }
}

/**
 * Hair hanging over a lifted garment leaves strand-shaped gaps in it, up to
 * twice this across. A strand of a layer drawn under the garment (the back
 * hair) shows through its gap only at rest: once either moves, the gap shows
 * the backdrop. So the garment takes what the portrait has there, the strand
 * drawn on it. Under a layer drawn over the garment (a front lock hanging
 * down) the gap is the garment's own hidden part, filled with its colours, so
 * the lock can swing off it.
 */
const GARMENT_GAP = 12

/**
 * Closes the gaps hair leaves in a lifted garment, adding to `pixels` (keyed
 * by canvas index). `under` is the last of `visible` drawn under the garment.
 */
export function closeGarmentGaps(
  analysis: Readonly<Anime25DPsdAnalysis>,
  shown: readonly number[],
  under: number,
  reference: Readonly<Anime25DSourceReference>,
  pixels: Map<number, readonly [number, number, number, number]>,
): void {
  const { bounds } = analysis
  const mask = new Uint8Array(bounds.width * bounds.height)
  for (const target of shown) mask[target] = 1
  const closed = closeMask(mask, bounds.width, bounds.height, GARMENT_GAP)
  const hidden: number[] = []
  for (let target = 0; target < closed.length; target += 1) {
    if (!closed[target] || mask[target]) continue
    const x = bounds.x0 + (target % bounds.width)
    const y = bounds.y0 + Math.floor(target / bounds.width)
    const offset = (y * reference.width + x) * 4
    if (reference.data[offset + 3] < 250) continue
    const key = y * reference.width + x
    if (analysis.top[target] <= under) {
      pixels.set(key, [reference.data[offset], reference.data[offset + 1], reference.data[offset + 2], 255])
    } else {
      hidden.push(key)
    }
  }
  fillFromNeighbours(hidden, reference.width, pixels)
}

/** Grows the known colours into `keys`, a ring at a time, each the mean of its known neighbours. */
function fillFromNeighbours(
  keys: readonly number[],
  width: number,
  pixels: Map<number, readonly [number, number, number, number]>,
): void {
  let left = keys.filter((key) => !pixels.has(key))
  while (left.length > 0) {
    const ring: Array<[number, readonly [number, number, number, number]]> = []
    const rest: number[] = []
    for (const key of left) {
      let red = 0
      let green = 0
      let blue = 0
      let count = 0
      for (const next of [key - 1, key + 1, key - width, key + width]) {
        const known = pixels.get(next)
        if (!known) continue
        red += known[0]
        green += known[1]
        blue += known[2]
        count += 1
      }
      if (count === 0) rest.push(key)
      else ring.push([key, [Math.round(red / count), Math.round(green / count), Math.round(blue / count), 255]])
    }
    if (ring.length === 0) break
    for (const [key, rgba] of ring) pixels.set(key, rgba)
    left = rest
  }
}
