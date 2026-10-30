import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { Anime25DEyeAnchor } from '../anime25drig/types'
import type { Anime25DSourceReference, EyeSide, RasterLayer } from './anime25dImportTypes'
import { ANIME25D_LAYER_DESCRIPTORS } from './anime25d'
import { trimRaster, uniquePartId } from './anime25dRaster'

/**
 * Expression variants of the portrait redrawn by an image model: the same
 * picture with only the face changed. Each one is cut against the face layer
 * into the replacement parts the rig already fades between. A redraw replaces
 * procedural glyphs and the generic closed eyes the rigger ships; only parts
 * the artist drew in the PSD win over it.
 */
export const AUTHORED_EXPRESSION_KINDS = ['cry', 'squeeze', 'close'] as const
export type AuthoredExpressionKind = (typeof AUTHORED_EXPRESSION_KINDS)[number]

export interface AuthoredExpressionReference extends Anime25DSourceReference {
  kind: AuthoredExpressionKind
}

const EYE_ROLES = {
  cry: 'eye-cry',
  squeeze: 'eye-squeeze',
  close: 'eye-close',
} as const satisfies Record<AuthoredExpressionKind, RasterLayer['role']>

// Channel difference from the skin below: noise of the redraw stays clear,
// line art and tears come through opaque.
const ALPHA_FLOOR = 14
const ALPHA_FULL = 54
// Cut edge fades over this many pixels so blush or shading never ends in a seam.
const EDGE_FEATHER = 6
// Hair, brows and anything else drawn over the part belong to their own
// layers; copying them would leave strands behind when the hair swings. Under
// them nothing is cut; thin strands never paint fully opaque, so partial cover
// already counts. The redraw moves strand edges by a pixel or two, so near them
// a pixel is only cut where the portrait also differed from the skin: a strand
// the redraw shifted is skin in the portrait, a lash seen between strands is not.
const OCCLUDER_SOLID = 64
const OCCLUDER_EDGE = 24
const OCCLUDER_REACH = 3
// Only two kinds of pixel are cut: those the hidden neutral part drew, which
// something must now cover, and those the redraw changed. Everything else the
// rig already paints; copying it (strands kept in the face, shading) would
// freeze it onto the expression part.
const CHANGE_FLOOR = 14
const CHANGE_FULL = 42
const COVER_REACH = 2
// A changed patch that stays clear of the hidden part and is this small a
// share of the cut is a strand the redraw moved, not part of the expression.
const STRAY_SHARE = 0.004
// Mean channel difference between the redraw and the portrait away from the
// face. A clean redraw measures about 5; one pixel of drift already reaches 7.
const REGISTRATION_LIMIT = 8
const REGISTRATION_SEARCH = 3
const REGISTRATION_STEP = 4

export function addAuthoredExpressionLayers(
  layers: RasterLayer[],
  anchors: Anime25DRiggerAnchors,
  neutral: Anime25DSourceReference | undefined,
  references: readonly AuthoredExpressionReference[],
): RasterLayer[] {
  if (!neutral || references.length === 0) return layers
  const face = layers.find((layer) => layer.role === 'face')
  if (!face) return layers
  const usedIds = new Set(layers.map((layer) => layer.id))
  let output = layers
  for (const reference of references) {
    if (reference.width !== neutral.width || reference.height !== neutral.height) continue
    const shift = registerExpression(neutral, reference, anchors.face)
    if (!shift) continue
    const cut = (
      role: keyof typeof ANIME25D_LAYER_DESCRIPTORS,
      region: Region,
      hidden: readonly RasterLayer[],
    ) =>
      cutAgainstFace(
        reference,
        neutral,
        shift,
        face,
        occludersOf(output, role),
        hidden,
        region,
      )
    const eyes: RasterLayer[] = []
    for (const side of ['left', 'right'] as const) {
      const role = EYE_ROLES[reference.kind]
      const replaced = output.filter((layer) => layer.role === role && layer.side === side)
      if (replaced.some((layer) => !layer.synthetic)) continue
      const eye = side === 'left' ? anchors.eyeL : anchors.eyeR
      if (!eye) continue
      const hidden = eyeLayers(output, side)
      const part = cut(role, eyeRegion(eye, hidden), hidden)
      if (!part) continue
      output = output.filter((layer) => !replaced.includes(layer))
      eyes.push({
        id: uniquePartId(`${role}-${side}`, usedIds),
        role,
        sourceName: `${role}-${side}`,
        order: 0,
        side,
        group: 'head',
        ...part,
      })
    }
    output = insertAfter(output, eyes, (layer) =>
      layer.role === 'eye-close' ||
      layer.role === 'eye-dizzy' ||
      layer.role === 'eye-squeeze' ||
      layer.role === 'eye-cry' ||
      layer.role === 'eyelash')
    if (
      reference.kind === 'cry' &&
      !output.some((layer) => layer.role === 'mouth-cry' && !layer.synthetic)
    ) {
      const part = cut(
        'mouth-cry',
        mouthRegion(anchors, output),
        output.filter((layer) => layer.role === 'mouth-close'),
      )
      if (part) {
        output = insertAfter(output.filter((layer) => layer.role !== 'mouth-cry'), [{
          id: uniquePartId('mouth-cry', usedIds),
          role: 'mouth-cry',
          sourceName: 'mouth-cry',
          order: 0,
          side: null,
          group: 'head',
          ...part,
        }], (layer) => layer.role.startsWith('mouth-'))
      }
    }
  }
  return output
}

/**
 * Static artwork the rig paints above the part: hair, brows, the nose,
 * headwear. What the redraw shows there belongs to those layers.
 */
function occludersOf(
  layers: readonly RasterLayer[],
  role: keyof typeof ANIME25D_LAYER_DESCRIPTORS,
): RasterLayer[] {
  const depth = ANIME25D_LAYER_DESCRIPTORS[role].depth
  return layers.filter((layer) => {
    if (layer.group !== 'head' || layer.role === 'unknown') return false
    const descriptor = ANIME25D_LAYER_DESCRIPTORS[layer.role]
    return descriptor.fade === null && descriptor.depth > depth
  })
}

interface Region {
  x0: number
  y0: number
  x1: number
  y1: number
}

interface Shift {
  x: number
  y: number
}

/** Integer offset that lays the redraw over the portrait, or null when it drifted. */
export function registerExpression(
  neutral: Anime25DSourceReference,
  expression: Anime25DSourceReference,
  face: Anime25DRiggerAnchors['face'],
): Shift | null {
  // The face is what changed; everything around it must match.
  const margin = (face.x1 - face.x0) * 0.15
  const skip = {
    x0: face.x0 - margin,
    y0: face.y0 - margin,
    x1: face.x1 + margin,
    y1: face.y1 + margin,
  }
  let best: Shift | null = null
  let bestError = Infinity
  for (let dy = -REGISTRATION_SEARCH; dy <= REGISTRATION_SEARCH; dy += 1) {
    for (let dx = -REGISTRATION_SEARCH; dx <= REGISTRATION_SEARCH; dx += 1) {
      const error = registrationError(neutral, expression, skip, dx, dy)
      if (error < bestError) {
        bestError = error
        best = { x: dx, y: dy }
      }
    }
  }
  return bestError <= REGISTRATION_LIMIT ? best : null
}

function registrationError(
  neutral: Anime25DSourceReference,
  expression: Anime25DSourceReference,
  skip: Region,
  dx: number,
  dy: number,
): number {
  const { width, height } = neutral
  const edge = REGISTRATION_SEARCH + 1
  let sum = 0
  let count = 0
  for (let y = edge; y < height - edge; y += REGISTRATION_STEP) {
    for (let x = edge; x < width - edge; x += REGISTRATION_STEP) {
      if (x > skip.x0 && x < skip.x1 && y > skip.y0 && y < skip.y1) continue
      const i = (y * width + x) * 4
      // Portrait padding outside the illustration carries no evidence.
      if (neutral.data[i + 3] < 128) continue
      const j = ((y + dy) * width + x + dx) * 4
      sum += Math.abs(neutral.data[i] - expression.data[j]) +
        Math.abs(neutral.data[i + 1] - expression.data[j + 1]) +
        Math.abs(neutral.data[i + 2] - expression.data[j + 2])
      count += 3
    }
  }
  return count === 0 ? Infinity : sum / count
}

function eyeLayers(layers: readonly RasterLayer[], side: EyeSide): RasterLayer[] {
  return layers.filter((layer) =>
    layer.side === side &&
    (layer.role === 'eyewhite' || layer.role === 'irides' || layer.role === 'eyelash'))
}

function eyeRegion(eye: Anime25DEyeAnchor, drawn: readonly RasterLayer[]): Region {
  // The replacement must cover every pixel of the open eye it hides.
  let x0 = eye.x0
  let y0 = eye.y0
  let x1 = eye.x1
  let y1 = eye.y1
  for (const layer of drawn) {
    x0 = Math.min(x0, layer.left)
    y0 = Math.min(y0, layer.top)
    x1 = Math.max(x1, layer.left + layer.width)
    y1 = Math.max(y1, layer.top + layer.height)
  }
  const width = x1 - x0
  const height = y1 - y0
  // What an expression redraws lies within the open eye, save tears running
  // below the lid. Beyond that the redraw only rearranges hair strands, which
  // no expression part may carry.
  return {
    x0: x0 - width * 0.06,
    y0: y0 - height * 0.08,
    x1: x1 + width * 0.06,
    y1: y1 + height * 0.7,
  }
}

function mouthRegion(anchors: Anime25DRiggerAnchors, layers: readonly RasterLayer[]): Region {
  const faceWidth = anchors.face.x1 - anchors.face.x0
  let x0 = anchors.mouth.cx - faceWidth * 0.17
  let x1 = anchors.mouth.cx + faceWidth * 0.17
  let y0 = anchors.mouth.cy - faceWidth * 0.08
  let y1 = anchors.mouth.cy + faceWidth * 0.12
  for (const layer of layers) {
    if (layer.role !== 'mouth-close' && layer.role !== 'mouth-open') continue
    x0 = Math.min(x0, layer.left - 4)
    y0 = Math.min(y0, layer.top - 4)
    x1 = Math.max(x1, layer.left + layer.width + 4)
    y1 = Math.max(y1, layer.top + layer.height + 4)
  }
  return { x0, y0, x1, y1 }
}

type CutPart = Pick<RasterLayer, 'left' | 'top' | 'width' | 'height' | 'data'>

function cutAgainstFace(
  expression: Anime25DSourceReference,
  neutral: Anime25DSourceReference,
  shift: Shift,
  face: RasterLayer,
  occluders: readonly RasterLayer[],
  hidden: readonly RasterLayer[],
  region: Region,
): CutPart | null {
  const left = Math.max(0, Math.floor(region.x0))
  const top = Math.max(0, Math.floor(region.y0))
  const right = Math.min(expression.width, Math.ceil(region.x1))
  const bottom = Math.min(expression.height, Math.ceil(region.y1))
  const width = right - left
  const height = bottom - top
  if (width <= 0 || height <= 0) return null
  const data = new Uint8ClampedArray(width * height * 4)
  const nearOccluder = reachOf(occluders, OCCLUDER_EDGE, OCCLUDER_REACH, left, top, width, height)
  const covering = reachOf(hidden, OCCLUDER_EDGE, COVER_REACH, left, top, width, height)
  let opaque = 0
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const px = left + x
      const py = top + y
      const skin = sample(face, px, py)
      if (skin[3] < 128) continue
      if (occluders.some((layer) => sample(layer, px, py)[3] > OCCLUDER_SOLID)) continue
      const sx = Math.min(expression.width - 1, Math.max(0, px + shift.x))
      const sy = Math.min(expression.height - 1, Math.max(0, py + shift.y))
      const i = (sy * expression.width + sx) * 4
      const red = expression.data[i]
      const green = expression.data[i + 1]
      const blue = expression.data[i + 2]
      const drawn = expression.data.subarray(i, i + 3)
      const n = (py * neutral.width + px) * 4
      const portrait = neutral.data.subarray(n, n + 3)
      const needed = covering[y * width + x]
        ? 1
        : smoothstep(
            (channelDistance(drawn, portrait) - CHANGE_FLOOR) /
              (CHANGE_FULL - CHANGE_FLOOR),
          )
      if (needed <= 0) continue
      let difference = channelDistance(drawn, skin)
      if (nearOccluder[y * width + x]) {
        difference = Math.min(difference, channelDistance(portrait, skin))
      }
      const edge = Math.min(x + 1, y + 1, width - x, height - y) / EDGE_FEATHER
      const alpha =
        smoothstep((difference - ALPHA_FLOOR) / (ALPHA_FULL - ALPHA_FLOOR)) *
        needed *
        Math.min(1, edge)
      if (alpha <= 0.02) continue
      // Unblend from the skin so the layer over the face reproduces the redraw.
      const o = (y * width + x) * 4
      data[o] = (red - (1 - alpha) * skin[0]) / alpha
      data[o + 1] = (green - (1 - alpha) * skin[1]) / alpha
      data[o + 2] = (blue - (1 - alpha) * skin[2]) / alpha
      data[o + 3] = Math.round(alpha * 255)
      if (alpha > 0.5) opaque += 1
    }
  }
  if (opaque === 0) return null
  dropStrayPatches(data, covering, width, height)
  const trimmed = trimRaster({
    id: '',
    role: 'unknown',
    sourceName: '',
    order: 0,
    side: null,
    group: 'head',
    left,
    top,
    width,
    height,
    data,
  })
  return {
    left: trimmed.left,
    top: trimmed.top,
    width: trimmed.width,
    height: trimmed.height,
    data: trimmed.data,
  }
}

/** Pixels of the region within reach of the layers' paint, dilated square-wise. */
function reachOf(
  layers: readonly RasterLayer[],
  threshold: number,
  reach: number,
  left: number,
  top: number,
  width: number,
  height: number,
): Uint8Array {
  const paddedWidth = width + reach * 2
  const paddedHeight = height + reach * 2
  const seeds = new Uint8Array(paddedWidth * paddedHeight)
  for (let y = 0; y < paddedHeight; y += 1) {
    for (let x = 0; x < paddedWidth; x += 1) {
      const px = left + x - reach
      const py = top + y - reach
      if (layers.some((layer) => sample(layer, px, py)[3] > threshold)) {
        seeds[y * paddedWidth + x] = 1
      }
    }
  }
  const rows = new Uint8Array(width * paddedHeight)
  for (let y = 0; y < paddedHeight; y += 1) {
    for (let x = 0; x < width; x += 1) {
      let hit = 0
      for (let k = 0; k <= reach * 2 && !hit; k += 1) hit = seeds[y * paddedWidth + x + k]
      rows[y * width + x] = hit
    }
  }
  const reached = new Uint8Array(width * height)
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      let hit = 0
      for (let k = 0; k <= reach * 2 && !hit; k += 1) hit = rows[(y + k) * width + x]
      reached[y * width + x] = hit
    }
  }
  return reached
}

/** Clears painted patches that neither touch the hidden part nor reach a useful size. */
function dropStrayPatches(
  data: Uint8ClampedArray,
  covering: Uint8Array,
  width: number,
  height: number,
): void {
  const limit = Math.max(4, width * height * STRAY_SHARE)
  const seen = new Uint8Array(width * height)
  const stack: number[] = []
  const patch: number[] = []
  for (let start = 0; start < seen.length; start += 1) {
    if (seen[start] || data[start * 4 + 3] === 0) continue
    seen[start] = 1
    stack.push(start)
    patch.length = 0
    let anchored = false
    while (stack.length > 0) {
      const index = stack.pop()!
      patch.push(index)
      if (covering[index]) anchored = true
      const x = index % width
      const y = (index - x) / width
      for (let dy = -1; dy <= 1; dy += 1) {
        for (let dx = -1; dx <= 1; dx += 1) {
          const nx = x + dx
          const ny = y + dy
          if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue
          const next = ny * width + nx
          if (seen[next] || data[next * 4 + 3] === 0) continue
          seen[next] = 1
          stack.push(next)
        }
      }
    }
    if (anchored || patch.length >= limit) continue
    for (const index of patch) data[index * 4 + 3] = 0
  }
}

function channelDistance(colour: ArrayLike<number>, skin: Uint8ClampedArray): number {
  return Math.max(
    Math.abs(colour[0] - skin[0]),
    Math.abs(colour[1] - skin[1]),
    Math.abs(colour[2] - skin[2]),
  )
}

const TRANSPARENT = new Uint8ClampedArray(4)

function sample(layer: RasterLayer, x: number, y: number): Uint8ClampedArray {
  const lx = x - layer.left
  const ly = y - layer.top
  if (lx < 0 || ly < 0 || lx >= layer.width || ly >= layer.height) return TRANSPARENT
  const i = (ly * layer.width + lx) * 4
  return layer.data.subarray(i, i + 4)
}

function insertAfter(
  layers: RasterLayer[],
  inserted: RasterLayer[],
  anchor: (layer: RasterLayer) => boolean,
): RasterLayer[] {
  if (inserted.length === 0) return layers
  let insertAt = -1
  for (let index = 0; index < layers.length; index += 1) {
    if (anchor(layers[index])) insertAt = index
  }
  return layers.toSpliced(insertAt + 1, 0, ...inserted)
}

function smoothstep(value: number): number {
  const t = Math.max(0, Math.min(1, value))
  return t * t * (3 - 2 * t)
}
