import type { Layer, PixelData, Psd } from 'ag-psd'
import type { Anime25DLayerRole } from './anime25d'
import type {
  Anime25DSourceReference,
  AnimeAnchors,
  EyeSide,
  PreparedLayer,
  RasterLayer,
  RigCanvasFrame,
} from './anime25dImportTypes'
import type { MeropeRigImportSource, RigPoint } from './types'
import { currentCopy } from '../../../i18n/localeCopy'
import { analyzeAnime25DMouthProfile } from '../anime25drig/mouthProfile'
import {
  buildAnime25DPlayback,
  remapRiggerAnchors,
} from '../anime25drig/playback'
import { genericParts as GenericParts } from '../anime25drig/upstream/genericParts'
import { rigger as Rigger } from '../anime25drig/upstream/rigger'
import { ANIME25D_LAYER_DEPTH } from './anime25d'
import { validateAnime25DCharacterLayers } from './anime25dAssetValidation'
import { packAnime25DAtlas } from './anime25dAtlasCompiler'
import { splitHighCollarOcclusion } from './anime25dCollarCompiler'
import { compileAnime25DExpressionLayers } from './anime25dExpressionCompiler'
import { rasterBounds, trimRaster, uniquePartId } from './anime25dRaster'
import {
  buildAnime25DBonesAndHandles,
  buildAnime25DLayerSources,
} from './anime25dSkeletonCompiler'
import { compensateSyntheticClosedEyeAngles } from './closedEyeCompensation'
import {
  CHARACTER_ASSET_CONTRACT_VERSION,
  MAX_RIG_PARTS,
  PORTRAIT_CANVAS,
  RIG_IR_VERSION,
} from './contract'
import { inferOutfitProfileFromPartIds } from './outfit'

function genericCloseParts() {
  if (!GenericParts) return undefined
  const eyeL = GenericParts.get('eyeL')
  const eyeR = GenericParts.get('eyeR')
  const mouth = GenericParts.get('mouth')
  if (!eyeL && !mouth) return undefined
  return { eyeL, eyeR, mouth }
}

export { ANIME25D_LAYER_DEPTH, type Anime25DLayerRole } from './anime25d'

const ALPHA_COMPONENT_THRESHOLD = 16
export type { Anime25DSourceReference } from './anime25dImportTypes'

export interface PreparedAnime25DRigImport {
  atlas: Blob
  analysisReference: Blob
  source: MeropeRigImportSource
  partCount: number
}

const SEE_THROUGH_LAYER_ALIASES: Readonly<Record<string, string>> = {
  hair: 'front-hair',
  hairf: 'front-hair',
  hairb: 'back-hair',
  eyes: 'eyelash',
  eyer: 'eyelash-r',
  eyel: 'eyelash-l',
  browr: 'eyebrow-r',
  browl: 'eyebrow-l',
  earr: 'ears-r',
  earl: 'ears-l',
  eyebg: 'eyewhite',
}

const UPPER_BODY_IGNORED_LAYERS = new Set(['legwear', 'footwear'])

export function normalizeAnime25DLayerName(value: string | undefined): string {
  let name = canonicalAnime25DLayerName(value)
  if (name === 'eyelash-c') name = 'eye-close'
  if (name === 'mouth-c') name = 'mouth-close'
  if (name === 'mouth' || /^mouth-?\d+$/.test(name)) name = 'mouth-open'
  if (name === 'レイヤー-1') name = 'facedetail'
  return SEE_THROUGH_LAYER_ALIASES[name] || name
}

function canonicalAnime25DLayerName(value: string | undefined): string {
  return (value || '')
    .normalize('NFKC')
    .trim()
    .toLowerCase()
    .replace(/\s*(?:のコピー|copy)(?:\s*\d+)?$/u, '')
    .replace(/[\s_]+/g, '-')
    .replace(/-+/g, '-')
}

export function anime25DBaseRole(
  normalizedName: string,
): Anime25DLayerRole | null {
  const candidate = normalizedName.replace(/-(?:\d+|l|r|left|right)$/, '')
  return Object.hasOwn(ANIME25D_LAYER_DEPTH, candidate)
    ? (candidate as Anime25DLayerRole)
    : null
}

function anime25DLayerSide(normalizedName: string): EyeSide | null {
  const suffix = normalizedName.match(/-(l|r|left|right)$/)?.[1]
  if (suffix === 'l' || suffix === 'left') return 'left'
  if (suffix === 'r' || suffix === 'right') return 'right'
  return null
}

export function isAnime25DDocument(psd: Psd): boolean {
  const names = flattenVisibleLayers(psd.children || []).map((layer) =>
    normalizeAnime25DLayerName(layer.name),
  )
  return names.some((name) => anime25DBaseRole(name) === 'face')
}

export async function prepareAnime25DRigPsd(
  psd: Psd,
  sourceMasterAssetId: string,
  onStage?: (stage: 'validated' | 'packing') => void,
  sourceGenerationFingerprint?: string,
  sourceReference?: Anime25DSourceReference,
): Promise<PreparedAnime25DRigImport> {
  if (!isAnime25DDocument(psd)) {
    throw new Error(currentCopy().merope.anime25dMissingFace)
  }
  const staticSeeThroughMouth = hasStaticSeeThroughMouth(psd)
  const working = flattenPsdForRigger(psd)
  Rigger.cleanPsdLayers(working)
  const rig = Rigger.buildRig(working, { generic: genericCloseParts() })
  compensateSyntheticClosedEyeAngles(rig.layers)
  onStage?.('validated')
  const usedIds = new Set<string>()
  let layers = rig.layers.map((part) => rasterFromRiggerPart(part, usedIds))
  if (staticSeeThroughMouth) layers = preserveStaticMouthAsClosed(layers)
  layers = splitHandwearIfNeeded(layers, rig.anchors.face.cx)
  layers = splitVariantEyesIfNeeded(layers, rig.anchors.face.cx, 'eye-dizzy')
  layers = splitVariantEyesIfNeeded(layers, rig.anchors.face.cx, 'eye-squeeze')
  layers = splitVariantEyesIfNeeded(layers, rig.anchors.face.cx, 'eye-cry')
  layers = compileAnime25DExpressionLayers(layers, rig.anchors)
  layers = splitHighCollarOcclusion(layers, rig.anchors, sourceReference)
  layers.forEach((layer, index) => {
    layer.order = index
  })
  assignCrossfadeSlots(layers)
  validateAnime25DCharacterLayers(layers)
  const faceCenter = {
    x: rig.anchors.face.cx,
    y: rig.anchors.face.cy,
  }
  if (layers.length === 0 || layers.length > MAX_RIG_PARTS) {
    throw new Error(
      currentCopy().merope.anime25dPartCount.replace(
        '{max}',
        String(MAX_RIG_PARTS),
      ),
    )
  }
  const frame = contentFrame(psd, layers)
  onStage?.('packing')
  const {
    atlas,
    analysisReference,
    layers: prepared,
    width: packedWidth,
    height: packedHeight,
  } = await packAnime25DAtlas(frame, layers)
  const anchors = deriveAnchors(frame, prepared, faceCenter)
  const { bones, layerHandles, secondaryBoneIds } =
    buildAnime25DBonesAndHandles(prepared, anchors)
  const rigLayers = buildAnime25DLayerSources(prepared, layerHandles)
  const partIds = prepared.map((layer) => `a25d-${layer.id}`)
  const playbackAnchors = remapRiggerAnchors(rig.anchors, frame)
  const mouthProfile = analyzeAnime25DMouthProfile(
    prepared,
    frame,
    playbackAnchors.mouth,
  )
  const anime25dPlayback = buildAnime25DPlayback({
    frameWidth: frame.width,
    frameHeight: frame.height,
    layers: prepared,
    anchors: playbackAnchors,
    mouthProfile,
  })
  const outfitProfile = inferOutfitProfileFromPartIds(partIds)
  const semanticBones: Record<string, string> = {
    root: 'root',
    torso: 'body',
    head: 'head',
    face: 'face',
  }
  if (bones.some((bone) => bone.id === 'left-eye')) {
    semanticBones['left-eye'] = 'left-eye'
  }
  if (bones.some((bone) => bone.id === 'right-eye')) {
    semanticBones['right-eye'] = 'right-eye'
  }
  if (bones.some((bone) => bone.id === 'mouth')) semanticBones.mouth = 'mouth'
  if (bones.some((bone) => bone.id === 'a25d-handwear')) {
    semanticBones.handwear = 'a25d-handwear'
  }
  return {
    atlas,
    analysisReference,
    partCount: prepared.length,
    source: {
      rigIrVersion: RIG_IR_VERSION,
      characterAssetContractVersion: CHARACTER_ASSET_CONTRACT_VERSION,
      sourceMasterAssetId,
      ...(sourceGenerationFingerprint ? { sourceGenerationFingerprint } : {}),
      canvas: { ...PORTRAIT_CANVAS },
      atlas: { id: 'atlas', width: packedWidth, height: packedHeight },
      bones,
      layers: rigLayers,
      outfitProfile,
      semanticAnchors: semanticAnchors(anchors),
      semantics: {
        bones: semanticBones,
        chains: { torso: ['root', 'body', 'head'] },
        secondaryBoneIds,
      },
      anime25dPlayback,
    },
  }
}

function flattenVisibleLayers(layers: Layer[]): Layer[] {
  const output: Layer[] = []
  for (const layer of layers) {
    if (layer.hidden) continue
    if (layer.children) output.push(...flattenVisibleLayers(layer.children))
    else output.push(layer)
  }
  return output
}

/** See-through's plain `mouth` is the static portrait mouth, not an open phoneme. */
function hasStaticSeeThroughMouth(psd: Psd): boolean {
  const names = flattenVisibleLayers(psd.children || []).map((layer) =>
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

function toRiggerLayerName(value: string | undefined): string {
  let kebab = normalizeAnime25DLayerName(value)
  const numbered = kebab.match(/-(\d+)$/)
  const number = numbered?.[1]
  if (number) kebab = kebab.slice(0, -(number.length + 1))
  kebab = kebab.replace(/-(?:l|r|left|right)$/, '')
  const riggerName =
    kebab === 'front-hair'
      ? 'front hair'
      : kebab === 'back-hair'
        ? 'back hair'
        : kebab.replace(/-/g, '_')
  return number ? `${riggerName}_${number}` : riggerName
}

function flattenPsdForRigger(psd: Psd): Psd {
  const children = flattenVisibleLayers(psd.children || [])
    .filter((layer) => validPixelData(layer.imageData))
    .filter(
      (layer) =>
        !UPPER_BODY_IGNORED_LAYERS.has(normalizeAnime25DLayerName(layer.name)),
    )
    .map((layer) => {
      const pixels = layer.imageData
      if (!validPixelData(pixels)) return layer
      return {
        ...layer,
        name: toRiggerLayerName(layer.name),
        imageData: {
          width: pixels.width,
          height: pixels.height,
          data: new Uint8ClampedArray(pixels.data),
        },
      }
    })
  return { width: psd.width, height: psd.height, children }
}

function rasterFromRiggerPart(
  part: {
    name: string
    x: number
    y: number
    w: number
    h: number
    group: 'head' | 'body'
    side: 'L' | 'R' | null
    strands: Array<{ x: number; rootY: number; tipY: number }> | null
    synthetic?: boolean
    img: { width: number; height: number; data: Uint8ClampedArray }
  },
  usedIds: Set<string>,
): RasterLayer {
  const kebab = part.name.replace(/_/g, '-').replace(/ /g, '-').toLowerCase()
  const side: EyeSide | null =
    part.side === 'L'
      ? 'left'
      : part.side === 'R'
        ? 'right'
        : anime25DLayerSide(kebab)
  const role = anime25DBaseRole(kebab.replace(/-(?:l|r)$/, '')) || 'unknown'
  const preferred = side ? `${role}-${side}` : kebab.replace(/-(?:l|r)$/, '')
  return {
    id: uniquePartId(preferred, usedIds),
    role,
    sourceName: kebab,
    order: usedIds.size,
    side,
    group: part.group,
    left: part.x,
    top: part.y,
    width: part.w,
    height: part.h,
    data: part.img.data,
    synthetic: part.synthetic,
    documentStrands: part.strands || undefined,
  }
}

function preserveStaticMouthAsClosed(layers: RasterLayer[]): RasterLayer[] {
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
  output.splice(Math.min(insertAt, output.length), 0, closed)
  return output
}

function splitHandwearIfNeeded(
  layers: RasterLayer[],
  faceCenterX: number,
): RasterLayer[] {
  const output: RasterLayer[] = []
  const usedIds = new Set(layers.map((layer) => layer.id))
  for (const layer of layers) {
    if (layer.role !== 'handwear' || layer.side) {
      output.push(layer)
      continue
    }
    for (const side of ['right', 'left'] as const) {
      const split = splitRasterByComponents(layer, faceCenterX, side)
      if (!rasterBounds(split)) continue
      split.id = uniquePartId(`handwear-${side}`, usedIds)
      split.side = side
      output.push(trimRaster(split))
    }
  }
  return output
}

function splitVariantEyesIfNeeded(
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

function validPixelData(value: PixelData | undefined): value is PixelData {
  return Boolean(
    value &&
    value.width > 0 &&
    value.height > 0 &&
    (value.data instanceof Uint8Array ||
      value.data instanceof Uint8ClampedArray) &&
    value.data.length === value.width * value.height * 4,
  )
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

function assignCrossfadeSlots(layers: RasterLayer[]): void {
  for (const side of ['left', 'right'] as const) {
    const open = layers.find(
      (layer) => layer.role === 'eyelash' && layer.side === side,
    )
    const closed = layers.find(
      (layer) => layer.role === 'eye-close' && layer.side === side,
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
    if (closed) {
      closed.slot = slot
      closed.variant = 'closed'
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

/** Removes model letterboxing, then pads (never stretches) into the canonical 3:4 stage. */
function contentFrame(
  psd: Psd,
  layers: readonly RasterLayer[],
): RigCanvasFrame {
  const documentArea = psd.width * psd.height
  const framingLayers = layers.filter(
    (layer) =>
      layer.role !== 'bottomwear' &&
      (layer.role !== 'unknown' ||
        layer.width * layer.height < documentArea * 0.5),
  )
  const candidates = framingLayers.length > 0 ? framingLayers : layers
  let left = psd.width
  let top = psd.height
  let right = 0
  let bottom = 0
  for (const layer of candidates) {
    left = Math.min(left, layer.left)
    top = Math.min(top, layer.top)
    right = Math.max(right, layer.left + layer.width)
    bottom = Math.max(bottom, layer.top + layer.height)
  }
  if (right <= left || bottom <= top) {
    return { x: 0, y: 0, width: psd.width, height: psd.height }
  }
  const horizontalPadding = Math.max(4, Math.round((right - left) * 0.04))
  const verticalPadding = Math.max(4, Math.round((bottom - top) * 0.025))
  left = Math.max(0, Math.floor(left - horizontalPadding))
  top = Math.max(0, Math.floor(top - verticalPadding))
  right = Math.min(psd.width, Math.ceil(right + horizontalPadding))
  bottom = Math.min(psd.height, Math.ceil(bottom + verticalPadding))
  let width = Math.max(1, right - left)
  let height = Math.max(1, bottom - top)
  const targetAspect = PORTRAIT_CANVAS.width / PORTRAIT_CANVAS.height
  if (width / height > targetAspect) {
    const targetHeight = width / targetAspect
    top -= (targetHeight - height) / 2
    height = targetHeight
  } else {
    const targetWidth = height * targetAspect
    left -= (targetWidth - width) / 2
    width = targetWidth
  }
  return { x: left, y: top, width, height }
}

function deriveAnchors(
  frame: RigCanvasFrame,
  layers: PreparedLayer[],
  rawFaceCenter: RigPoint,
): AnimeAnchors {
  const face = requiredLayer(layers, 'face').bounds
  const center = (layer: PreparedLayer | undefined): RigPoint | undefined =>
    layer
      ? {
          x: layer.bounds.x + layer.bounds.width / 2,
          y: layer.bounds.y + layer.bounds.height / 2,
        }
      : undefined
  const eyes: Partial<Record<EyeSide, RigPoint>> = {}
  const irises: Partial<Record<EyeSide, RigPoint>> = {}
  for (const side of ['left', 'right'] as const) {
    const eye = layers.find(
      (layer) => layer.role === 'eyewhite' && layer.side === side,
    )
    const iris = layers.find(
      (layer) => layer.role === 'irides' && layer.side === side,
    )
    const fallback = layers.find(
      (layer) => layer.role === 'eyelash' && layer.side === side,
    )
    const eyeCenter = center(eye || fallback)
    if (eyeCenter) eyes[side] = eyeCenter
    const irisCenter = center(iris)
    if (irisCenter) irises[side] = irisCenter
  }
  const neckLayer = layers.find((layer) => layer.role === 'neck')
  const topwear = layers.find((layer) => layer.role === 'topwear')
  const bottomwear = layers.find((layer) => layer.role === 'bottomwear')
  const bodyReference = topwear || bottomwear || neckLayer
  const neck = neckLayer
    ? {
        x: neckLayer.bounds.x + neckLayer.bounds.width / 2,
        y: neckLayer.bounds.y + neckLayer.bounds.height * 0.85,
      }
    : {
        x: face.x + face.width / 2,
        y: face.y + face.height + 20 / frame.width,
      }
  const bodyBottom = bodyReference
    ? {
        x: bodyReference.bounds.x + bodyReference.bounds.width / 2,
        y: bodyReference.bounds.y + bodyReference.bounds.height,
      }
    : { x: 0.5, y: frame.height / frame.width }
  const mouth = center(
    layers.find((layer) => layer.role === 'mouth-open') ||
      layers.find((layer) => layer.role === 'mouth-close'),
  )
  return {
    face,
    faceCenter: {
      x: (rawFaceCenter.x - frame.x) / frame.width,
      y: (rawFaceCenter.y - frame.y) / frame.width,
    },
    neck,
    bodyBottom,
    eyes,
    irises,
    mouth: mouth || null,
  }
}

function semanticAnchors(
  anchors: AnimeAnchors,
): MeropeRigImportSource['semanticAnchors'] {
  const relative = (boneId: string, pivot: RigPoint, point: RigPoint) => ({
    boneId,
    offset: { x: point.x - pivot.x, y: point.y - pivot.y },
  })
  return {
    forehead: relative('head', anchors.neck, {
      x: anchors.face.x + anchors.face.width * 0.5,
      y: anchors.face.y + anchors.face.height * 0.18,
    }),
    'temple-right': relative('head', anchors.neck, {
      x: anchors.face.x + anchors.face.width * 0.28,
      y: anchors.face.y + anchors.face.height * 0.3,
    }),
    chin: relative('head', anchors.neck, {
      x: anchors.face.x + anchors.face.width * 0.5,
      y: anchors.face.y + anchors.face.height * 0.88,
    }),
    chest: relative('body', anchors.neck, {
      x: anchors.neck.x,
      y: anchors.neck.y + anchors.face.height * 0.42,
    }),
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

function requiredLayer(
  layers: PreparedLayer[],
  role: Anime25DLayerRole,
): PreparedLayer {
  const layer = layers.find((candidate) => candidate.role === role)
  if (!layer) {
    throw new Error(
      currentCopy().merope.anime25dMissingLayer.replace('{role}', role),
    )
  }
  return layer
}
