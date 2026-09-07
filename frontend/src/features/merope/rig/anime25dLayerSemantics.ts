import type { Anime25DLayerRole } from './anime25d'
import { ANIME25D_LAYER_DEPTH } from './anime25d'

/**
 * See-through bodytags_v3 categories plus the established native PSD aliases.
 * https://huggingface.co/spaces/24yearsold/see-through-demo/blob/main/common/assets/bodytags_v3.json
 * Only exact tokens are aliases: a "necklace-shadow" is not a necklace.
 */
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
// Recognizing an extra drawing must not promote a canvas-sized prop into the
// portrait's framing authority. Small accessories still retain their margins.
const EXTRA_FRAMING_ROLES = new Set(['unknown', 'objects', 'wings', 'tail'])

export function anime25DLayerAffectsFraming(
  layer: { role: string; width: number; height: number },
  documentArea: number,
): boolean {
  return (
    layer.role !== 'bottomwear' &&
    (!EXTRA_FRAMING_ROLES.has(layer.role) ||
      layer.width * layer.height < documentArea * 0.5)
  )
}

export function canonicalAnime25DLayerName(value: string | undefined): string {
  return (value || '')
    .normalize('NFKC')
    .trim()
    .toLowerCase()
    .replace(/\s*(?:のコピー|copy)(?:\s*\d+)?$/u, '')
    .replace(/[\s_]+/g, '-')
    .replace(/-+/g, '-')
}

/** Preserve every numbered/depth/side fragment; strip suffixes only for lookup. */
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
  return (ALIASES[base] ?? base) + suffix
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

/**
 * Used at import and binding, including named drawings stored as "unknown".
 * Does not change the immutable asset, its draw order, UVs, or layer identity.
 */
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
  // Unknown art stays drawable, but does not gain cloth/chest deformation or
  // turn into an articulated limb. Its existing head/body group owns the mount.
  return (
    !source.fade &&
    !source.phys &&
    (RIGID_ROLES.has(source.role) || source.role === 'unknown')
  )
}
