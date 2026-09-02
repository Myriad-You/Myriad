import { CHARACTER_ASSET_REQUIRED_CAPABILITIES } from './contract'

export interface Anime25DCapabilityLayer {
  id: string
  role?: string
  side?: 'left' | 'right' | null
  slot?: string | null
  variant?: string | null
}

export const ANIME25D_FACIAL_CAPABILITIES = [
  'independent-eyes',
  'blink',
  'dizzy-eye-variant',
  'squeeze-eye-variant',
  'cry-eye-variant',
  'silly-eye-variant',
  'lovestruck-heart-pupils',
  'lovestruck-face-effects',
  'cry-mouth-variant',
  'maniac-mouth-variant',
  'silly-mouth-variant',
  'mouth-shapes',
] as const

export function missingAnime25DRequiredCapabilities(
  layers: readonly Anime25DCapabilityLayer[],
): string[] {
  return CHARACTER_ASSET_REQUIRED_CAPABILITIES.filter(
    (capability) => !hasAnime25DCapability(layers, capability),
  )
}

export function hasAnime25DCapability(
  layers: readonly Anime25DCapabilityLayer[],
  capability: string,
): boolean {
  const hasLayer = (name: string) =>
    layers.some(
      (layer) => layer.role === name || matchesLayerId(layer.id, name),
    )
  const hasSideRole = (role: string, side: 'left' | 'right') =>
    layers.some((layer) => layer.role === role && layer.side === side) ||
    hasLayer(`${role}-${side}`)
  const hasVariant = (slot: string, variant: string) =>
    layers.some((layer) => layer.slot === slot && layer.variant === variant)

  switch (capability) {
    case 'separate-face':
      return hasLayer('face')
    case 'independent-eyes':
      return hasVariant('eye-left', 'open') && hasVariant('eye-right', 'open')
    case 'blink':
      return (
        hasVariant('eye-left', 'closed') && hasVariant('eye-right', 'closed')
      )
    case 'dizzy-eye-variant':
      return hasVariant('eye-left', 'dizzy') && hasVariant('eye-right', 'dizzy')
    case 'squeeze-eye-variant':
      return (
        hasVariant('eye-left', 'squeeze') && hasVariant('eye-right', 'squeeze')
      )
    case 'cry-eye-variant':
      return hasVariant('eye-left', 'cry') && hasVariant('eye-right', 'cry')
    case 'silly-eye-variant':
      return hasVariant('eye-left', 'silly') && hasVariant('eye-right', 'silly')
    case 'lovestruck-heart-pupils':
      return (
        hasSideRole('lovestruck-heart', 'left') &&
        hasSideRole('lovestruck-heart', 'right')
      )
    case 'lovestruck-face-effects':
      return hasLayer('lovestruck-face-effect') && hasLayer('lovestruck-drool')
    case 'cry-mouth-variant':
      return hasVariant('mouth', 'cry')
    case 'maniac-mouth-variant':
      return hasVariant('mouth', 'maniac')
    case 'silly-mouth-variant':
      return hasVariant('mouth', 'silly')
    case 'mouth-shapes':
      return ['closed', 'open', 'wide', 'round', 'narrow'].every((variant) =>
        hasVariant('mouth', variant),
      )
    case 'separate-front-hair':
      return hasLayer('front-hair')
    case 'separate-back-hair':
      return hasLayer('back-hair')
    case 'separate-topwear':
      return hasLayer('topwear')
    case 'rigid-left-arm-fragment':
      return hasSideRole('handwear', 'left')
    case 'rigid-right-arm-fragment':
      return hasSideRole('handwear', 'right')
    default:
      return false
  }
}

function matchesLayerId(id: string | undefined, name: string): boolean {
  if (!id) return false
  for (const prefix of [name, `a25d-${name}`]) {
    if (id === prefix || id.startsWith(`${prefix}-`)) return true
  }
  return false
}
