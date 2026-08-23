import { currentCopy } from '../../../i18n/localeCopy'
import { getDefaultLocale } from '../../../i18n'

export type OnboardingStep = 1 | 2 | 3 | 4 | 5

/** 步骤上报给二级页标题栏：说明 + 可选「换一批」 */
export interface OnboardingHeaderAction {
  label: string
  busy?: boolean
  disabled?: boolean
  onClick: () => void
}

export interface OnboardingHeaderChrome {
  description: string
  action?: OnboardingHeaderAction
  /** Return true when this page handled back and the wizard should stay. */
  onBack?: () => boolean
}

/** 设定引导二级页标题栏，由引导页合成后交给设置壳 */
export interface OnboardingPageChrome {
  title: string
  description: string
  detailTone: 'default' | 'warning'
  action?: OnboardingHeaderAction
  backDisabled: boolean
  backAria: string
  onBack: () => void
}

export type LifeGender = 'female' | 'male' | 'nonbinary' | 'unspecified'

export const GENDER_OPTIONS: LifeGender[] = [
  'female',
  'male',
  'nonbinary',
  'unspecified',
]

export function genderFromProfile(profile: unknown): LifeGender | null {
  if (!profile || typeof profile !== 'object' || Array.isArray(profile)) {
    return null
  }
  const gender = (profile as Record<string, unknown>).gender
  return typeof gender === 'string' &&
    (GENDER_OPTIONS as string[]).includes(gender)
    ? (gender as LifeGender)
    : null
}

export type NameStyle = 'chinese' | 'japanese' | 'european' | 'mythic'

export const NAME_STYLE_OPTIONS: NameStyle[] = [
  'chinese',
  'japanese',
  'european',
  'mythic',
]

export function defaultNameStyle(locale: string): NameStyle {
  if (locale.startsWith('zh')) return 'chinese'
  if (locale.startsWith('ja')) return 'japanese'
  return 'european'
}

export type ClothingStyle =
  | 'everyday'
  | 'uniform'
  | 'fantasy'
  | 'urban'
  | 'east-asian'
  | 'japanese'
  | 'sci-fi'
  | 'formal'
  | 'sport'
  | 'idol'
  | 'gothic'
  | 'lounge'
  | 'royal'
  | 'mystic'
  | 'travel'
  | 'vintage'
  | 'rain'

export const CLOTHING_STYLE_OPTIONS: ClothingStyle[] = [
  'everyday',
  'uniform',
  'fantasy',
  'urban',
  'east-asian',
  'japanese',
  'sci-fi',
  'formal',
  'sport',
  'idol',
  'gothic',
  'lounge',
  'royal',
  'mystic',
  'travel',
  'vintage',
  'rain',
]

export function clothingStylePreview(style: ClothingStyle): string {
  return `/life/clothing/${style}.png`
}

export interface LifeOnboardingTag {
  id: string
  label: string
  weight: number
}

export interface StructuredPersona {
  summary: string
  temperament: string[]
  likes: string[]
  drives: string[]
  socialStyle: string
  speechStyle: string
}

export const CHARACTER_VISUAL_KEYS = [
  'faceDesign',
  'eyeDesign',
  'hairShape',
  'hairLayerPlan',
] as const

export const OUTFIT_VISUAL_KEYS = [
  'upperBodySilhouette',
  'outfitConstruction',
  'sleeveArmDesign',
  'materialPlan',
  'heroAccessory',
  'paletteHint',
  'motif',
] as const

export const UPPER_BODY_VISUAL_IDENTITY_KEYS = [
  ...CHARACTER_VISUAL_KEYS,
  ...OUTFIT_VISUAL_KEYS,
] as const

export type CharacterVisualKey = (typeof CHARACTER_VISUAL_KEYS)[number]
export type OutfitVisualKey = (typeof OUTFIT_VISUAL_KEYS)[number]
export type UpperBodyVisualIdentityKey =
  (typeof UPPER_BODY_VISUAL_IDENTITY_KEYS)[number]

export type CharacterVisual = Record<CharacterVisualKey, string>
export type OutfitVisual = Record<OutfitVisualKey, string>

export interface UpperBodyVisualIdentity {
  character: CharacterVisual
  outfit: OutfitVisual
}

export const UPPER_BODY_VISUAL_IDENTITY_LIMITS: Record<
  UpperBodyVisualIdentityKey,
  number
> = {
  faceDesign: 500,
  eyeDesign: 500,
  hairShape: 500,
  hairLayerPlan: 700,
  upperBodySilhouette: 700,
  outfitConstruction: 1_200,
  sleeveArmDesign: 700,
  materialPlan: 1_200,
  heroAccessory: 500,
  paletteHint: 500,
  motif: 500,
}

function parseFieldGroup<K extends string>(
  source: Record<string, unknown>,
  keys: readonly K[],
): Record<K, string> | null {
  const entries = keys.map((key) => {
    const field = typeof source[key] === 'string' ? source[key].trim() : ''
    return [key, field] as const
  })
  if (entries.some(([, field]) => !field)) return null
  return Object.fromEntries(entries) as Record<K, string>
}

export function parseUpperBodyVisualIdentity(
  value: unknown,
): UpperBodyVisualIdentity | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const source = value as Record<string, unknown>
  const characterSource =
    source.character &&
    typeof source.character === 'object' &&
    !Array.isArray(source.character)
      ? (source.character as Record<string, unknown>)
      : source
  const outfitSource =
    source.outfit &&
    typeof source.outfit === 'object' &&
    !Array.isArray(source.outfit)
      ? (source.outfit as Record<string, unknown>)
      : source
  const character = parseFieldGroup(characterSource, CHARACTER_VISUAL_KEYS)
  const outfit = parseFieldGroup(outfitSource, OUTFIT_VISUAL_KEYS)
  if (!character || !outfit) return null
  return { character, outfit }
}

export function visualField(
  identity: UpperBodyVisualIdentity,
  key: UpperBodyVisualIdentityKey,
): string {
  return key in identity.character
    ? identity.character[key as CharacterVisualKey]
    : identity.outfit[key as OutfitVisualKey]
}

export function withVisualField(
  identity: UpperBodyVisualIdentity,
  key: UpperBodyVisualIdentityKey,
  value: string,
): UpperBodyVisualIdentity {
  if (key in identity.character) {
    return {
      ...identity,
      character: { ...identity.character, [key]: value },
    }
  }
  return {
    ...identity,
    outfit: { ...identity.outfit, [key]: value },
  }
}

export function visualIdentityFromProfile(
  value: unknown,
): UpperBodyVisualIdentity | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  return parseUpperBodyVisualIdentity(
    (value as Record<string, unknown>).visualIdentity,
  )
}

export function clothingStyleFromProfile(value: unknown): ClothingStyle | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const source = value as Record<string, unknown>
  const outfit =
    source.visualIdentity &&
    typeof source.visualIdentity === 'object' &&
    !Array.isArray(source.visualIdentity)
      ? (source.visualIdentity as Record<string, unknown>).outfit
      : null
  const outfitStyle =
    outfit && typeof outfit === 'object' && !Array.isArray(outfit)
      ? (outfit as Record<string, unknown>).clothingStyle
      : null
  const saved = source.clothingStyle ?? outfitStyle
  return typeof saved === 'string' &&
    (CLOTHING_STYLE_OPTIONS as string[]).includes(saved)
    ? (saved as ClothingStyle)
    : null
}

/** Resume at the first incomplete persisted asset stage. */
export function completedPersonaResumeStep(
  visualProfile: unknown,
): OnboardingStep {
  if (!genderFromProfile(visualProfile)) return 2
  return visualIdentityFromProfile(visualProfile) ? 5 : 4
}

export function emptyPersona(): StructuredPersona {
  return {
    summary: '',
    temperament: [],
    likes: [],
    drives: [],
    socialStyle: '',
    speechStyle: '',
  }
}

export function structuredPersonaIsComplete(
  persona: StructuredPersona,
): boolean {
  return incompletePersonaFields(persona).length === 0
}

export function incompletePersonaFields(
  persona: StructuredPersona,
): Array<keyof StructuredPersona> {
  const missing: Array<keyof StructuredPersona> = []
  if (persona.summary.trim().length < 8) missing.push('summary')
  if (persona.temperament.length === 0) missing.push('temperament')
  if (persona.likes.length === 0) missing.push('likes')
  if (persona.drives.length === 0) missing.push('drives')
  if (!persona.socialStyle.trim()) missing.push('socialStyle')
  if (!persona.speechStyle.trim()) missing.push('speechStyle')
  return missing
}

export function onboardingSeedsFromProfile(value: unknown): {
  sourceTags: string[]
  personaExtraRequirements: string
} {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return { sourceTags: [], personaExtraRequirements: '' }
  }
  const source = value as Record<string, unknown>
  const sourceTags = Array.isArray(source.sourceTags)
    ? source.sourceTags
        .filter((tag): tag is string => typeof tag === 'string')
        .map((tag) => tag.trim())
        .filter(Boolean)
        .slice(0, 28)
    : []
  return {
    sourceTags,
    personaExtraRequirements:
      typeof source.personaExtraRequirements === 'string'
        ? source.personaExtraRequirements
        : '',
  }
}

export function parseList(value: string): string[] {
  return value
    .split(/[、，;/|]/)
    .map((item) => item.trim())
    .filter(Boolean)
    .slice(0, 12)
}

export function joinList(value: unknown): string {
  return Array.isArray(value)
    ? value
        .filter((item): item is string => typeof item === 'string')
        .join(getDefaultLocale() === 'en-US' ? ', ' : '、')
    : ''
}

function personaFieldLabels() {
  const c = currentCopy().companion
  return {
    temperament: c.personaLabelTemperament,
    likes: c.personaLabelLikes,
    drives: c.personaLabelDrives,
    social: c.personaLabelSocial,
    speech: c.personaLabelSpeech,
  }
}

const PERSONA_PARSE_KEYS = {
  temperament: ['气质', 'Temperament', '気質'],
  likes: ['喜好', 'Likes', '好み'],
  drives: ['驱动力', 'Drive', '原動力'],
  social: ['社交', 'Social', '社交'],
  speech: ['表达', 'Voice', '話し方'],
} as const

export function flattenPersona(persona: StructuredPersona): string {
  const labels = personaFieldLabels()
  const joiner = getDefaultLocale() === 'en-US' ? ', ' : '、'
  const lines: string[] = []
  if (persona.temperament.length) {
    lines.push(`${labels.temperament}：${persona.temperament.join(joiner)}`)
  }
  if (persona.likes.length) {
    lines.push(`${labels.likes}：${persona.likes.join(joiner)}`)
  }
  if (persona.drives.length) {
    lines.push(`${labels.drives}：${persona.drives.join(joiner)}`)
  }
  if (persona.socialStyle.trim()) {
    lines.push(`${labels.social}：${persona.socialStyle.trim()}`)
  }
  if (persona.speechStyle.trim()) {
    lines.push(`${labels.speech}：${persona.speechStyle.trim()}`)
  }
  if (persona.summary.trim()) {
    if (lines.length) lines.push('')
    lines.push(persona.summary.trim())
  }
  return lines.join('\n')
}

export function parseFlattenedPersona(raw: string): StructuredPersona {
  const persona = emptyPersona()
  const leftover: string[] = []
  for (const line of raw.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed) continue
    const match = trimmed.match(/^(.+?)[：:](.*)$/)
    if (!match) {
      leftover.push(trimmed)
      continue
    }
    const [, key, rawValue] = match
    const value = rawValue.trim()
    const live = personaFieldLabels()
    if (
      PERSONA_PARSE_KEYS.temperament.includes(key as never) ||
      key === live.temperament
    ) {
      persona.temperament = parseList(value)
    } else if (
      PERSONA_PARSE_KEYS.likes.includes(key as never) ||
      key === live.likes
    ) {
      persona.likes = parseList(value)
    } else if (
      PERSONA_PARSE_KEYS.drives.includes(key as never) ||
      key === live.drives
    ) {
      persona.drives = parseList(value)
    } else if (
      PERSONA_PARSE_KEYS.social.includes(key as never) ||
      key === live.social
    ) {
      persona.socialStyle = value
    } else if (
      PERSONA_PARSE_KEYS.speech.includes(key as never) ||
      key === live.speech
    ) {
      persona.speechStyle = value
    } else {
      leftover.push(trimmed)
    }
  }
  if (leftover.length) persona.summary = leftover.join('\n')
  return persona
}

export function personaFromApi(value: unknown): StructuredPersona {
  const source =
    value && typeof value === 'object' ? (value as Record<string, unknown>) : {}
  return {
    summary: typeof source.summary === 'string' ? source.summary : '',
    temperament: parseList(joinList(source.temperament || source.traits)),
    likes: parseList(joinList(source.likes)),
    drives: parseList(joinList(source.drives)),
    socialStyle: typeof source.socialStyle === 'string' ? source.socialStyle : '',
    speechStyle:
      typeof source.speechStyle === 'string'
        ? source.speechStyle
        : typeof source.voice === 'string'
          ? source.voice
          : '',
  }
}
