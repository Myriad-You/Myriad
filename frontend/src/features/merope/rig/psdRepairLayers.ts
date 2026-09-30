import type { Anime25DSourceReference, RasterLayer } from './anime25dImportTypes'
import type { Anime25DPsdAnalysis } from './psdReconciliation'
import { anime25DLayerFade } from './anime25d'
import { uniquePartId } from './anime25dRaster'
import { MATCH_DISTANCE } from './psdReconciliation'
import { alphaAt, fringe } from './psdRepairMasks'

type Group = RasterLayer['group']
const GROUP_VOTE_RING = 3

export function hasExpressionVariants(layer: RasterLayer): boolean {
  return Boolean(layer.slot) || anime25DLayerFade(layer.role) !== null
}

/** Only erase fringe pixels where the covered art is the closer match. */
function revealsBetter(
  analysis: Readonly<Anime25DPsdAnalysis>,
  below: RasterLayer,
  target: number,
  { x, y }: { x: number; y: number },
  reference: Readonly<Anime25DSourceReference>,
): boolean {
  if (alphaAt(below, x, y) < 128) return false
  const referenceOffset = (y * reference.width + x) * 4
  if (reference.data[referenceOffset + 3] < 250) return false
  const belowOffset = ((y - below.top) * below.width + (x - below.left)) * 4
  const belowDistance = Math.hypot(
    below.data[belowOffset] - reference.data[referenceOffset],
    below.data[belowOffset + 1] - reference.data[referenceOffset + 1],
    below.data[belowOffset + 2] - reference.data[referenceOffset + 2],
  )
  if (belowDistance >= MATCH_DISTANCE) return false
  const compositeDistance = Math.hypot(
    analysis.color[target * 3] - reference.data[referenceOffset],
    analysis.color[target * 3 + 1] - reference.data[referenceOffset + 1],
    analysis.color[target * 3 + 2] - reference.data[referenceOffset + 2],
  )
  return compositeDistance > belowDistance + 10
}

/** The highest covered layer whose art beats the composite at a pixel. */
export function revealableBelow(
  analysis: Readonly<Anime25DPsdAnalysis>,
  visible: readonly RasterLayer[],
  target: number,
  position: { x: number; y: number },
  reference: Readonly<Anime25DSourceReference>,
): number {
  for (let index = analysis.top[target] - 1; index >= 0; index -= 1) {
    if (revealsBetter(analysis, visible[index], target, position, reference)) {
      return index
    }
  }
  return -1
}

export interface RecoveredPiece {
  pixels: Map<number, readonly [number, number, number, number]>
  /** Topmost layer the piece overrides, or -1 over open backdrop. */
  above: number
  /** The layer it touches most, used to place art over open backdrop. */
  neighbour: number
  /** A rigid accessory this piece belongs to, or -1. */
  owner: number
  group: Group
  role: RasterLayer['role']
}

const ACCESSORY_ROLES = new Set([
  'headwear',
  'earwear',
  'neckwear',
  'eyewear',
  'wings',
  'tail',
])
/** An accessory touching this much of the rim is taken as the piece's owner. */
const ACCESSORY_VOTE_SHARE = 0.15
/** Hanging art is held at its top: that end must reach what holds it. */
const HOOK_REACH = 8
const HOOK_BAND = 0.2

/**
 * Recovered art should move with what it hangs from: a tassel under a hair
 * ornament shares that ornament's role and so its mount and depth, an earring
 * mounts on the ears, anything on hair rides the hair.
 */
export function mountFor(
  analysis: Readonly<Anime25DPsdAnalysis>,
  members: readonly number[],
  visible: readonly RasterLayer[],
): Pick<RecoveredPiece, 'group' | 'neighbour' | 'owner' | 'role'> {
  const votes = new Map<number, number>()
  let total = 0
  for (const [target] of fringe(
    members,
    analysis.bounds.width,
    analysis.bounds.height,
    GROUP_VOTE_RING,
  )) {
    const top = analysis.top[target]
    if (top < 0) continue
    votes.set(top, (votes.get(top) ?? 0) + 1)
    total += 1
  }
  if (total === 0) {
    return { group: 'body', neighbour: -1, owner: -1, role: 'objects' }
  }
  const ranked = [...votes].toSorted((left, right) => right[1] - left[1])
  const neighbour = ranked[0][0]
  const accessory = ranked.find(
    ([index, count]) =>
      ACCESSORY_ROLES.has(visible[index].role) &&
      count / total >= ACCESSORY_VOTE_SHARE,
  )
  const holder = hangsFrom(analysis, members, visible, (layer) =>
    ACCESSORY_ROLES.has(layer.role),
  )
  if (holder >= 0) {
    const owner = visible[holder]
    return {
      group: owner.group,
      neighbour: holder,
      owner: hasExpressionVariants(owner) ? -1 : holder,
      role: owner.role,
    }
  }
  if (accessory) {
    const owner = visible[accessory[0]]
    return {
      group: owner.group,
      neighbour: accessory[0],
      owner: hasExpressionVariants(owner) ? -1 : accessory[0],
      role: owner.role,
    }
  }
  if (
    hangsFrom(
      analysis,
      members,
      visible,
      (layer) => layer.role === 'ears' || layer.role === 'earwear',
    ) >= 0
  ) {
    return { group: 'head', neighbour, owner: -1, role: 'earwear' }
  }
  const main = visible[neighbour]
  if (main.role === 'front-hair' || main.role === 'back-hair') {
    return { group: 'head', neighbour, owner: -1, role: 'headwear' }
  }
  if (
    main.role === 'neck' ||
    main.role === 'collar-front' ||
    main.role === 'collar-back'
  ) {
    return { group: 'body', neighbour, owner: -1, role: 'neckwear' }
  }
  return { group: main.group, neighbour, owner: -1, role: 'objects' }
}

/** The first layer matching `accepts` that the piece's top end reaches. */
function hangsFrom(
  analysis: Readonly<Anime25DPsdAnalysis>,
  members: readonly number[],
  visible: readonly RasterLayer[],
  accepts: (layer: RasterLayer) => boolean,
): number {
  const candidates = visible
    .map((layer, index) => ({ layer, index }))
    .filter(({ layer }) => accepts(layer))
  if (candidates.length === 0) return -1
  const { bounds } = analysis
  let top = Number.POSITIVE_INFINITY
  let bottom = 0
  for (const target of members) {
    const y = Math.floor(target / bounds.width)
    top = Math.min(top, y)
    bottom = Math.max(bottom, y)
  }
  const band = top + Math.max(4, (bottom - top) * HOOK_BAND)
  for (const target of members) {
    if (Math.floor(target / bounds.width) > band) continue
    const x = bounds.x0 + (target % bounds.width)
    const y = bounds.y0 + Math.floor(target / bounds.width)
    for (let dy = -HOOK_REACH; dy <= HOOK_REACH; dy += 2) {
      for (let dx = -HOOK_REACH; dx <= HOOK_REACH; dx += 2) {
        const hook = candidates.find(
          ({ layer }) => alphaAt(layer, x + dx, y + dy) >= 128,
        )
        if (hook) return hook.index
      }
    }
  }
  return -1
}

export function mergeByGroup(
  pieces: readonly RecoveredPiece[],
  maxLayers: number,
): RecoveredPiece[] {
  const merged = new Map<Group, RecoveredPiece>()
  for (const piece of pieces) {
    const existing = merged.get(piece.group)
    if (!existing) {
      if (merged.size >= maxLayers) continue
      merged.set(piece.group, {
        ...piece,
        pixels: new Map(piece.pixels),
        role: 'objects',
      })
      continue
    }
    for (const [key, rgba] of piece.pixels) existing.pixels.set(key, rgba)
    existing.above = Math.max(existing.above, piece.above)
    if (existing.neighbour < 0) existing.neighbour = piece.neighbour
  }
  return [...merged.values()]
}

/** Paints recovered pixels into a layer, growing its bounds as needed. */
export function withPixels(
  layer: RasterLayer,
  pixels: ReadonlyMap<number, readonly [number, number, number, number]>,
  referenceWidth: number,
): RasterLayer {
  let left = layer.left
  let top = layer.top
  let right = layer.left + layer.width
  let bottom = layer.top + layer.height
  for (const key of pixels.keys()) {
    const x = key % referenceWidth
    const y = Math.floor(key / referenceWidth)
    left = Math.min(left, x)
    top = Math.min(top, y)
    right = Math.max(right, x + 1)
    bottom = Math.max(bottom, y + 1)
  }
  const width = right - left
  const height = bottom - top
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < layer.height; y += 1) {
    const start = y * layer.width * 4
    data.set(
      layer.data.subarray(start, start + layer.width * 4),
      ((y + layer.top - top) * width + layer.left - left) * 4,
    )
  }
  for (const [key, [red, green, blue, alpha]] of pixels) {
    const offset =
      ((Math.floor(key / referenceWidth) - top) * width +
        (key % referenceWidth) -
        left) *
      4
    // Over, in straight alpha: the source illustration wins where it is opaque.
    const coverage = alpha / 255
    const below = data[offset + 3] / 255
    const combined = coverage + below * (1 - coverage)
    if (combined <= 0) continue
    const mix = (channel: number, value: number) =>
      (value * coverage + data[offset + channel] * below * (1 - coverage)) /
      combined
    data[offset] = mix(0, red)
    data[offset + 1] = mix(1, green)
    data[offset + 2] = mix(2, blue)
    data[offset + 3] = combined * 255
  }
  return { ...layer, left, top, width, height, data }
}

export function overlayLayer(
  { group, pixels, role }: RecoveredPiece,
  referenceWidth: number,
  layers: readonly RasterLayer[],
): RasterLayer | null {
  if (pixels.size === 0) return null
  let left = Number.POSITIVE_INFINITY
  let top = Number.POSITIVE_INFINITY
  let right = 0
  let bottom = 0
  for (const key of pixels.keys()) {
    const x = key % referenceWidth
    const y = Math.floor(key / referenceWidth)
    left = Math.min(left, x)
    top = Math.min(top, y)
    right = Math.max(right, x + 1)
    bottom = Math.max(bottom, y + 1)
  }
  const width = right - left
  const height = bottom - top
  const data = new Uint8ClampedArray(width * height * 4)
  for (const [key, rgba] of pixels) {
    const x = (key % referenceWidth) - left
    const y = Math.floor(key / referenceWidth) - top
    data.set(rgba, (y * width + x) * 4)
  }
  const id = uniquePartId(
    `recovered-${role}`,
    new Set(layers.map((layer) => layer.id)),
  )
  return {
    id,
    role,
    sourceName: id,
    order: 0,
    side: null,
    group,
    left,
    top,
    width,
    height,
    data,
    synthetic: true,
  }
}
