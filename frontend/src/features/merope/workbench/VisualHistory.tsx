import type {
  UpperBodyVisualIdentity,
  UpperBodyVisualIdentityKey,
} from '../../../components/agent/onboarding/onboardingTypes'
import type { PersonaFieldHistoryEntry } from '../../../services/agent/agentApi'
import { Fragment } from 'react'
import {
  CHARACTER_VISUAL_KEYS,
  joinList,
  parseUpperBodyVisualIdentity,
  visualField,
} from '../../../components/agent/onboarding/onboardingTypes'
import { useI18n } from '../../../contexts/I18nContext'
import FieldHistory from './FieldHistory'

/** Her visual profile's keys cross as persona fields under this prefix. */
const PREFIX = 'visual.'
/** Keys that are not fields: the wardrobe crosses as outfits, and what she wears is each side's own. */
const NOT_FIELDS = ['wardrobe', 'activeOutfitId', 'activeFullBodyOutfitId']
/** Keys the site writes, listed even when her profile has none of them now. */
const KNOWN = [
  'visualIdentity',
  'gender',
  'clothingStyle',
  'language',
  'sourceTags',
  'extraRequirements',
  'personaExtraRequirements',
]

interface Props {
  /** Her visual profile as stored. */
  profile: Record<string, unknown> | null | undefined
  /** Her design as it is now. */
  identity: UpperBodyVisualIdentity | null
  labels: Record<UpperBodyVisualIdentityKey, string>
  busy?: boolean
  /** Puts back an old face and hair; the outfit stays the worn one's. */
  onIdentity: (identity: UpperBodyVisualIdentity) => void
  /** Puts back what another key of her visual profile held. */
  onKey: (key: string, value: unknown) => void
  onClose: () => void
}

function same(a: unknown, b: unknown): boolean {
  return JSON.stringify(a ?? null) === JSON.stringify(b ?? null)
}

/**
 * What her visual settings held before, every `visual.*` field together,
 * newest first. Her design keeps the worn outfit inside it, so an old one
 * is shown and put back by its face and hair only.
 */
export default function VisualHistory({
  profile,
  identity,
  labels,
  busy = false,
  onIdentity,
  onKey,
  onClose,
}: Props) {
  const { t } = useI18n()
  const o = t.agentPersona.onboarding
  const m = t.merope
  const keys = [
    ...new Set([
      ...KNOWN,
      ...Object.keys(profile ?? {}).filter((key) => !NOT_FIELDS.includes(key)),
    ]),
  ]
  const keyOf = (field: string) => field.slice(PREFIX.length)

  const fieldLabel = (field: string) => {
    const key = keyOf(field)
    const named: Record<string, string> = {
      visualIdentity: m.visualFixedTitle,
      gender: o.genderLabel,
      clothingStyle: o.clothingStyleLabel,
      language: m.historyVisualLanguage,
      sourceTags: m.historyVisualSourceTags,
      extraRequirements: o.visualRequirements,
      personaExtraRequirements: o.extraLabel,
    }
    return named[key] ?? key
  }

  /** An old design whose face and hair differ from hers now. */
  const oldCharacter = (entry: PersonaFieldHistoryEntry) => {
    const old = parseUpperBodyVisualIdentity(entry.value)
    if (!old) return null
    return identity &&
      CHARACTER_VISUAL_KEYS.every(
        (key) => visualField(old, key) === visualField(identity, key),
      )
      ? null
      : old
  }

  const display = (entry: PersonaFieldHistoryEntry) => {
    const key = keyOf(entry.field)
    const value = entry.value
    if (key === 'visualIdentity') {
      const old = parseUpperBodyVisualIdentity(value)
      return old ? (
        <dl>
          {CHARACTER_VISUAL_KEYS.map((field) => (
            <Fragment key={field}>
              <dt>{labels[field]}</dt>
              <dd>{visualField(old, field)}</dd>
            </Fragment>
          ))}
        </dl>
      ) : null
    }
    if (key === 'gender' && typeof value === 'string') {
      return o.gender[value as keyof typeof o.gender] ?? value
    }
    if (key === 'clothingStyle' && typeof value === 'string') {
      return o.clothingStyle[value as keyof typeof o.clothingStyle] ?? value
    }
    if (Array.isArray(value)) return joinList(value)
    if (typeof value === 'string') return value
    if (typeof value === 'number' || typeof value === 'boolean') {
      return String(value)
    }
    return value === null ? null : JSON.stringify(value)
  }

  const canRestore = (entry: PersonaFieldHistoryEntry) => {
    const key = keyOf(entry.field)
    if (key === 'visualIdentity') {
      return Boolean(identity) && oldCharacter(entry) !== null
    }
    return !same(entry.value, profile?.[key])
  }

  return (
    <FieldHistory
      fields={keys.map((key) => `${PREFIX}${key}`)}
      fieldLabel={fieldLabel}
      // A design changes with every outfit worn; only other faces and hair
      // are history here.
      keep={(entry) =>
        keyOf(entry.field) !== 'visualIdentity' || oldCharacter(entry) !== null
      }
      display={display}
      canRestore={canRestore}
      onRestore={(entry) => {
        const key = keyOf(entry.field)
        if (key === 'visualIdentity') {
          const old = oldCharacter(entry)
          if (old) onIdentity(old)
        } else {
          onKey(key, entry.value)
        }
      }}
      busy={busy}
      onClose={onClose}
    />
  )
}
