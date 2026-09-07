import type { Anime25DFade } from '../anime25drig/types'

/**
 * One registry owns render depth and activation channel for every semantic
 * drawing. Depths originate from Anime2.5DRig's MIT-licensed `lib/rigger.js`.
 */
export const ANIME25D_LAYER_DESCRIPTORS = {
  'back-hair': { depth: 0.55, fade: null },
  bottomwear: { depth: 0.88, fade: null },
  'collar-back': { depth: 0.94, fade: null },
  neck: { depth: 0.95, fade: null },
  'collar-front': { depth: 0.955, fade: null },
  topwear: { depth: 0.9, fade: null },
  handwear: { depth: 0.86, fade: null },
  earwear: { depth: 0.97, fade: null },
  neckwear: { depth: 1, fade: null },
  eyewear: { depth: 1.16, fade: null },
  // Recognized upstream drawings, not articulated wings/tail/object bones.
  wings: { depth: 1, fade: null },
  tail: { depth: 1, fade: null },
  objects: { depth: 1, fade: null },
  ears: { depth: 0.96, fade: null },
  face: { depth: 1, fade: null },
  facedetail: { depth: 1.02, fade: null },
  'lovestruck-face-effect': { depth: 1.04, fade: 'lovestruckFace' },
  'maniac-eye-shadow': { depth: 1.04, fade: 'maniacEyeShadow' },
  'maniac-mouth-shadow': { depth: 1.07, fade: 'maniacMouthShadow' },
  headwear: { depth: 1.2, fade: null },
  'mouth-close': { depth: 1.08, fade: 'mouthClose' },
  'mouth-open': { depth: 1.08, fade: 'mouthOpen' },
  'mouth-wide': { depth: 1.08, fade: 'mouthWide' },
  'mouth-round': { depth: 1.08, fade: 'mouthRound' },
  'mouth-narrow': { depth: 1.08, fade: 'mouthNarrow' },
  'mouth-cry': { depth: 1.08, fade: 'mouthCry' },
  'mouth-maniac': { depth: 1.08, fade: 'mouthManiac' },
  'mouth-silly': { depth: 1.08, fade: 'mouthSilly' },
  'lovestruck-drool': { depth: 1.09, fade: 'lovestruckDrool' },
  nose: { depth: 1.15, fade: null },
  eyewhite: { depth: 1.06, fade: 'eyeOpen' },
  eyebrow: { depth: 1.14, fade: null },
  irides: { depth: 1.08, fade: 'eyeOpen' },
  'lovestruck-heart': { depth: 1.1, fade: 'lovestruckHeart' },
  eyelash: { depth: 1.12, fade: 'eyeOpen' },
  'eye-close': { depth: 1.12, fade: 'eyeClose' },
  'eye-close2': { depth: 1.12, fade: 'eyeClose' },
  'eye-dizzy': { depth: 1.12, fade: 'eyeDizzy' },
  'eye-squeeze': { depth: 1.12, fade: 'eyeSqueeze' },
  'eye-cry': { depth: 1.12, fade: 'eyeCry' },
  'eye-silly-white': { depth: 1.12, fade: 'eyeSilly' },
  'iris-silly': { depth: 1.13, fade: 'eyeSilly' },
  'front-hair': { depth: 1.28, fade: null },
  'anger-mark': { depth: 1.3, fade: 'angerMark' },
  'speechless-sweat': { depth: 1.3, fade: 'speechlessSweat' },
} as const satisfies Record<
  string,
  { depth: number; fade: Anime25DFade | null }
>

export type Anime25DLayerRole = keyof typeof ANIME25D_LAYER_DESCRIPTORS

export const ANIME25D_LAYER_DEPTH = Object.fromEntries(
  Object.entries(ANIME25D_LAYER_DESCRIPTORS).map(([role, descriptor]) => [
    role,
    descriptor.depth,
  ]),
) as Record<Anime25DLayerRole, number>

export function anime25DLayerFade(role: string): Anime25DFade | null {
  return Object.hasOwn(ANIME25D_LAYER_DESCRIPTORS, role)
    ? ANIME25D_LAYER_DESCRIPTORS[role as Anime25DLayerRole].fade
    : null
}
