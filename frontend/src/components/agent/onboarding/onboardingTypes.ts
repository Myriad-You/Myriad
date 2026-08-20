export type OnboardingStep = 1 | 2 | 3

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

export function parseList(value: string): string[] {
  return value
    .split(/[、，;/|]/)
    .map((item) => item.trim())
    .filter(Boolean)
    .slice(0, 12)
}

export function joinList(value: unknown): string {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string').join('、')
    : ''
}

export function flattenPersona(persona: StructuredPersona): string {
  const lines: string[] = []
  if (persona.temperament.length) {
    lines.push(`气质：${persona.temperament.join('、')}`)
  }
  if (persona.likes.length) {
    lines.push(`喜好：${persona.likes.join('、')}`)
  }
  if (persona.drives.length) {
    lines.push(`驱动力：${persona.drives.join('、')}`)
  }
  if (persona.socialStyle.trim()) {
    lines.push(`社交：${persona.socialStyle.trim()}`)
  }
  if (persona.speechStyle.trim()) {
    lines.push(`表达：${persona.speechStyle.trim()}`)
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
    const match = trimmed.match(/^(气质|喜好|驱动力|社交|表达)[：:](.*)$/)
    if (!match) {
      leftover.push(trimmed)
      continue
    }
    const [, key, rawValue] = match
    const value = rawValue.trim()
    if (key === '气质') persona.temperament = parseList(value)
    else if (key === '喜好') persona.likes = parseList(value)
    else if (key === '驱动力') persona.drives = parseList(value)
    else if (key === '社交') persona.socialStyle = value
    else if (key === '表达') persona.speechStyle = value
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
