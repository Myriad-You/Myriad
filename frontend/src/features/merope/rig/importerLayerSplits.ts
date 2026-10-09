import type { Layer, Psd } from 'ag-psd'
import type { Anime25DSourceReference, EyeSide, RasterLayer } from './anime25dImportTypes'
import { canonicalAnime25DLayerName } from './anime25dLayerSemantics'
import { rasterBounds, trimRaster, uniquePartId } from './anime25dRaster'
import { anime25DShoulderSeeds, splitLinkedHandwear } from './linkedHandwear'
import { flattenVisibleLayers } from './riggerBridge'

const ALPHA_COMPONENT_THRESHOLD = 16

/** See-through's plain `mouth` is the static portrait mouth, not an open phoneme. */
export function hasStaticSeeThroughMouth(psd: Psd): boolean {
  const names = flattenVisibleLayers(psd.children ?? []).map((layer) =>
    canonicalAnime25DLayerName(layer.name),
  )
  const hasPlainMouth = names.some(
    (name) => name === 'mouth' || /^mouth-?\d+$/.test(name),
  )
  const hasAuthoredOpen = names.some(
    (name) => name === 'mouth-open' || /^mouth-open-?\d+$/.test(name),
  )
  const hasAuthoredClose = names.some(
    (name) => name === 'mouth-c' || name === 'mouth-close',
  )
  return hasPlainMouth && !hasAuthoredOpen && !hasAuthoredClose
}

export function preserveStaticMouthAsClosed(layers: RasterLayer[]): RasterLayer[] {
  const staticMouth = layers.find(
    (layer) => layer.role === 'mouth-open' && !layer.synthetic,
  )
  if (!staticMouth) return layers
  const output = layers.filter(
    (layer) => layer !== staticMouth && layer.role !== 'mouth-close',
  )
  const usedIds = new Set(output.map((layer) => layer.id))
  const closed = {
    ...staticMouth,
    id: uniquePartId('mouth-close', usedIds),
    role: 'mouth-close' as const,
    sourceName: 'mouth-close',
    synthetic: false,
  }
  const insertAt = Math.max(0, layers.indexOf(staticMouth))
  return output.toSpliced(Math.min(insertAt, output.length), 0, closed)
}

export function splitHandwearIfNeeded(
  layers: RasterLayer[],
  faceCenterX: number,
): RasterLayer[] {
  const output: RasterLayer[] = []
  const usedIds = new Set(layers.map((layer) => layer.id))
  const shoulders = anime25DShoulderSeeds(layers)
  for (const layer of layers) {
    if (layer.role !== 'handwear' || layer.side) {
      output.push(layer)
      continue
    }
    const bySide = {
      right: splitRasterByComponents(layer, faceCenterX, 'right'),
      left: splitRasterByComponents(layer, faceCenterX, 'left'),
    }
    // Touching hands leave both arms in one piece; cut it at the shoulders'
    // meeting line instead of losing a whole arm to the other side.
    const separate = rasterBounds(bySide.right) && rasterBounds(bySide.left)
    const linked = !separate && shoulders
      ? splitLinkedHandwear(layer, shoulders, faceCenterX)
      : null
    for (const side of ['right', 'left'] as const) {
      const split = (linked ?? bySide)[side]
      if (!rasterBounds(split)) continue
      split.id = uniquePartId(`handwear-${side}`, usedIds)
      split.side = side
      output.push(trimRaster(split))
    }
  }
  return output
}

/**
 * A full figure's legs and shoes each arrive as one layer, but each side
 * stands and swings on its own. Split them about the legs' own midline; legs
 * pressed together leave one piece, which is cut down that midline instead.
 */
export function splitLowerLimbsIfNeeded(layers: RasterLayer[]): RasterLayer[] {
  const reference =
    layers.find((layer) => layer.role === 'legwear' && !layer.side) ??
    layers.find((layer) => layer.role === 'footwear' && !layer.side)
  const midline = reference ? (alphaCenter(reference)?.x ?? null) : null
  if (midline === null) return layers
  const output: RasterLayer[] = []
  const usedIds = new Set(layers.map((layer) => layer.id))
  for (const layer of layers) {
    if ((layer.role !== 'legwear' && layer.role !== 'footwear') || layer.side) {
      output.push(layer)
      continue
    }
    const bySide = {
      left: splitRasterByComponents(layer, midline, 'left'),
      right: splitRasterByComponents(layer, midline, 'right'),
    }
    const separate = rasterBounds(bySide.left) && rasterBounds(bySide.right)
    for (const side of ['left', 'right'] as const) {
      const split = separate ? bySide[side] : splitRasterAtColumn(layer, midline, side)
      if (!rasterBounds(split)) continue
      split.id = uniquePartId(`${layer.role}-${side}`, usedIds)
      split.side = side
      output.push(trimRaster(split))
    }
  }
  return output
}

/** Keeps the part of a layer on one side of a canvas column. */
function splitRasterAtColumn(
  source: RasterLayer,
  canvasX: number,
  side: EyeSide,
): RasterLayer {
  const output = { ...source, data: new Uint8ClampedArray(source.data) }
  for (let y = 0; y < source.height; y += 1) {
    for (let x = 0; x < source.width; x += 1) {
      const onLeft = source.left + x + 0.5 < canvasX
      if (onLeft !== (side === 'left')) {
        output.data[(y * source.width + x) * 4 + 3] = 0
      }
    }
  }
  return output
}

/** A pixel is decided when one layer is this much closer to the portrait's colour. */
const STACK_DECISIVE_DISTANCE = 18
/** A pair is restacked only when at least this many pixels decide it, and most agree. */
const STACK_MIN_DECIDED = 40
const STACK_MAJORITY = 0.65

/**
 * A decomposer orders a leg and its shoe by estimated depth, which can put a
 * sock over the shoe it is worn in (or a shoe over a trouser hem). Where the
 * two overlap, the portrait shows which one is in front: put that one on top.
 */
export function stackLowerLimbsByReference(
  layers: RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): RasterLayer[] {
  const output = [...layers]
  for (const side of ['left', 'right'] as const) {
    const leg = output.findIndex((layer) => layer.role === 'legwear' && layer.side === side)
    const foot = output.findIndex((layer) => layer.role === 'footwear' && layer.side === side)
    if (leg >= 0 && foot >= 0) stackPairByReference(output, leg, foot, reference)
  }
  return output
}

/**
 * The same depth guess, on a small head in a full figure, can put a choker
 * and its pendant behind the neck and the top they lie on, leaving a faint
 * band where the choker was. The portrait shows which is in front.
 */
export function stackNeckwearByReference(
  layers: RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): RasterLayer[] {
  return stackRoleByReference(layers, 'neckwear', ['neck', 'topwear'], reference)
}

/**
 * The decomposer can put a hair clip under the front hair it is pinned to,
 * where the hair then hides it whole (a small head in a full figure: both
 * clips gone). The portrait shows which is on top; a hat brim the bangs fall
 * over stays under them.
 */
export function stackHeadwearByReference(
  layers: RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): RasterLayer[] {
  return stackRoleByReference(layers, 'headwear', ['front-hair'], reference)
}

/**
 * In a full figure the sleeves and hands can be put behind the skirt and the
 * legs; a hand hanging beside the skirt then slips behind it, and behind the
 * thigh, as the arm swings in. Where the portrait shows which is in front it
 * decides; where an arm barely touches them it goes in front. An arm partly
 * in front and partly behind keeps its place.
 */
export function stackArmsByReference(
  layers: RasterLayer[],
  reference: Readonly<Anime25DSourceReference>,
): RasterLayer[] {
  return stackRoleByReference(layers, 'handwear', ['bottomwear', 'legwear'], reference, true)
}

/**
 * Each layer of `role` against each layer of `unders`, in that order; with
 * `inFront`, a pair that barely overlaps puts the `role` layer in front.
 */
function stackRoleByReference(
  layers: RasterLayer[],
  role: RasterLayer['role'],
  unders: readonly RasterLayer['role'][],
  reference: Readonly<Anime25DSourceReference>,
  inFront = false,
): RasterLayer[] {
  const output = [...layers]
  for (const under of unders) {
    for (const below of output.filter((layer) => layer.role === under)) {
      for (const piece of output.filter((layer) => layer.role === role)) {
        stackPairByReference(output, output.indexOf(below), output.indexOf(piece), reference, inFront)
      }
    }
  }
  return output
}

/**
 * Of two layers, moves the one the portrait shows in front just above the
 * other, if it is behind. With `bInFront`, `b` goes in front when too little
 * of their overlap shows to tell; a split verdict moves nothing.
 */
function stackPairByReference(
  output: RasterLayer[],
  a: number,
  b: number,
  reference: Readonly<Anime25DSourceReference>,
  bInFront = false,
): void {
  const votes = referenceVotes(output[a], output[b], reference)
  const front =
    frontByVotes(votes, output[a], output[b]) ??
    (bInFront && votes.forA + votes.forB < STACK_MIN_DECIDED ? output[b] : null)
  // Removing the lower one shifts the upper one down a place, so the moved
  // layer lands just above it.
  if (front === output[b] && b < a) {
    output.splice(a, 0, ...output.splice(b, 1))
  } else if (front === output[a] && a < b) {
    output.splice(b, 0, ...output.splice(a, 1))
  }
}

interface ReferenceVotes {
  forA: number
  forB: number
}

/** Of two overlapping layers, the one whose overlap the portrait shows, if clear. */
function frontByVotes(votes: ReferenceVotes, a: RasterLayer, b: RasterLayer): RasterLayer | null {
  const decided = votes.forA + votes.forB
  if (decided < STACK_MIN_DECIDED) return null
  if (votes.forA / decided >= STACK_MAJORITY) return a
  if (votes.forB / decided >= STACK_MAJORITY) return b
  return null
}

/** Pixels of the two layers' overlap where the portrait clearly matches one of them. */
function referenceVotes(
  a: RasterLayer,
  b: RasterLayer,
  reference: Readonly<Anime25DSourceReference>,
): ReferenceVotes {
  const x0 = Math.max(a.left, b.left, 0)
  const y0 = Math.max(a.top, b.top, 0)
  const x1 = Math.min(a.left + a.width, b.left + b.width, reference.width)
  const y1 = Math.min(a.top + a.height, b.top + b.height, reference.height)
  let forA = 0
  let forB = 0
  for (let y = y0; y < y1; y += 1) {
    for (let x = x0; x < x1; x += 1) {
      const ia = ((y - a.top) * a.width + (x - a.left)) * 4
      const ib = ((y - b.top) * b.width + (x - b.left)) * 4
      if (a.data[ia + 3] < 200 || b.data[ib + 3] < 200) continue
      const ir = (y * reference.width + x) * 4
      if (reference.data[ir + 3] < 250) continue
      const distance = (data: Uint8ClampedArray, i: number) =>
        Math.hypot(
          data[i] - reference.data[ir],
          data[i + 1] - reference.data[ir + 1],
          data[i + 2] - reference.data[ir + 2],
        )
      const difference = distance(a.data, ia) - distance(b.data, ib)
      if (difference < -STACK_DECISIVE_DISTANCE) forA += 1
      else if (difference > STACK_DECISIVE_DISTANCE) forB += 1
    }
  }
  return { forA, forB }
}

/** Bottom to top: an open eye's white, then its iris, then its lashes. */
const OPEN_EYE_STACK = ['eyewhite', 'irides', 'eyelash'] as const

/**
 * A decomposer orders layers by estimated depth, which can put an eye's white
 * over its own iris and lashes and leave a blank eye. Restack each eye's open
 * parts in drawing order, within the places they already hold; an eye drawn in
 * order stays as it is.
 */
export function stackOpenEyesInOrder(layers: RasterLayer[]): RasterLayer[] {
  const output = [...layers]
  for (const side of ['left', 'right'] as const) {
    const slots = output
      .map((layer, index) => ({ layer, index }))
      .filter(({ layer }) =>
        layer.side === side && (OPEN_EYE_STACK as readonly string[]).includes(layer.role),
      )
    const ordered = slots
      .map(({ layer }) => layer)
      .toSorted(
        (a, b) =>
          OPEN_EYE_STACK.indexOf(a.role as (typeof OPEN_EYE_STACK)[number]) -
          OPEN_EYE_STACK.indexOf(b.role as (typeof OPEN_EYE_STACK)[number]),
      )
    slots.forEach(({ index }, slot) => {
      output[index] = ordered[slot]
    })
  }
  return output
}

function alphaCenter(layer: RasterLayer): { x: number; y: number } | null {
  let weight = 0
  let sumX = 0
  let sumY = 0
  for (let y = 0; y < layer.height; y += 1) {
    for (let x = 0; x < layer.width; x += 1) {
      const alpha = layer.data[(y * layer.width + x) * 4 + 3]
      if (alpha <= ALPHA_COMPONENT_THRESHOLD) continue
      weight += alpha
      sumX += alpha * x
      sumY += alpha * y
    }
  }
  return weight > 0
    ? { x: layer.left + sumX / weight, y: layer.top + sumY / weight }
    : null
}

/**
 * A decomposer working on a small face can lose one eye's white while keeping
 * that eye's iris and lashes. Without a white the rigger anchors no eye there,
 * and no closed or expression eyes follow. Before rigging, give that eye the
 * other eye's white, mirrored and placed where it sits relative to its own
 * iris; it stays under iris and lashes, so only its outline shows.
 */
export function mirrorLostEyeWhite(
  psd: Psd,
  baseName: (name: string) => string,
): void {
  const layers = (psd.children ?? []).filter((layer) => layer.imageData)
  const named = (name: string) =>
    layers.filter((layer) => baseName(layer.name ?? '') === name)
  const face = named('face')[0]
  const whites = named('eyewhite')
  const faceCenter = face && psdLayersCenter([face])
  if (!faceCenter || whites.length !== 1) return
  const white = whites[0]
  const whiteCenter = psdLayersCenter([white])!
  const whiteLeft = whiteCenter.x < faceCenter.x
  const pixels = white.imageData!
  const left = white.left ?? 0
  const top = white.top ?? 0
  for (let y = 0; y < pixels.height; y += 1) {
    for (let x = 0; x < pixels.width; x += 1) {
      const onLeft = left + x < faceCenter.x
      if (onLeft !== whiteLeft && pixels.data[(y * pixels.width + x) * 4 + 3] > ALPHA_COMPONENT_THRESHOLD) {
        return
      }
    }
  }
  // Each eye's iris, split at the face's middle: one layer may hold both.
  const irisOn = (onLeft: boolean) =>
    psdLayersCenter(named('irides'), (x) => x < faceCenter.x === onLeft)
  const own = irisOn(whiteLeft)
  const other = irisOn(!whiteLeft)
  if (!own || !other) return
  // Mirror about the white's own centre, then carry it to the other iris.
  const shiftX = Math.round(other.x - (whiteCenter.x - own.x) - whiteCenter.x)
  const shiftY = Math.round(other.y + (whiteCenter.y - own.y) - whiteCenter.y)
  const mirroredLeft = Math.round(2 * whiteCenter.x - (left + pixels.width)) + shiftX
  const mirroredTop = top + shiftY
  const unionLeft = Math.min(left, mirroredLeft)
  const unionTop = Math.min(top, mirroredTop)
  const width = Math.max(left, mirroredLeft) + pixels.width - unionLeft
  const height = Math.max(top, mirroredTop) + pixels.height - unionTop
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < pixels.height; y += 1) {
    for (let x = 0; x < pixels.width; x += 1) {
      const from = (y * pixels.width + x) * 4
      if (pixels.data[from + 3] === 0) continue
      const own = ((top - unionTop + y) * width + (left - unionLeft + x)) * 4
      const mirrored =
        ((mirroredTop - unionTop + y) * width +
          (mirroredLeft - unionLeft + pixels.width - 1 - x)) * 4
      data.set(pixels.data.subarray(from, from + 4), own)
      data.set(pixels.data.subarray(from, from + 4), mirrored)
    }
  }
  white.left = unionLeft
  white.top = unionTop
  white.right = unionLeft + width
  white.bottom = unionTop + height
  white.imageData = { width, height, data }
}

/** The alpha-weighted centre of some layers' pixels, in document space, optionally only where `keep(x)`. */
function psdLayersCenter(
  layers: readonly Layer[],
  keep: (documentX: number) => boolean = () => true,
): { x: number; y: number } | null {
  let weight = 0
  let sumX = 0
  let sumY = 0
  for (const layer of layers) {
    const pixels = layer.imageData
    if (!pixels) continue
    const left = layer.left ?? 0
    const top = layer.top ?? 0
    for (let y = 0; y < pixels.height; y += 1) {
      for (let x = 0; x < pixels.width; x += 1) {
        const alpha = pixels.data[(y * pixels.width + x) * 4 + 3]
        if (alpha <= ALPHA_COMPONENT_THRESHOLD || !keep(left + x)) continue
        weight += alpha
        sumX += alpha * (left + x)
        sumY += alpha * (top + y)
      }
    }
  }
  return weight > 0 ? { x: sumX / weight, y: sumY / weight } : null
}

export function splitVariantEyesIfNeeded(
  layers: RasterLayer[],
  faceCenterX: number,
  role: 'eye-dizzy' | 'eye-squeeze' | 'eye-cry',
): RasterLayer[] {
  const output: RasterLayer[] = []
  const usedIds = new Set(layers.map((layer) => layer.id))
  for (const layer of layers) {
    if (layer.role !== role || layer.side) {
      output.push(layer)
      continue
    }
    for (const side of ['left', 'right'] as const) {
      const split = splitRasterByComponents(layer, faceCenterX, side)
      if (!rasterBounds(split)) continue
      split.id = uniquePartId(`${role}-${side}`, usedIds)
      split.side = side
      output.push(trimRaster(split))
    }
  }
  return output
}

function splitRasterByComponents(
  source: RasterLayer,
  faceCenterX: number,
  anatomicalSide: EyeSide,
): RasterLayer {
  const output = { ...source, data: new Uint8ClampedArray(source.data) }
  const components = labelAlphaComponents(
    source.data,
    source.width,
    source.height,
  )
  const keep = new Set<number>()
  for (let component = 1; component <= components.count; component += 1) {
    if (components.sizes[component] < 20) continue
    const canvasX =
      source.left + components.sumX[component] / components.sizes[component]
    const side: EyeSide = canvasX < faceCenterX ? 'left' : 'right'
    if (side === anatomicalSide) keep.add(component)
  }
  for (let pixel = 0; pixel < components.labels.length; pixel += 1) {
    if (!keep.has(components.labels[pixel])) output.data[pixel * 4 + 3] = 0
  }
  return output
}

export function assignCrossfadeSlots(layers: RasterLayer[]): void {
  for (const side of ['left', 'right'] as const) {
    const open = layers.find(
      (layer) => layer.role === 'eyelash' && layer.side === side,
    )
    const dizzy = layers.find(
      (layer) => layer.role === 'eye-dizzy' && layer.side === side,
    )
    const squeeze = layers.find(
      (layer) => layer.role === 'eye-squeeze' && layer.side === side,
    )
    const cry = layers.find(
      (layer) => layer.role === 'eye-cry' && layer.side === side,
    )
    const silly = layers.find(
      (layer) => layer.role === 'eye-silly-white' && layer.side === side,
    )
    const slot = side === 'left' ? 'eye-left' : 'eye-right'
    if (open) {
      open.slot = slot
      open.variant = 'open'
    }
    for (const closed of layers) {
      if (
        (closed.role === 'eye-close' || closed.role === 'eye-close2') &&
        closed.side === side
      ) {
        closed.slot = slot
        closed.variant = 'closed'
      }
    }
    if (dizzy) {
      dizzy.slot = slot
      dizzy.variant = 'dizzy'
    }
    if (squeeze) {
      squeeze.slot = slot
      squeeze.variant = 'squeeze'
    }
    if (cry) {
      cry.slot = slot
      cry.variant = 'cry'
    }
    if (silly) {
      silly.slot = slot
      silly.variant = 'silly'
    }
  }
  const mouthVariants = [
    ['mouth-open', 'open'],
    ['mouth-wide', 'wide'],
    ['mouth-round', 'round'],
    ['mouth-narrow', 'narrow'],
    ['mouth-close', 'closed'],
    ['mouth-cry', 'cry'],
    ['mouth-maniac', 'maniac'],
    ['mouth-silly', 'silly'],
  ] as const
  for (const [role, variant] of mouthVariants) {
    const layer = layers.find((candidate) => candidate.role === role)
    if (layer) {
      layer.slot = 'mouth'
      layer.variant = variant
    }
  }
}

function labelAlphaComponents(
  data: Uint8ClampedArray,
  width: number,
  height: number,
): { labels: Int32Array; sizes: number[]; sumX: number[]; count: number } {
  const labels = new Int32Array(width * height)
  const stack = new Int32Array(width * height)
  const sizes = [0]
  const sumX = [0]
  let count = 0
  for (let start = 0; start < labels.length; start += 1) {
    if (labels[start] || data[start * 4 + 3] <= ALPHA_COMPONENT_THRESHOLD)
      continue
    count += 1
    let stackSize = 0
    stack[stackSize++] = start
    labels[start] = count
    let size = 0
    let xSum = 0
    while (stackSize > 0) {
      const pixel = stack[--stackSize]
      const x = pixel % width
      const y = Math.floor(pixel / width)
      size += 1
      xSum += x
      for (const neighbor of [
        x > 0 ? pixel - 1 : -1,
        x < width - 1 ? pixel + 1 : -1,
        y > 0 ? pixel - width : -1,
        y < height - 1 ? pixel + width : -1,
      ]) {
        if (
          neighbor >= 0 &&
          labels[neighbor] === 0 &&
          data[neighbor * 4 + 3] > ALPHA_COMPONENT_THRESHOLD
        ) {
          labels[neighbor] = count
          stack[stackSize++] = neighbor
        }
      }
    }
    sizes.push(size)
    sumX.push(xSum)
  }
  return { labels, sizes, sumX, count }
}
