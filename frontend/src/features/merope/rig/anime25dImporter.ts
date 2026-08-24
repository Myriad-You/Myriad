import type { Layer, PixelData, Psd } from 'ag-psd'
import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { Anime25DLayerRole } from './anime25d'
import type {
  MeropeRigImportSource,
  RigBone,
  RigBoneHandle,
  RigLayerMeshSource,
  RigLayerSource,
  RigPoint,
  RigRect,
} from './types'
import { currentCopy } from '../../../i18n/localeCopy'
import {
  buildAnime25DPlayback,
  remapRiggerAnchors,
} from '../anime25drig/playback'
import { ANIME25D_LAYER_DEPTH } from './anime25d'
import { compensateSyntheticClosedEyeAngles } from './closedEyeCompensation'
import {
  CHARACTER_ASSET_CONTRACT_VERSION,
  MAX_RIG_BONES,
  MAX_RIG_PARTS,
  PORTRAIT_CANVAS,
  RIG_IR_VERSION,
} from './contract'
import { inferOutfitProfileFromPartIds } from './outfit'
import '../anime25drig/vendor/genericparts.js'
import '../anime25drig/vendor/rigger.js'

const Rigger = (
  globalThis as unknown as {
    Rigger: {
      buildRig: (
        psd: { width: number; height: number; children?: unknown[] },
        opts?: { generic?: unknown },
      ) => {
        layers: Array<{
          name: string
          x: number
          y: number
          w: number
          h: number
          z: number
          group: 'head' | 'body'
          side: 'L' | 'R' | null
          strands: Array<{ x: number; rootY: number; tipY: number }> | null
          synthetic?: boolean
          img: { width: number; height: number; data: Uint8ClampedArray }
        }>
        anchors: Anime25DRiggerAnchors
      }
      cleanPsdLayers: (psd: unknown) => unknown
    }
  }
).Rigger

const GenericParts = (
  globalThis as unknown as {
    GenericParts: {
      get: (key: string) => {
        width: number
        height: number
        data: Uint8ClampedArray
      } | null
    }
  }
).GenericParts

function genericCloseParts() {
  if (!GenericParts) return undefined
  const eyeL = GenericParts.get('eyeL')
  const eyeR = GenericParts.get('eyeR')
  const mouth = GenericParts.get('mouth')
  if (!eyeL && !mouth) return undefined
  return { eyeL, eyeR, mouth }
}

export { ANIME25D_LAYER_DEPTH, type Anime25DLayerRole } from './anime25d'

const ATLAS_PADDING = 8
const MAX_ATLAS_EDGE = 8192
const MIN_ATLAS_EDGE = 256
const ALPHA_COMPONENT_THRESHOLD = 16
const MIN_COMPONENT_PIXELS = 40

type EyeSide = 'left' | 'right'

interface RasterLayer {
  id: string
  role: Anime25DLayerRole | 'unknown'
  sourceName: string
  order: number
  side: EyeSide | null
  group: 'head' | 'body'
  left: number
  top: number
  width: number
  height: number
  data: Uint8ClampedArray
  slot?: 'eye-left' | 'eye-right' | 'mouth'
  variant?: 'open' | 'closed'
  documentStrands?: HairStrand[]
}

interface PreparedLayer extends RasterLayer {
  bounds: RigRect
  textureBounds: RigRect
  strands: HairStrand[]
}

interface HairStrand {
  x: number
  rootY: number
  tipY: number
}

type RigCanvasFrame = RigRect

interface AnimeAnchors {
  face: RigRect
  faceCenter: RigPoint
  neck: RigPoint
  bodyBottom: RigPoint
  eyes: Partial<Record<EyeSide, RigPoint>>
  irises: Partial<Record<EyeSide, RigPoint>>
  mouth: RigPoint | null
}

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
  let name = (value || '')
    .normalize('NFKC')
    .trim()
    .toLowerCase()
    .replace(/\s*(?:のコピー|copy)(?:\s*\d+)?$/u, '')
    .replace(/[\s_]+/g, '-')
    .replace(/-+/g, '-')
  if (name === 'eyelash-c') name = 'eye-close'
  if (name === 'mouth-c') name = 'mouth-close'
  if (name === 'mouth' || /^mouth-?\d+$/.test(name)) name = 'mouth-open'
  if (name === 'レイヤー-1') name = 'facedetail'
  return SEE_THROUGH_LAYER_ALIASES[name] || name
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
): Promise<PreparedAnime25DRigImport> {
  if (!isAnime25DDocument(psd)) {
    throw new Error(currentCopy().merope.anime25dMissingFace)
  }
  const working = flattenPsdForRigger(psd)
  Rigger.cleanPsdLayers(working)
  const rig = Rigger.buildRig(working, { generic: genericCloseParts() })
  compensateSyntheticClosedEyeAngles(rig.layers)
  onStage?.('validated')
  const usedIds = new Set<string>()
  let layers = rig.layers.map((part) => rasterFromRiggerPart(part, usedIds))
  layers = splitHandwearIfNeeded(layers, rig.anchors.face.cx)
  assignCrossfadeSlots(layers)
  validateCharacterAssetLayers(layers)
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
  } = await packAtlas(frame, layers)
  const anchors = deriveAnchors(frame, prepared, faceCenter)
  const { bones, layerHandles, secondaryBoneIds } = buildBonesAndHandles(
    prepared,
    anchors,
  )
  const rigLayers = buildLayerSources(prepared, layerHandles)
  const partIds = prepared.map((layer) => `a25d-${layer.id}`)
  const anime25dPlayback = buildAnime25DPlayback({
    frameWidth: frame.width,
    frameHeight: frame.height,
    layers: prepared,
    anchors: remapRiggerAnchors(rig.anchors, frame),
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
    documentStrands: part.strands || undefined,
  }
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

function cleanedRaster(layer: Layer): RasterLayer {
  const pixels = layer.imageData
  if (!validPixelData(pixels)) {
    throw new Error(currentCopy().merope.anime25dInvalidPixels)
  }
  const data = new Uint8ClampedArray(pixels.data)
  const opacity = clamp(layer.opacity ?? 1, 0, 1)
  for (let index = 3; index < data.length; index += 4) {
    data[index] = Math.round(data[index] * opacity)
  }
  cleanSmallComponents(data, pixels.width, pixels.height)
  return {
    id: '',
    role: 'unknown',
    sourceName: normalizeAnime25DLayerName(layer.name),
    order: 0,
    side: null,
    group: 'body',
    left: layer.left || 0,
    top: layer.top || 0,
    width: pixels.width,
    height: pixels.height,
    data,
  }
}

function buildRasterLayers(
  layers: Layer[],
  faceCenterX: number,
): RasterLayer[] {
  const output: RasterLayer[] = []
  const usedIds = new Set<string>()
  layers.forEach((source, order) => {
    const raster = cleanedRaster(source)
    const normalized = raster.sourceName
    if (UPPER_BODY_IGNORED_LAYERS.has(normalized)) return
    const role = anime25DBaseRole(normalized)
    raster.role = role || 'unknown'
    raster.order = order
    const explicitSide = anime25DLayerSide(normalized)
    if (role && explicitSide) {
      raster.side = explicitSide
      raster.id = uniquePartId(`${role}-${explicitSide}`, usedIds)
      if (rasterBounds(raster)) output.push(trimRaster(raster))
      return
    }
    if (role && splitRole(role)) {
      for (const side of ['right', 'left'] as const) {
        const split = splitRasterByComponents(raster, faceCenterX, side)
        if (!rasterBounds(split)) continue
        split.id = uniquePartId(`${role}-${side}`, usedIds)
        split.side = side
        output.push(trimRaster(split))
      }
      return
    }
    if (!rasterBounds(raster)) return
    const preferred = role
      ? normalized
      : `unknown-${safeId(normalized) || order + 1}`
    raster.id = uniquePartId(preferred, usedIds)
    output.push(trimRaster(raster))
  })
  return output
}

function splitRole(role: Anime25DLayerRole): boolean {
  return [
    'eyewhite',
    'irides',
    'eyelash',
    'eye-close',
    'eyebrow',
    'handwear',
  ].includes(role)
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
    if (!open || !closed) continue
    open.slot = side === 'left' ? 'eye-left' : 'eye-right'
    open.variant = 'open'
    closed.slot = open.slot
    closed.variant = 'closed'
  }
  const mouthOpen = layers.find((layer) => layer.role === 'mouth-open')
  const mouthClose = layers.find((layer) => layer.role === 'mouth-close')
  if (mouthOpen && mouthClose) {
    mouthOpen.slot = 'mouth'
    mouthOpen.variant = 'open'
    mouthClose.slot = 'mouth'
    mouthClose.variant = 'closed'
  }
}

function validateCharacterAssetLayers(layers: readonly RasterLayer[]): void {
  const hasRole = (role: Anime25DLayerRole) =>
    layers.some((layer) => layer.role === role)
  const hasSides = (role: Anime25DLayerRole) =>
    (['left', 'right'] as const).every((side) =>
      layers.some((layer) => layer.role === role && layer.side === side),
    )
  const missing: string[] = []
  if (!hasRole('face')) missing.push('face')
  if (!hasRole('front-hair')) missing.push('front-hair')
  if (!hasRole('back-hair')) missing.push('back-hair')
  if (!hasRole('topwear')) missing.push('topwear')
  if (!hasSides('eyelash') || !hasSides('eye-close')) {
    missing.push('independent open/closed eyes')
  }
  if (!hasRole('mouth-open') || !hasRole('mouth-close')) {
    missing.push('open/closed mouth')
  }
  if (!hasSides('handwear')) {
    missing.push('left/right sleeve-forearm-hand fragments')
  }
  if (missing.length > 0) {
    throw new Error(
      currentCopy().merope.anime25dContractMissing.replace(
        '{missing}',
        missing.join(', '),
      ),
    )
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

async function packAtlas(
  frame: RigCanvasFrame,
  layers: RasterLayer[],
): Promise<{
  atlas: Blob
  analysisReference: Blob
  layers: PreparedLayer[]
  width: number
  height: number
}> {
  const places: Array<{ x: number; y: number }> = []
  let cursorX = ATLAS_PADDING
  let cursorY = ATLAS_PADDING
  let rowHeight = 0
  let packedWidth = ATLAS_PADDING
  let packedHeight = ATLAS_PADDING
  for (const layer of layers) {
    const drawWidth = Math.max(1, layer.width)
    const drawHeight = Math.max(1, layer.height)
    if (drawWidth + ATLAS_PADDING * 2 > MAX_ATLAS_EDGE) {
      throw new Error(
        currentCopy()
          .merope.anime25dLayerTooWide.replace('{id}', layer.id)
          .replace('{max}', String(MAX_ATLAS_EDGE)),
      )
    }
    if (cursorX + drawWidth + ATLAS_PADDING > MAX_ATLAS_EDGE) {
      cursorX = ATLAS_PADDING
      cursorY += rowHeight + ATLAS_PADDING
      rowHeight = 0
    }
    if (cursorY + drawHeight + ATLAS_PADDING > MAX_ATLAS_EDGE) {
      throw new Error(
        currentCopy().merope.anime25dAtlasOverflow.replace(
          '{max}',
          String(MAX_ATLAS_EDGE),
        ),
      )
    }
    places.push({ x: cursorX, y: cursorY })
    cursorX += drawWidth + ATLAS_PADDING
    rowHeight = Math.max(rowHeight, drawHeight)
    packedWidth = Math.max(packedWidth, cursorX)
    packedHeight = Math.max(packedHeight, cursorY + drawHeight + ATLAS_PADDING)
  }
  packedWidth = Math.max(MIN_ATLAS_EDGE, packedWidth)
  packedHeight = Math.max(MIN_ATLAS_EDGE, packedHeight)
  const atlas = document.createElement('canvas')
  atlas.width = packedWidth
  atlas.height = packedHeight
  const context = requiredContext(atlas)
  const analysisCanvas = document.createElement('canvas')
  analysisCanvas.width = Math.max(1, Math.round(frame.width))
  analysisCanvas.height = Math.max(1, Math.round(frame.height))
  const analysisContext = requiredContext(analysisCanvas)
  const analysisScaleX = analysisCanvas.width / Math.max(1, frame.width)
  const analysisScaleY = analysisCanvas.height / Math.max(1, frame.height)
  const layerCanvas = document.createElement('canvas')
  const prepared: PreparedLayer[] = []
  for (const [index, layer] of layers.entries()) {
    const drawX = places[index].x
    const drawY = places[index].y
    layerCanvas.width = layer.width
    layerCanvas.height = layer.height
    const imageBytes = new Uint8ClampedArray(layer.data.length)
    imageBytes.set(layer.data)
    requiredContext(layerCanvas).putImageData(
      new ImageData(imageBytes, layer.width, layer.height),
      0,
      0,
    )
    context.drawImage(layerCanvas, drawX, drawY)
    if (visibleInAnalysisReference(layer)) {
      analysisContext.drawImage(
        layerCanvas,
        (layer.left - frame.x) * analysisScaleX,
        (layer.top - frame.y) * analysisScaleY,
        layer.width * analysisScaleX,
        layer.height * analysisScaleY,
      )
    }
    const bounds = {
      x: (layer.left - frame.x) / frame.width,
      y: (layer.top - frame.y) / frame.width,
      width: layer.width / frame.width,
      height: layer.height / frame.width,
    }
    prepared.push({
      ...layer,
      bounds,
      textureBounds: {
        x: drawX / packedWidth,
        y: drawY / packedHeight,
        width: layer.width / packedWidth,
        height: layer.height / packedHeight,
      },
      strands:
        layer.role === 'front-hair' || layer.role === 'back-hair'
          ? (layer.documentStrands || []).map((strand) => ({
              x: (strand.x - frame.x) / frame.width,
              rootY: (strand.rootY - frame.y) / frame.width,
              tipY: (strand.tipY - frame.y) / frame.width,
            }))
          : [],
    })
  }
  const [blob, analysisReference] = await Promise.all([
    canvasPng(atlas),
    canvasPng(analysisCanvas),
  ])
  return {
    atlas: blob,
    analysisReference,
    layers: prepared,
    width: packedWidth,
    height: packedHeight,
  }
}

function visibleInAnalysisReference(layer: RasterLayer): boolean {
  if (
    (layer.slot === 'eye-left' || layer.slot === 'eye-right') &&
    layer.variant === 'closed'
  ) {
    return false
  }
  return layer.slot !== 'mouth' || layer.variant !== 'open'
}

function canvasPng(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise<Blob>((resolve, reject) =>
    canvas.toBlob(
      (value) =>
        value
          ? resolve(value)
          : reject(new Error(currentCopy().merope.rigAtlasFailed)),
      'image/png',
    ),
  )
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

function buildBonesAndHandles(
  layers: PreparedLayer[],
  anchors: AnimeAnchors,
): {
  bones: RigBone[]
  layerHandles: Map<string, RigBoneHandle[]>
  secondaryBoneIds: string[]
} {
  const bones: RigBone[] = [
    { id: 'root', parent: null, pivot: anchors.bodyBottom },
    { id: 'body', parent: 'root', pivot: anchors.neck },
    { id: 'head', parent: 'body', pivot: anchors.neck },
    { id: 'face', parent: 'head', pivot: anchors.faceCenter },
  ]
  const ensureBone = (id: string, parent: string, pivot: RigPoint): string => {
    if (!bones.some((bone) => bone.id === id)) bones.push({ id, parent, pivot })
    return id
  }
  for (const side of ['left', 'right'] as const) {
    const eye = anchors.eyes[side]
    if (!eye) continue
    ensureBone(`${side}-eye`, 'face', eye)
    if (
      layers.some((layer) => layer.side === side && layer.role === 'eyewhite')
    ) {
      ensureBone(`a25d-eyewhite-${side}`, 'face', eye)
    }
    if (anchors.irises[side]) {
      ensureBone(`a25d-irides-${side}`, `${side}-eye`, anchors.irises[side]!)
    }
    if (
      layers.some(
        (layer) =>
          layer.side === side &&
          (layer.role === 'eyelash' || layer.role === 'eye-close'),
      )
    ) {
      ensureBone(`a25d-eyelash-${side}`, 'face', eye)
    }
    ensureBone(`a25d-eyebrow-${side}`, 'face', {
      x: eye.x,
      y: eye.y - anchors.face.height * 0.12,
    })
  }
  if (anchors.mouth) ensureBone('mouth', 'face', anchors.mouth)
  if (layers.some((layer) => layer.role === 'topwear')) {
    ensureBone('a25d-chest', 'body', {
      x: anchors.neck.x,
      y: anchors.neck.y + anchors.face.height * 0.35,
    })
  }
  if (layers.some((layer) => layer.role === 'handwear')) {
    const handwearLayers = layers.filter((layer) => layer.role === 'handwear')
    const handwear = unionLayerBounds(handwearLayers)
    ensureBone('a25d-handwear', 'body', {
      x: handwear.x + handwear.width / 2,
      y: handwear.y + handwear.height * 0.22,
    })
    for (const side of ['left', 'right'] as const) {
      const sideLayer = handwearLayers.find((layer) => layer.side === side)
      if (!sideLayer) continue
      ensureBone(`a25d-handwear-${side}`, 'a25d-handwear', {
        x: sideLayer.bounds.x + sideLayer.bounds.width / 2,
        y: sideLayer.bounds.y + sideLayer.bounds.height * 0.16,
      })
    }
  }
  const independentHeadRoles: Anime25DLayerRole[] = [
    'nose',
    'ears',
    'earwear',
    'headwear',
    'facedetail',
  ]
  for (const role of independentHeadRoles) {
    const layer = layers.find((candidate) => candidate.role === role)
    if (!layer) continue
    ensureBone(`a25d-${role}`, 'head', rectCenter(layer.bounds))
  }

  const secondaryBoneIds: string[] = []
  const hairBones = new Map<string, RigBoneHandle[]>()
  let availableStrands = Math.max(
    0,
    Math.floor((MAX_RIG_BONES - bones.length) / 2),
  )
  for (const layer of layers.filter(
    (candidate) =>
      candidate.role === 'front-hair' || candidate.role === 'back-hair',
  )) {
    const selected = layer.strands.slice(0, availableStrands)
    availableStrands -= selected.length
    const handles: RigBoneHandle[] = []
    const spacing = layer.bounds.width / Math.max(2, selected.length)
    selected.forEach((strand, index) => {
      const prefix = `a25d-${layer.id}-strand-${index + 1}`
      const root = `${prefix}-hair-root`
      const tip = `${prefix}-hair-tip`
      const midY = strand.rootY + (strand.tipY - strand.rootY) * 0.48
      bones.push({
        id: root,
        parent: 'head',
        pivot: { x: strand.x, y: strand.rootY },
      })
      bones.push({ id: tip, parent: root, pivot: { x: strand.x, y: midY } })
      secondaryBoneIds.push(root, tip)
      handles.push(
        {
          boneId: root,
          start: { x: strand.x, y: strand.rootY },
          end: { x: strand.x, y: midY },
          falloff: Math.max(0.025, spacing * 0.95),
        },
        {
          boneId: tip,
          start: { x: strand.x, y: midY },
          end: { x: strand.x, y: strand.tipY },
          falloff: Math.max(0.025, spacing * 0.82),
        },
      )
    })
    if (handles.length === 0) handles.push(fullLayerHandle(layer, 'head'))
    hairBones.set(layer.id, handles)
  }
  if (bones.length > MAX_RIG_BONES) {
    throw new Error(
      currentCopy().merope.anime25dBoneLimit.replace(
        '{max}',
        String(MAX_RIG_BONES),
      ),
    )
  }

  const layerHandles = new Map<string, RigBoneHandle[]>()
  for (const layer of layers) {
    const hair = hairBones.get(layer.id)
    if (hair) {
      layerHandles.set(layer.id, hair)
      continue
    }
    layerHandles.set(layer.id, handlesForLayer(layer, bones))
  }
  return { bones, layerHandles, secondaryBoneIds }
}

function handlesForLayer(
  layer: PreparedLayer,
  bones: RigBone[],
): RigBoneHandle[] {
  const has = (id: string) => bones.some((bone) => bone.id === id)
  const side = layer.side
  if (layer.role === 'face') return [fullLayerHandle(layer, 'face')]
  if (side && layer.role === 'eyewhite' && has(`a25d-eyewhite-${side}`)) {
    return [fullLayerHandle(layer, `a25d-eyewhite-${side}`)]
  }
  if (
    side &&
    (layer.role === 'eyelash' || layer.role === 'eye-close') &&
    has(`a25d-eyelash-${side}`)
  ) {
    return [fullLayerHandle(layer, `a25d-eyelash-${side}`)]
  }
  if (side && layer.role === 'irides' && has(`a25d-irides-${side}`)) {
    return [fullLayerHandle(layer, `a25d-irides-${side}`)]
  }
  if (side && layer.role === 'eyebrow' && has(`a25d-eyebrow-${side}`)) {
    return [fullLayerHandle(layer, `a25d-eyebrow-${side}`)]
  }
  if (
    (layer.role === 'mouth-open' || layer.role === 'mouth-close') &&
    has('mouth')
  ) {
    return [fullLayerHandle(layer, 'mouth')]
  }
  if (layer.role === 'topwear' && has('a25d-chest')) {
    return verticalBlendHandles(layer, 'body', 'a25d-chest', 0.58)
  }
  if (layer.role === 'neck') {
    return verticalBlendHandles(layer, 'head', 'body', 0.52)
  }
  if (layer.role === 'handwear' && has('a25d-handwear')) {
    const sideBone = layer.side ? `a25d-handwear-${layer.side}` : ''
    return [
      fullLayerHandle(
        layer,
        sideBone && has(sideBone) ? sideBone : 'a25d-handwear',
      ),
    ]
  }
  if (layer.role === 'bottomwear') return [fullLayerHandle(layer, 'root')]
  const dedicated = `a25d-${layer.role}`
  if (has(dedicated)) return [fullLayerHandle(layer, dedicated)]
  const group =
    layer.bounds.y + layer.bounds.height / 2 < 0.62 ? 'head' : 'body'
  return [fullLayerHandle(layer, group)]
}

function fullLayerHandle(layer: PreparedLayer, boneId: string): RigBoneHandle {
  return {
    boneId,
    start: {
      x: layer.bounds.x + layer.bounds.width / 2,
      y: layer.bounds.y,
    },
    end: {
      x: layer.bounds.x + layer.bounds.width / 2,
      y: layer.bounds.y + layer.bounds.height,
    },
    falloff: Math.max(layer.bounds.width, layer.bounds.height, 0.02),
  }
}

function verticalBlendHandles(
  layer: PreparedLayer,
  topBone: string,
  bottomBone: string,
  split: number,
): RigBoneHandle[] {
  const centerX = layer.bounds.x + layer.bounds.width / 2
  const middleY = layer.bounds.y + layer.bounds.height * split
  const falloff = Math.max(
    layer.bounds.width * 0.68,
    layer.bounds.height * 0.38,
  )
  return [
    {
      boneId: topBone,
      start: { x: centerX, y: layer.bounds.y },
      end: { x: centerX, y: middleY },
      falloff,
    },
    {
      boneId: bottomBone,
      start: { x: centerX, y: middleY },
      end: { x: centerX, y: layer.bounds.y + layer.bounds.height },
      falloff,
    },
  ]
}

function buildLayerSources(
  layers: PreparedLayer[],
  handles: Map<string, RigBoneHandle[]>,
): RigLayerSource[] {
  return layers.map((layer) => {
    const mesh = gridMesh(layer.bounds, layer.role)
    const depth =
      layer.role === 'unknown' ? 1 : ANIME25D_LAYER_DEPTH[layer.role]
    return {
      id: `a25d-${layer.id}`,
      textureId: 'atlas',
      textureBounds: layer.textureBounds,
      // ag-psd exposes this PSD bottom-to-top. Anime2.5DRig deliberately
      // overrides that order with its semantic depth table, then keeps the PSD
      // order as a stable tie-break for numbered/repeated layers.
      zIndex: Math.round(depth * 100) * 100 + Math.round(layer.order),
      opacity: 1,
      slot: layer.slot,
      variant: layer.variant,
      contours: [],
      mesh,
      boneHandles: handles.get(layer.id) || [fullLayerHandle(layer, 'body')],
    }
  })
}

export function gridMesh(
  bounds: RigRect,
  role: Anime25DLayerRole | 'unknown',
): RigLayerMeshSource {
  const deformable =
    role === 'front-hair' ||
    role === 'back-hair' ||
    role === 'topwear' ||
    role === 'neck'
  const cell = deformable ? 0.035 : 0.075
  const columns = clampInt(
    Math.ceil(bounds.width / cell),
    2,
    deformable ? 14 : 8,
  )
  const rows = clampInt(Math.ceil(bounds.height / cell), 2, deformable ? 18 : 8)
  const vertices: RigPoint[] = []
  const indices: number[] = []
  for (let row = 0; row <= rows; row += 1) {
    for (let column = 0; column <= columns; column += 1) {
      vertices.push({
        x: bounds.x + (bounds.width * column) / columns,
        y: bounds.y + (bounds.height * row) / rows,
      })
    }
  }
  for (let row = 0; row < rows; row += 1) {
    for (let column = 0; column < columns; column += 1) {
      const topLeft = row * (columns + 1) + column
      const topRight = topLeft + 1
      const bottomLeft = topLeft + columns + 1
      const bottomRight = bottomLeft + 1
      indices.push(
        topLeft,
        topRight,
        bottomLeft,
        topRight,
        bottomRight,
        bottomLeft,
      )
    }
  }
  return { vertices, indices }
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

function detectHairStrands(
  layer: RasterLayer,
  frame: RigCanvasFrame,
): HairStrand[] {
  const bottom = new Float32Array(layer.width)
  const top = new Float32Array(layer.width)
  let minX = layer.width
  let maxX = -1
  for (let x = 0; x < layer.width; x += 1) {
    top[x] = -1
    for (let y = 0; y < layer.height; y += 1) {
      if (
        layer.data[(y * layer.width + x) * 4 + 3] <= ALPHA_COMPONENT_THRESHOLD
      )
        continue
      if (top[x] < 0) top[x] = y
      bottom[x] = y
    }
    if (top[x] >= 0) {
      minX = Math.min(minX, x)
      maxX = Math.max(maxX, x)
    }
  }
  if (maxX < minX) return []
  const smoothRadius = Math.min(20, Math.max(3, Math.floor(layer.width / 24)))
  const smoothed = smoothProfile(bottom, smoothRadius)
  const numbered = /-\d+$/.test(layer.sourceName)
  const wanted = numbered ? clampInt(Math.round(layer.width / 110), 2, 6) : 6
  const peaks = findProfilePeaks(
    smoothed,
    Math.max(30, Math.round(layer.width / (wanted * 1.6))),
  )
  const xs = peaks.slice(0, wanted)
  while (xs.length < wanted) {
    let best = -1
    let bestDistance = -1
    for (let sample = 0; sample < 48; sample += 1) {
      const x = Math.round(minX + ((maxX - minX) * sample) / 47)
      if (top[x] < 0 || xs.includes(x)) continue
      const distance =
        xs.length === 0
          ? Number.MAX_SAFE_INTEGER
          : Math.min(...xs.map((value) => Math.abs(x - value)))
      if (distance > bestDistance) {
        best = x
        bestDistance = distance
      }
    }
    if (best < 0) break
    xs.push(best)
  }
  return xs
    .sort((left, right) => left - right)
    .filter((x) => top[x] >= 0)
    .map((x) => ({
      x: (layer.left + x - frame.x) / frame.width,
      rootY: (layer.top + top[x] - frame.y) / frame.width,
      tipY: (layer.top + bottom[x] - frame.y) / frame.width,
    }))
}

function smoothProfile(values: Float32Array, radius: number): Float32Array {
  const prefix = new Float32Array(values.length + 1)
  for (let index = 0; index < values.length; index += 1) {
    prefix[index + 1] = prefix[index] + values[index]
  }
  const output = new Float32Array(values.length)
  for (let index = 0; index < values.length; index += 1) {
    const start = Math.max(0, index - radius)
    const end = Math.min(values.length - 1, index + radius)
    output[index] = (prefix[end + 1] - prefix[start]) / (end - start + 1)
  }
  return output
}

function findProfilePeaks(values: Float32Array, minDistance: number): number[] {
  const candidates: Array<{ x: number; prominence: number }> = []
  for (let index = 1; index < values.length - 1; index += 1) {
    if (values[index] <= values[index - 1] || values[index] < values[index + 1])
      continue
    let leftMin = values[index]
    let rightMin = values[index]
    for (let left = index - 1; left >= 0; left -= 1) {
      if (values[left] > values[index]) break
      leftMin = Math.min(leftMin, values[left])
    }
    for (let right = index + 1; right < values.length; right += 1) {
      if (values[right] > values[index]) break
      rightMin = Math.min(rightMin, values[right])
    }
    const prominence = values[index] - Math.max(leftMin, rightMin)
    if (prominence >= 10) candidates.push({ x: index, prominence })
  }
  candidates.sort((left, right) => right.prominence - left.prominence)
  const selected: number[] = []
  for (const candidate of candidates) {
    if (
      selected.every((value) => Math.abs(value - candidate.x) >= minDistance)
    ) {
      selected.push(candidate.x)
    }
  }
  return selected
}

function cleanSmallComponents(
  data: Uint8ClampedArray,
  width: number,
  height: number,
): void {
  const components = labelAlphaComponents(data, width, height)
  if (components.count === 0) return
  const hasLarge = components.sizes.some(
    (size, index) => index > 0 && size >= MIN_COMPONENT_PIXELS,
  )
  if (!hasLarge) return
  for (let pixel = 0; pixel < components.labels.length; pixel += 1) {
    if (components.sizes[components.labels[pixel]] < MIN_COMPONENT_PIXELS) {
      data[pixel * 4 + 3] = 0
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

function rasterBounds(layer: RasterLayer): RigRect | null {
  let minX = layer.width
  let minY = layer.height
  let maxX = -1
  let maxY = -1
  for (let y = 0; y < layer.height; y += 1) {
    for (let x = 0; x < layer.width; x += 1) {
      if (layer.data[(y * layer.width + x) * 4 + 3] <= 8) continue
      minX = Math.min(minX, x)
      minY = Math.min(minY, y)
      maxX = Math.max(maxX, x)
      maxY = Math.max(maxY, y)
    }
  }
  return maxX < minX
    ? null
    : {
        x: minX,
        y: minY,
        width: maxX - minX + 1,
        height: maxY - minY + 1,
      }
}

function rasterCentroid(layer: RasterLayer): RigPoint | null {
  let sumX = 0
  let sumY = 0
  let sum = 0
  for (let y = 0; y < layer.height; y += 1) {
    for (let x = 0; x < layer.width; x += 1) {
      const alpha = layer.data[(y * layer.width + x) * 4 + 3]
      if (alpha === 0) continue
      sumX += (layer.left + x) * alpha
      sumY += (layer.top + y) * alpha
      sum += alpha
    }
  }
  return sum > 0 ? { x: sumX / sum, y: sumY / sum } : null
}

function trimRaster(layer: RasterLayer): RasterLayer {
  const bounds = rasterBounds(layer)
  if (!bounds) return layer
  const padding = 2
  const left = Math.max(0, Math.floor(bounds.x) - padding)
  const top = Math.max(0, Math.floor(bounds.y) - padding)
  const right = Math.min(
    layer.width,
    Math.ceil(bounds.x + bounds.width) + padding,
  )
  const bottom = Math.min(
    layer.height,
    Math.ceil(bounds.y + bounds.height) + padding,
  )
  const width = right - left
  const height = bottom - top
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y += 1) {
    const start = ((y + top) * layer.width + left) * 4
    data.set(layer.data.subarray(start, start + width * 4), y * width * 4)
  }
  return {
    ...layer,
    left: layer.left + left,
    top: layer.top + top,
    width,
    height,
    data,
  }
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

function unionLayerBounds(layers: PreparedLayer[]): RigRect {
  if (layers.length === 0) {
    throw new Error(currentCopy().merope.anime25dMissingHandwear)
  }
  const left = Math.min(...layers.map((layer) => layer.bounds.x))
  const top = Math.min(...layers.map((layer) => layer.bounds.y))
  const right = Math.max(
    ...layers.map((layer) => layer.bounds.x + layer.bounds.width),
  )
  const bottom = Math.max(
    ...layers.map((layer) => layer.bounds.y + layer.bounds.height),
  )
  return { x: left, y: top, width: right - left, height: bottom - top }
}

function requiredContext(canvas: HTMLCanvasElement): CanvasRenderingContext2D {
  const context = canvas.getContext('2d', { willReadFrequently: true })
  if (!context) throw new Error(currentCopy().merope.canvasUnsupported)
  return context
}

function uniquePartId(preferred: string, used: Set<string>): string {
  let id = preferred
  let suffix = 2
  while (used.has(id)) id = `${preferred}-${suffix++}`
  used.add(id)
  return id
}

function safeId(value: string): string {
  return value.replace(/[^a-z0-9-]+/g, '-').replace(/^-|-$/g, '')
}

function rectCenter(rect: RigRect): RigPoint {
  return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function clampInt(value: number, minimum: number, maximum: number): number {
  return Math.round(clamp(value, minimum, maximum))
}
