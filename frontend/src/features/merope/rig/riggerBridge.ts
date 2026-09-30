import type { Layer, PixelData, Psd } from 'ag-psd'
import type { EyeSide, RasterLayer } from './anime25dImportTypes'
import { genericParts as GenericParts } from '../anime25drig/upstream/genericParts'
import { anime25DBaseRole, anime25DLayerGroup, anime25DLayerNameParts, canonicalAnime25DLayerName, isAnime25DRigidAttachment, normalizeAnime25DLayerName } from './anime25dLayerSemantics'
import { uniquePartId } from './anime25dRaster'

export function genericCloseParts() {
  if (!GenericParts) return undefined
  const eyeL = GenericParts.get('eyeL')
  const eyeR = GenericParts.get('eyeR')
  const mouth = GenericParts.get('mouth')
  if (!eyeL && !mouth) return undefined
  return { eyeL, eyeR, mouth }
}

const UPPER_BODY_IGNORED_LAYERS = new Set(['footwear'])
/**
 * A long garment's visible front is often labelled legwear (a kimono or long
 * skirt over the legs). Kept as lower-body clothing it paints where it was
 * drawn and the portrait crop cuts it; dropped, it bares the flat fill a
 * decomposer paints behind it. Feet are always far below the crop.
 */
const UPPER_BODY_AS_BOTTOMWEAR = new Set(['legwear'])

function anime25DLayerSide(normalizedName: string): EyeSide | null {
  const suffix = anime25DLayerNameParts(normalizedName).suffix.match(
    /-(l|r|left|right)(?:-|$)/,
  )?.[1]
  if (suffix === 'l' || suffix === 'left') return 'left'
  if (suffix === 'r' || suffix === 'right') return 'right'
  return null
}

export function flattenVisibleLayers(layers: Layer[], parentOpacity = 1): Layer[] {
  const output: Layer[] = []
  for (const layer of layers) {
    if (layer.hidden) continue
    const opacity =
      parentOpacity *
      (Number.isFinite(layer.opacity)
        ? Math.max(0, Math.min(1, layer.opacity!))
        : 1)
    if (opacity === 0) continue
    if (layer.children)
      output.push(...flattenVisibleLayers(layer.children, opacity))
    else output.push({ ...layer, opacity })
  }
  return output
}

function toRiggerLayerName(value: string | undefined): string {
  let kebab = normalizeAnime25DLayerName(value)
  // Extra drawings retain their full source identity instead of losing depth/side fragment suffixes.
  const role = anime25DBaseRole(kebab)
  if (isAnime25DRigidAttachment({ role: role ?? 'unknown' })) {
    return kebab.replaceAll('-', '_')
  }
  const numbered = kebab.match(/-(\d+)$/)
  const number = numbered?.[1]
  if (number) kebab = kebab.slice(0, -(number.length + 1))
  kebab = kebab.replaceAll(/-(?:l|r|left|right)$/g, '')
  const riggerName =
    kebab === 'front-hair'
      ? 'front hair'
      : kebab === 'back-hair'
        ? 'back hair'
        : kebab.replaceAll('-', '_')
  return number ? `${riggerName}_${number}` : riggerName
}

export function flattenPsdForRigger(psd: Psd): Psd {
  const visible = flattenVisibleLayers(psd.children ?? [])
    .filter((layer) => validPixelData(layer.imageData))
    .filter(
      (layer) =>
        !UPPER_BODY_IGNORED_LAYERS.has(
          anime25DLayerNameParts(normalizeAnime25DLayerName(layer.name)).base,
        ),
    )
  const face = visible.findIndex(
    (layer) =>
      anime25DBaseRole(normalizeAnime25DLayerName(layer.name)) === 'face',
  )
  const children = visible.map((layer, index) => {
    const pixels = layer.imageData
    if (!validPixelData(pixels)) return layer
    const data = new Uint8ClampedArray(pixels.data)
    const opacity = layer.opacity ?? 1
    if (opacity !== 1) {
      for (let offset = 3; offset < data.length; offset += 4)
        data[offset] *= opacity
    }
    const base = anime25DLayerNameParts(normalizeAnime25DLayerName(layer.name)).base
    return {
      ...layer,
      opacity: 1,
      name: UPPER_BODY_AS_BOTTOMWEAR.has(base)
        ? 'bottomwear'
        : toRiggerLayerName(plainHairBySide(layer.name, index, face)),
      imageData: {
        width: pixels.width,
        height: pixels.height,
        data,
      },
    }
  })
  return { width: psd.width, height: psd.height, children }
}

/**
 * A plain `hair` layer names no side of the face. As in upstream 7ddbd99,
 * the painter's order decides: above the face it is front hair, below it back.
 */
function plainHairBySide(
  name: string | undefined,
  index: number,
  face: number,
): string | undefined {
  const { base, suffix } = anime25DLayerNameParts(
    canonicalAnime25DLayerName(name),
  )
  if (base !== 'hair' || face < 0) return name
  return `${index > face ? 'front-hair' : 'back-hair'}${suffix}`
}

export function rasterFromRiggerPart(
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
  const kebab = part.name.replaceAll('_', '-').replaceAll(' ', '-').toLowerCase()
  const side: EyeSide | null =
    part.side === 'L'
      ? 'left'
      : part.side === 'R'
        ? 'right'
        : anime25DLayerSide(kebab)
  const role = anime25DBaseRole(kebab.replaceAll(/-(?:l|r)$/g, '')) || 'unknown'
  const numbered = /-\d+(?:-|$)/.test(anime25DLayerNameParts(kebab).suffix)
  const preferred = numbered
    ? kebab
    : side
      ? `${role}-${side}`
      : kebab.replaceAll(/-(?:l|r)$/g, '')
  return {
    id: uniquePartId(preferred, usedIds),
    role,
    sourceName: kebab,
    order: usedIds.size,
    side,
    group: anime25DLayerGroup(role, part.group),
    left: part.x,
    top: part.y,
    width: part.w,
    height: part.h,
    data: part.img.data,
    synthetic: part.synthetic,
    documentStrands: part.strands ?? undefined,
  }
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
