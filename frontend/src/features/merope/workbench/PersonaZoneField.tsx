import type { AgentPersona } from '../../../services/agent/agentApi'
import { useEffect, useId, useMemo, useState } from 'react'
import { Field, TextInput } from '../../../components/agent/onboarding/ui/Field'
import { SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { emitAppEvent } from '../../../utils/appEvents'
import { userFacingError } from '../../../utils/userFacingError'
import FieldHistory from './FieldHistory'
import { reportMeropeError } from './workbenchShared'

interface Props {
  persona: AgentPersona
  busy?: boolean
  /** Her zone as saved: an IANA name, or empty for the site's own. */
  onSaved: (zone: string) => void
}

/** IANA names this browser knows, for suggestions; the site checks the name. */
function knownZones(): string[] {
  try {
    const intl = Intl as unknown as {
      supportedValuesOf?: (key: string) => string[]
    }
    return intl.supportedValuesOf?.('timeZone') ?? []
  } catch {
    return []
  }
}

/** Where her day runs: which hour it is for her, and when a day turns. */
export default function PersonaZoneField({
  persona,
  busy = false,
  onSaved,
}: Props) {
  const { t } = useI18n()
  const m = t.merope
  const zone = persona.zone ?? ''
  const listId = useId()
  const zones = useMemo(knownZones, [])
  const [draft, setDraft] = useState(zone)
  const [saving, setSaving] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)
  useEffect(() => setDraft(zone), [zone])
  const changed = draft.trim() !== zone.trim()

  /** Saves `next` as her zone; empty is the site's own. */
  const save = (next: string) => {
    setSaving(true)
    agentService
      .putPersona({
        name: persona.name,
        personality: persona.personality ?? '',
        zone: next.trim() || null,
      })
      .then((saved) => {
        onSaved(saved.zone ?? '')
        emitAppEvent('arael-persona-updated')
      })
      .catch((reason: unknown) => {
        reportMeropeError(userFacingError(reason, m.zoneSaveFailed))
      })
      .finally(() => setSaving(false))
  }

  return (
    <>
      <div className="merope-motion-persona__zone">
        <Field label={m.zoneLabel} hint={m.zoneHint}>
          <TextInput
            value={draft}
            list={listId}
            placeholder={m.zoneFollowSite}
            spellCheck={false}
            autoComplete="off"
            disabled={busy || saving}
            onChange={(event) => setDraft(event.target.value)}
          />
          <datalist id={listId}>
            {zones.map((name) => (
              <option key={name} value={name} />
            ))}
          </datalist>
        </Field>
        {changed ? (
          <SettingsButton
            type="button"
            size="sm"
            variant="primary"
            disabled={busy || saving}
            loading={saving}
            onClick={() => save(draft)}
          >
            {m.zoneSave}
          </SettingsButton>
        ) : null}
        <SettingsButton
          type="button"
          size="sm"
          disabled={busy || saving}
          aria-expanded={historyOpen}
          aria-label={`${m.personaHistory} · ${m.zoneLabel}`}
          onClick={() => setHistoryOpen((open) => !open)}
        >
          {m.personaHistory}
        </SettingsButton>
      </div>
      {historyOpen ? (
        <FieldHistory
          fields={['zone']}
          busy={busy || saving}
          // An unset zone is the site's own.
          display={(entry) =>
            typeof entry.value === 'string' && entry.value.trim()
              ? entry.value
              : m.historyZoneSite
          }
          canRestore={(entry) =>
            (entry.value === null || typeof entry.value === 'string') &&
            ((entry.value as string | null) ?? '').trim() !== zone.trim()
          }
          onRestore={(entry) =>
            save(typeof entry.value === 'string' ? entry.value : '')
          }
          onClose={() => setHistoryOpen(false)}
        />
      ) : null}
    </>
  )
}
