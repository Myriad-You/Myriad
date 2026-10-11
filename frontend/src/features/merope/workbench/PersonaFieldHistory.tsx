import type { StructuredPersona } from '../../../components/agent/onboarding/onboardingTypes'
import { joinList } from '../../../components/agent/onboarding/onboardingTypes'
import FieldHistory from './FieldHistory'

type Field = keyof StructuredPersona

interface Props {
  /** The persona field, as her persona names it (`summary`, `likes`, …). */
  field: Field
  persona: StructuredPersona
  busy?: boolean
  /** Saves her persona with an old value put back: an ordinary edit. */
  onPersona: (persona: StructuredPersona) => void
  onClose: () => void
}

const LISTS: readonly Field[] = ['temperament', 'likes', 'drives']

function display(field: Field, value: unknown): string {
  if (LISTS.includes(field)) return joinList(value)
  return typeof value === 'string' ? value : ''
}

/** Her persona with `value` back in `field`, when it is a value of that field. */
function restored(
  persona: StructuredPersona,
  field: Field,
  value: unknown,
): StructuredPersona | null {
  if (LISTS.includes(field)) {
    return Array.isArray(value)
      ? {
          ...persona,
          [field]: value.filter(
            (item): item is string => typeof item === 'string',
          ),
        }
      : null
  }
  return typeof value === 'string' ? { ...persona, [field]: value } : null
}

/** What one field of her persona held before, newest first, each can be put back. */
export default function PersonaFieldHistory({
  field,
  persona,
  busy = false,
  onPersona,
  onClose,
}: Props) {
  return (
    <FieldHistory
      fields={[field]}
      busy={busy}
      display={(entry) => display(field, entry.value)}
      canRestore={(entry) => restored(persona, field, entry.value) !== null}
      onRestore={(entry) => {
        const next = restored(persona, field, entry.value)
        if (next) onPersona(next)
      }}
      onClose={onClose}
    />
  )
}
