import type { ReactNode } from 'react'
import type { PersonaFieldHistoryEntry } from '../../../services/agent/agentApi'
import { useEffect, useState } from 'react'
import { SettingsButton } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { userFacingError } from '../../../utils/userFacingError'

interface Props {
  /**
   * Persona fields as they cross (`name`, `zone`, `visual.gender`, …).
   * Several are listed together, newest first.
   */
  fields: readonly string[]
  /** What an old value reads as; nothing shows a dash. */
  display: (entry: PersonaFieldHistoryEntry) => ReactNode
  /** Names each entry's field, when several are listed. */
  fieldLabel?: (field: string) => string
  /** Which entries are worth listing; all of them without it. */
  keep?: (entry: PersonaFieldHistoryEntry) => boolean
  /** Whether this old value can be put back now. */
  canRestore: (entry: PersonaFieldHistoryEntry) => boolean
  /** Puts it back: an ordinary edit, which keeps what it replaces too. */
  onRestore: (entry: PersonaFieldHistoryEntry) => void
  busy?: boolean
  onClose: () => void
}

/** What fields of her persona held before, newest first; each can be put back. */
export default function FieldHistory({
  fields,
  display,
  fieldLabel,
  keep,
  canRestore,
  onRestore,
  busy = false,
  onClose,
}: Props) {
  const { t, locale } = useI18n()
  const m = t.merope
  const [entries, setEntries] = useState<PersonaFieldHistoryEntry[] | null>(
    null,
  )
  const [error, setError] = useState('')
  const wanted = fields.join('\n')

  useEffect(() => {
    let live = true
    Promise.all(
      wanted.split('\n').map((field) => agentService.getPersonaHistory(field)),
    )
      .then((loaded) => {
        if (!live) return
        const all = loaded.flat()
        all.sort(
          (a, b) =>
            Date.parse(b.replacedAt) - Date.parse(a.replacedAt) || b.id - a.id,
        )
        setEntries(all)
      })
      .catch((reason: unknown) => {
        if (live) setError(userFacingError(reason, m.personaHistoryFailed))
      })
    return () => {
      live = false
    }
  }, [wanted, m.personaHistoryFailed])

  const shown = keep ? (entries ?? []).filter(keep) : (entries ?? [])

  const when = (at: string) => {
    const date = new Date(at)
    return Number.isNaN(date.getTime())
      ? at
      : date.toLocaleString(locale, { dateStyle: 'medium', timeStyle: 'short' })
  }

  return (
    <div
      className="merope-persona-history"
      role="region"
      aria-label={m.personaHistory}
    >
      <div className="merope-persona-history__head">
        <span>{m.personaHistory}</span>
        <SettingsButton type="button" size="sm" onClick={onClose}>
          {m.personaHistoryClose}
        </SettingsButton>
      </div>
      {error ? (
        <p className="merope-motion-home__help" role="alert">
          {error}
        </p>
      ) : entries === null ? (
        <p className="merope-motion-home__help">{m.personaHistoryLoading}</p>
      ) : shown.length === 0 ? (
        <p className="merope-motion-home__help">{m.personaHistoryEmpty}</p>
      ) : (
        <ol className="merope-persona-history__list">
          {shown.map((entry) => (
            <li key={entry.id} className="merope-persona-history__item">
              <div className="merope-persona-history__copy">
                <small>
                  {fieldLabel ? `${fieldLabel(entry.field)} · ` : ''}
                  {m.personaHistoryReplacedAt.replace(
                    '{time}',
                    when(entry.replacedAt),
                  )}
                  {entry.origin === 'site'
                    ? ''
                    : ` · ${m.personaHistoryFromDevice}`}
                </small>
                <div className="merope-persona-history__value">
                  {display(entry) || '—'}
                </div>
              </div>
              <SettingsButton
                type="button"
                size="sm"
                disabled={busy || !canRestore(entry)}
                onClick={() => {
                  if (canRestore(entry)) onRestore(entry)
                  onClose()
                }}
              >
                {m.personaHistoryRestore}
              </SettingsButton>
            </li>
          ))}
        </ol>
      )}
    </div>
  )
}
