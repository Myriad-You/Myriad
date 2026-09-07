import { getDefaultLocale } from '../../../i18n'
import { currentCopy } from '../../../i18n/localeCopy'

/**
 * 引导的页号。0 是分岔口，1 是导入，2 起是生成链。
 *
 * 三个页面不是一条直线：从分岔口出发有两条互不相干的路，所以不要拿 `step ± 1`
 * 去推上一页/下一页——那样从生成链第一步后退会落进导入页。要前后关系就用
 * [`previousOnboardingStep`]。
 */
export type OnboardingStep = 0 | 1 | 2 | 3 | 4 | 5 | 6

/** 分岔口：选生成还是导入。 */
export const CHOICE_STEP = 0 satisfies OnboardingStep
/** 导入：现成的人设 + 现成的主立绘，一页落地。 */
export const IMPORT_STEP = 1 satisfies OnboardingStep
/** 生成链的第一页（特征词条）。 */
export const GUIDED_FIRST_STEP = 2 satisfies OnboardingStep
/** 生成链的最后一页（主立绘）。 */
export const GUIDED_LAST_STEP = 6 satisfies OnboardingStep
/** 生成链要从报告里抽词条。导入不吃这个门槛。 */
export const GUIDED_MIN_REPORTS = 3

/**
 * 上一页。`null` 表示已经在最前面，再往回就是离开引导页。
 *
 * 导入和生成链的第一页都回到分岔口——它们是从那儿分开的。
 */
export function previousOnboardingStep(
  step: OnboardingStep,
): OnboardingStep | null {
  if (step === CHOICE_STEP) return null
  if (step === IMPORT_STEP || step === GUIDED_FIRST_STEP) return CHOICE_STEP
  return (step - 1) as OnboardingStep
}

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

export type PersonaGender = 'female' | 'male' | 'nonbinary' | 'unspecified'

export const GENDER_OPTIONS: PersonaGender[] = [
  'female',
  'male',
  'nonbinary',
  'unspecified',
]

export function genderFromProfile(profile: unknown): PersonaGender | null {
  if (!profile || typeof profile !== 'object' || Array.isArray(profile)) {
    return null
  }
  const gender = (profile as Record<string, unknown>).gender
  return typeof gender === 'string' &&
    (GENDER_OPTIONS as string[]).includes(gender)
    ? (gender as PersonaGender)
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
  return `/merope/clothing/${style}.png`
}

export interface OnboardingTag {
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

/** Keep in sync with myriad-merope `CHARACTER_VISUAL_FIELDS` / `OUTFIT_VISUAL_FIELDS`. */
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

/** Keep in sync with myriad-merope `MAX_VISUAL_NOTES_CHARS`. */
export const VISUAL_NOTES_LIMIT = 1_000

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

/**
 * Resume at the first incomplete persisted asset stage.
 *
 * 只会落在生成链上：分岔口和导入是入口，不是可恢复的进度。人设一旦存下来，
 * 下次进来就该接着生成链往下走。
 */
export function completedPersonaResumeStep(
  visualProfile: unknown,
): OnboardingStep {
  if (!genderFromProfile(visualProfile)) return 3
  return visualIdentityFromProfile(visualProfile) ? 6 : 5
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
        .join(getDefaultLocale() === 'en-US' ? '; ' : '、')
    : ''
}

function personaFieldLabels() {
  const c = currentCopy().merope
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
  // ASCII commas may be part of a single English trait (for example,
  // "Blunt mouth, soft heart"), so use a delimiter parseList can distinguish.
  const joiner = getDefaultLocale() === 'en-US' ? '; ' : '、'
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
    const match = trimmed.match(/^([^：:]+)[：:](.*)$/)
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
