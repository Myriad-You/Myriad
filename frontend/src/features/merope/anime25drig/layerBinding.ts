import type { HairSpringState } from './hairPhysics'
import type {
  Anime25DPlaybackAnchors,
  Anime25DPlaybackLayer,
} from './types'
import { localToAtlasUv } from './atlasUv'
import {
  frontHairUpperParallaxScale,
  hairStrandDynamics,
} from './hairPhysics'

const NECK_MESH_CELL = 28
const FRONT_COLLAR_MESH_CELL = 22

export type Anime25DLayerBindingExtension =
  | 'neck-mesh-density'
  | 'collar-mesh-density'
  | 'collar-contact-grid'
  | 'mouth-mesh-density'
  | 'cry-eye-mesh-density'
  | 'hair-length-dynamics'
  | 'front-hair-upper-parallax'

export interface Anime25DLayerSpringBinding {
  stiff: HairSpringState
  soft: HairSpringState
  phase: number
  stiffnessScale: number
  dampingScale: number
}

export interface Anime25DLayerBinding {
  rest: Float32Array
  atlasUvs: Float32Array
  indices: Uint16Array
  cols: number
  rows: number
  frontHair: boolean
  frontHairParallaxScale: Float32Array | null
  strandWeights: Float32Array | null
  alongStrand: Float32Array | null
  bangWeights: Float32Array | null
  springs: Anime25DLayerSpringBinding[] | null
  extensions: Anime25DLayerBindingExtension[]
}

export interface Anime25DLayerBindingInput {
  source: Anime25DPlaybackLayer
  canvasWidth: number
  face: Anime25DPlaybackAnchors['face']
  layerZ: number
  extraGridX?: readonly number[]
  extraGridY?: readonly number[]
}

/**
 * Pure CPU binding seam used by the enhanced player. Its unextended topology
 * follows upstream `applyRig`; every intentional Myriad addition is listed in
 * `extensions` so parity tests can distinguish replacement from drift.
 */
export function buildAnime25DLayerBinding(
  input: Readonly<Anime25DLayerBindingInput>,
): Anime25DLayerBinding {
  const { source } = input
  const extensions: Anime25DLayerBindingExtension[] = []
  const flexibleCell =
    source.role === 'neck'
      ? extension(extensions, 'neck-mesh-density', NECK_MESH_CELL)
      : source.role === 'collar-front'
        ? extension(extensions, 'collar-mesh-density', FRONT_COLLAR_MESH_CELL)
        : null
  if (input.extraGridX?.length || input.extraGridY?.length) {
    extensions.push('collar-contact-grid')
  }
  const cell =
    (flexibleCell ?? (source.phys ? 30 : 42)) *
    Math.max(0.6, input.canvasWidth / 768)
  const morphingMouth =
    source.fade === 'mouthOpen' ||
    source.fade === 'mouthWide' ||
    source.fade === 'mouthRound' ||
    source.fade === 'mouthNarrow' ||
    source.fade === 'mouthClose' ||
    source.fade === 'mouthManiac' ||
    source.fade === 'mouthSilly'
  const maniacMouthMesh = source.fade === 'mouthManiac'
  const sillyMouthMesh = source.fade === 'mouthSilly'
  if (morphingMouth) extensions.push('mouth-mesh-density')
  if (source.role === 'eye-cry') extensions.push('cry-eye-mesh-density')
  const baseCols = Math.max(
    maniacMouthMesh ? 14 : sillyMouthMesh ? 10 : morphingMouth ? 6 : 2,
    Math.round(source.w / cell),
  )
  const baseRows = Math.max(
    maniacMouthMesh
      ? 10
      : sillyMouthMesh
        ? 8
        : morphingMouth
          ? 4
          : source.role === 'eye-cry'
            ? 3
            : 2,
    Math.round(source.h / cell),
  )
  const xCoordinates = layerGridAxis(
    source.x,
    source.w,
    baseCols,
    input.extraGridX,
  )
  const yCoordinates = layerGridAxis(
    source.y,
    source.h,
    baseRows,
    input.extraGridY,
  )
  const cols = xCoordinates.length - 1
  const rows = yCoordinates.length - 1
  const rest = new Float32Array((cols + 1) * (rows + 1) * 2)
  const atlasUvs = new Float32Array(rest.length)
  let cursor = 0
  for (let row = 0; row <= rows; row += 1) {
    const y = yCoordinates[row]
    const localV = input.extraGridY?.length
      ? (y - source.y) / Math.max(1, source.h)
      : Math.fround(row / rows)
    for (let col = 0; col <= cols; col += 1) {
      const x = xCoordinates[col]
      const localU = input.extraGridX?.length
        ? (x - source.x) / Math.max(1, source.w)
        : Math.fround(col / cols)
      const [u, v] = localToAtlasUv(source.atlas, localU, localV)
      rest[cursor] = x
      rest[cursor + 1] = y
      atlasUvs[cursor] = u
      atlasUvs[cursor + 1] = v
      cursor += 2
    }
  }
  const indices = new Uint16Array(cols * rows * 6)
  let write = 0
  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < cols; col += 1) {
      const topLeft = row * (cols + 1) + col
      const topRight = topLeft + 1
      const bottomLeft = topLeft + cols + 1
      const bottomRight = bottomLeft + 1
      indices.set(
        [topLeft, topRight, bottomLeft, topRight, bottomRight, bottomLeft],
        write,
      )
      write += 6
    }
  }
  return {
    rest,
    atlasUvs,
    indices,
    cols,
    rows,
    ...bindHair(source, rest, input.face, input.layerZ, extensions),
    extensions,
  }
}

function extension<T>(
  extensions: Anime25DLayerBindingExtension[],
  name: Anime25DLayerBindingExtension,
  value: T,
): T {
  extensions.push(name)
  return value
}

function layerGridAxis(
  origin: number,
  length: number,
  segmentCount: number,
  extraCoordinates?: readonly number[],
): number[] {
  const coordinates: number[] = []
  for (let segment = 0; segment <= segmentCount; segment += 1) {
    coordinates.push(origin + (length * segment) / segmentCount)
  }
  for (const coordinate of extraCoordinates ?? []) {
    if (coordinate >= origin && coordinate <= origin + length) {
      coordinates.push(coordinate)
    }
  }
  coordinates.sort((left, right) => left - right)
  const unique: number[] = []
  for (const coordinate of coordinates) {
    if (
      unique.length === 0 ||
      Math.abs(coordinate - unique[unique.length - 1]) > 0.05
    ) {
      unique.push(coordinate)
    }
  }
  return unique
}

function bindHair(
  source: Anime25DPlaybackLayer,
  rest: Float32Array,
  face: Anime25DPlaybackAnchors['face'],
  layerZ: number,
  extensions: Anime25DLayerBindingExtension[],
): Pick<
  Anime25DLayerBinding,
  | 'frontHair'
  | 'frontHairParallaxScale'
  | 'strandWeights'
  | 'alongStrand'
  | 'bangWeights'
  | 'springs'
> {
  const frontHair = source.role === 'front-hair'
  const strands = source.strands
  if (strands.length === 0) {
    return {
      frontHair,
      frontHairParallaxScale: null,
      strandWeights: null,
      alongStrand: null,
      bangWeights: null,
      springs: null,
    }
  }
  extensions.push('hair-length-dynamics')
  if (frontHair) extensions.push('front-hair-upper-parallax')
  const strandCount = strands.length
  let spacing = 120
  if (strandCount > 1) {
    const gaps: number[] = []
    for (let index = 1; index < strandCount; index += 1) {
      gaps.push(strands[index].x - strands[index - 1].x)
    }
    gaps.sort((left, right) => left - right)
    spacing = gaps[gaps.length >> 1]
  }
  const sigma = spacing * 0.6
  const referenceHeight = Math.max(1, face.y1 - face.y0)
  const dynamics = strands.map((strand) =>
    hairStrandDynamics(strand.rootY, strand.tipY, referenceHeight),
  )
  const vertexCount = rest.length / 2
  const frontHairParallaxScale = frontHair
    ? new Float32Array(vertexCount)
    : null
  const strandWeights = new Float32Array(vertexCount * strandCount)
  const alongStrand = new Float32Array(vertexCount)
  for (let vertex = 0; vertex < vertexCount; vertex += 1) {
    const x = rest[vertex * 2]
    const y = rest[vertex * 2 + 1]
    if (frontHairParallaxScale) {
      frontHairParallaxScale[vertex] = frontHairUpperParallaxScale(
        y,
        source,
        face,
      )
    }
    let total = 0
    for (let strand = 0; strand < strandCount; strand += 1) {
      const weight = Math.exp(-(((x - strands[strand].x) / sigma) ** 2))
      strandWeights[vertex * strandCount + strand] = weight
      total += weight
    }
    let rootY = 0
    let tipY = 0
    if (total > 1e-6) {
      for (let strand = 0; strand < strandCount; strand += 1) {
        const weightIndex = vertex * strandCount + strand
        strandWeights[weightIndex] /= total
        const normalizedWeight = strandWeights[weightIndex]
        rootY += normalizedWeight * strands[strand].rootY
        tipY += normalizedWeight * strands[strand].tipY
        strandWeights[weightIndex] =
          normalizedWeight * dynamics[strand].amplitudeScale
      }
    } else {
      strandWeights[vertex * strandCount] = dynamics[0].amplitudeScale
      rootY = strands[0].rootY
      tipY = strands[0].tipY
    }
    alongStrand[vertex] = clamp(
      (y - rootY) / Math.max(1, tipY - rootY),
      0,
      1,
    )
  }
  let bangWeights: Float32Array | null = null
  if (frontHair) {
    const faceWidth = face.x1 - face.x0
    const leftSplit = face.cx - faceWidth * 0.22
    const rightSplit = face.cx + faceWidth * 0.22
    bangWeights = new Float32Array(vertexCount * 3)
    for (let vertex = 0; vertex < vertexCount; vertex += 1) {
      const x = rest[vertex * 2]
      const left = smoothstep((x - leftSplit) / 36 + 0.5)
      const right = smoothstep((x - rightSplit) / 36 + 0.5)
      bangWeights[vertex * 3] = 1 - left
      bangWeights[vertex * 3 + 1] = left * (1 - right)
      bangWeights[vertex * 3 + 2] = right
    }
  }
  return {
    frontHair,
    frontHairParallaxScale,
    strandWeights,
    alongStrand,
    bangWeights,
    springs: strands.map((_, index) => ({
      stiff: { x: 0, v: 0, dx: 0 },
      soft: { x: 0, v: 0, dx: 0 },
      phase: index * 1.37 + layerZ,
      stiffnessScale: dynamics[index].stiffnessScale,
      dampingScale: dynamics[index].dampingScale,
    })),
  }
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
