import type { Psd } from 'ag-psd'
import type { EyeSide, RasterLayer } from './anime25dImportTypes'
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
  const midline = reference ? alphaCenterX(reference) : null
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

function alphaCenterX(layer: RasterLayer): number | null {
  let weight = 0
  let sum = 0
  for (let y = 0; y < layer.height; y += 1) {
    for (let x = 0; x < layer.width; x += 1) {
      const alpha = layer.data[(y * layer.width + x) * 4 + 3]
      if (alpha <= ALPHA_COMPONENT_THRESHOLD) continue
      weight += alpha
      sum += alpha * x
    }
  }
  return weight > 0 ? layer.left + sum / weight : null
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
