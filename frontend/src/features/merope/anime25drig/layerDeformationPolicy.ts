import type { Anime25DFade } from './types'
import { resolveAnime25DMouthDeformation } from './mouthDeformation'

const SHADER_ONLY_FADES = new Set<Anime25DFade>([
  'maniacEyeShadow',
  'maniacMouthShadow',
])

const LOCAL_ROLE_DEFORMATION = new Set([
  'eye_close',
  'eye_close2',
  'eye_dizzy',
  'eye_squeeze',
  'eye_cry',
  'eyebrow',
  'face',
  'nose',
  'topwear',
  'handwear',
  'neck',
  'collar_back',
  'collar_front',
])

export interface Anime25DLayerDeformationPolicy {
  shaderGlobalTransform: boolean
  localDynamic: boolean
  deformationExtensions: Anime25DLayerDeformationExtension[]
}

export type Anime25DLayerDeformationExtension =
  | 'expression-eye-geometry'
  | 'stylized-overlay-geometry'
  | 'continuous-mouth-geometry'
  | 'cry-mouth-geometry'
  | 'jaw-face-coupling'
  | 'stylized-nose-lift'
  | 'collar-contact'
  | 'neck-collar-continuity'
  | 'geometry-weighted-chest'
  | 'length-scaled-hair-physics'
  | 'front-hair-depth-release'
  | 'ellipsoid-shell'
  | 'elliptic-torso-shell'
  | 'rigid-surface-attachment'

/** Mirrors the explicit local-deformation branches owned by Anime25DPlayer. */
export function resolveAnime25DLayerDeformationPolicy(input: {
  baseRole: string
  fade: Anime25DFade | null
  hairPhysics: boolean
  hasBangWeights: boolean
  hasFrontHairParallax: boolean
  hasCollarContact: boolean
  shellDeformation?: boolean
  torsoShellDeformation?: boolean
  rigidAttachment?: boolean
}): Anime25DLayerDeformationPolicy {
  if (input.rigidAttachment) {
    return {
      shaderGlobalTransform: true,
      localDynamic: false,
      deformationExtensions: ['rigid-surface-attachment'],
    }
  }
  const coupled =
    input.shellDeformation ||
    input.torsoShellDeformation ||
    input.hasCollarContact ||
    input.hasFrontHairParallax ||
    input.hairPhysics ||
    input.hasBangWeights ||
    input.baseRole === 'neck' ||
    input.baseRole === 'collar_back' ||
    input.baseRole === 'collar_front' ||
    input.baseRole === 'topwear' ||
    input.baseRole === 'handwear'
  return {
    shaderGlobalTransform: !coupled,
    localDynamic:
      coupled ||
      LOCAL_ROLE_DEFORMATION.has(input.baseRole) ||
      Boolean(input.fade && !SHADER_ONLY_FADES.has(input.fade)),
    deformationExtensions: deformationExtensions(input),
  }
}

function deformationExtensions(input: {
  baseRole: string
  fade: Anime25DFade | null
  hairPhysics: boolean
  hasFrontHairParallax: boolean
  hasCollarContact: boolean
  shellDeformation?: boolean
  torsoShellDeformation?: boolean
}): Anime25DLayerDeformationExtension[] {
  const extensions: Anime25DLayerDeformationExtension[] = []
  const mouthDeformation = resolveAnime25DMouthDeformation(input.fade)
  if (
    input.fade === 'eyeDizzy' ||
    input.fade === 'eyeSqueeze' ||
    input.fade === 'eyeCry' ||
    input.fade === 'eyeSilly' ||
    input.fade === 'lovestruckHeart'
  ) {
    extensions.push('expression-eye-geometry')
  }
  if (
    input.fade === 'lovestruckFace' ||
    input.fade === 'lovestruckDrool' ||
    input.fade === 'angerMark' ||
    input.fade === 'speechlessSweat'
  ) {
    extensions.push('stylized-overlay-geometry')
  }
  if (mouthDeformation === 'continuous') {
    extensions.push('continuous-mouth-geometry')
  } else if (mouthDeformation === 'cry') {
    extensions.push('cry-mouth-geometry')
  }
  if (input.baseRole === 'face' || mouthDeformation) {
    extensions.push('jaw-face-coupling')
  }
  if (input.baseRole === 'nose') extensions.push('stylized-nose-lift')
  if (input.hasCollarContact) extensions.push('collar-contact')
  if (
    input.baseRole === 'neck' ||
    input.baseRole === 'collar_back' ||
    input.baseRole === 'collar_front'
  ) {
    extensions.push('neck-collar-continuity')
  }
  if (input.baseRole === 'topwear') extensions.push('geometry-weighted-chest')
  if (input.hairPhysics) extensions.push('length-scaled-hair-physics')
  if (input.hasFrontHairParallax) extensions.push('front-hair-depth-release')
  if (input.shellDeformation) extensions.push('ellipsoid-shell')
  if (input.torsoShellDeformation) extensions.push('elliptic-torso-shell')
  return extensions
}
