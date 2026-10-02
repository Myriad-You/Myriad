import type { Anime25DLayerRole } from './anime25d'
import { ANIME25D_LAYER_DEPTH } from './anime25d'

const ALIASES: Readonly<Record<string, string>> = {
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
  eyeclose: 'eye-close',
  eyeclose2: 'eye-close2',
  necklace: 'neckwear',
  pendant: 'neckwear',
  amulet: 'neckwear',
  glasses: 'eyewear',
  goggles: 'eyewear',
  earrings: 'earwear',
  'ear-ornament': 'earwear',
  hairclip: 'headwear',
  hairpin: 'headwear',
  'hair-ornament': 'headwear',
}

const BODY_ROLES = new Set([
  'neck',
  'neckwear',
  'topwear',
  'bottomwear',
  'legwear',
  'footwear',
  'handwear',
  'collar-front',
  'collar-back',
  'wings',
  'tail',
])
const RIGID_ROLES = new Set([
  'neckwear',
  'eyewear',
  'earwear',
  'headwear',
  'wings',
  'tail',
  'objects',
])
const EXTRA_FRAMING_ROLES = new Set(['unknown', 'objects', 'wings', 'tail'])
/** A bust's crop cuts these; only a full figure is framed by them. */
const LOWER_BODY_ROLES = new Set(['bottomwear', 'legwear', 'footwear'])
const FACE_SURFACE_ROLES = new Set([
  'face', 'facedetail', 'nose', 'eyewhite', 'eyelash', 'eyebrow', 'irides',
  'iris-silly', 'lovestruck-heart', 'lovestruck-face-effect', 'lovestruck-drool',
  'maniac-eye-shadow', 'maniac-mouth-shadow',
])

/** Painted features share the face's surface; earrings and floating stickers do not. */
export function anime25DLayerUsesFaceSurface(source: { group: string; role: string }): boolean {
  return source.group === 'head' && (
    FACE_SURFACE_ROLES.has(source.role) ||
    source.role.startsWith('mouth-') || source.role.startsWith('eye-')
  )
}

export function anime25DLayerAffectsFraming(
  layer: { role: string; width: number; height: number },
  documentArea: number,
  lowerBodyFrames = false,
): boolean {
  return (
    (lowerBodyFrames || !LOWER_BODY_ROLES.has(layer.role)) &&
    (!EXTRA_FRAMING_ROLES.has(layer.role) ||
      layer.width * layer.height < documentArea * 0.5)
  )
}

export function canonicalAnime25DLayerName(value: string | undefined): string {
  return (value || '')
    .normalize('NFKC')
    .trim()
    .toLowerCase()
    .replaceAll(/\s*(?:のコピー|copy)(?:\s*\d+)?$/gu, '')
    .replaceAll(/[\s_]+/g, '-')
    .replaceAll(/-+/g, '-')
}

/** Preserve every numbered/depth/side fragment */
export function anime25DLayerNameParts(value: string): {
  base: string
  suffix: string
} {
  const suffix =
    value.match(/(?:-(?:\d+|l|r|left|right|top|bottom))+$/)?.[0] ?? ''
  return { base: value.slice(0, value.length - suffix.length), suffix }
}

export function normalizeAnime25DLayerName(value: string | undefined): string {
  let name = canonicalAnime25DLayerName(value)
  if (name === 'eyelash-c') name = 'eye-close'
  if (name === 'mouth-c') name = 'mouth-close'
  if (name === 'mouth' || /^mouth-?\d+$/.test(name)) name = 'mouth-open'
  if (name === 'レイヤー-1') name = 'facedetail'
  const { base, suffix } = anime25DLayerNameParts(name)
  return (Object.hasOwn(ALIASES, base) ? ALIASES[base] : base) + suffix
}

export function anime25DBaseRole(
  normalizedName: string,
): Anime25DLayerRole | null {
  const { base } = anime25DLayerNameParts(normalizedName)
  return Object.hasOwn(ANIME25D_LAYER_DEPTH, base)
    ? (base as Anime25DLayerRole)
    : null
}

export function anime25DLayerGroup(
  role: string,
  fallback: 'head' | 'body',
): 'head' | 'body' {
  if (BODY_ROLES.has(role)) return 'body'
  if (role === 'objects' || !Object.hasOwn(ANIME25D_LAYER_DEPTH, role))
    return fallback
  return 'head'
}

export function resolveAnime25DLayerSemantics<
  T extends { name: string; role: string; group: 'head' | 'body' },
>(source: T): T {
  const role =
    source.role === 'unknown'
      ? (anime25DBaseRole(normalizeAnime25DLayerName(source.name)) ??
        source.role)
      : source.role
  const group = anime25DLayerGroup(role, source.group)
  return role === source.role && group === source.group
    ? source
    : { ...source, role, group }
}

export function isAnime25DRigidAttachment(source: {
  role: string
  fade?: string | null
  phys?: string | null
}): boolean {
  return (
    !source.fade &&
    !source.phys &&
    (RIGID_ROLES.has(source.role) || source.role === 'unknown')
  )
}
