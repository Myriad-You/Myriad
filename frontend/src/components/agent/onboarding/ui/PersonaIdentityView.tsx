import type { StructuredPersona } from '../onboardingTypes'
import { LuCheck, LuEdit3, LuX } from '@lib/icons'
import { useState } from 'react'
import { joinList, parseList } from '../onboardingTypes'
import { TextArea } from './Field'
import '../../PersonaOnboarding.css'

type PersonaFieldKey = keyof StructuredPersona

interface Props {
  persona: StructuredPersona
  labels: Record<PersonaFieldKey, string>
  editLabel: string
  cancelLabel: string
  saveLabel: string
  groupLabel: string
  busy?: boolean
  onPersona: (persona: StructuredPersona) => void
}

function displayValue(persona: StructuredPersona, key: PersonaFieldKey): string {
  if (key === 'temperament' || key === 'likes' || key === 'drives') {
    return joinList(persona[key])
  }
  return persona[key]
}

function withPersonaField(
  persona: StructuredPersona,
  key: PersonaFieldKey,
  value: string,
): StructuredPersona {
  if (key === 'temperament' || key === 'likes' || key === 'drives') {
    return { ...persona, [key]: parseList(value) }
  }
  return { ...persona, [key]: value.trim() }
}

export default function PersonaIdentityView({
  persona,
  labels,
  editLabel,
  cancelLabel,
  saveLabel,
  groupLabel,
  busy = false,
  onPersona,
}: Props) {
  const [editingField, setEditingField] = useState<PersonaFieldKey | null>(null)
  const [draft, setDraft] = useState('')
  const rows: Array<{ key: PersonaFieldKey; areaRows: number }> = [
    { key: 'temperament', areaRows: 3 },
    { key: 'likes', areaRows: 3 },
    { key: 'drives', areaRows: 3 },
    { key: 'socialStyle', areaRows: 4 },
    { key: 'speechStyle', areaRows: 4 },
    { key: 'summary', areaRows: 5 },
  ]

  const startEdit = (key: PersonaFieldKey) => {
    setEditingField(key)
    setDraft(displayValue(persona, key))
  }

  const cancelEdit = () => {
    setEditingField(null)
    setDraft('')
  }

  const commitEdit = () => {
    if (!editingField) return
    onPersona(withPersonaField(persona, editingField, draft))
    setEditingField(null)
    setDraft('')
  }

  return (
    <div className="merope-ob-persona-groups">
      <section className="merope-ob-persona-group" aria-label={groupLabel}>
        <dl className="merope-ob-persona-view">
          {rows.map((row) => {
            const isEditing = editingField === row.key
            return (
              <div
                key={row.key}
                className={`merope-ob-persona-view__row${isEditing ? ' is-editing' : ''}`}
              >
                {isEditing ? (
                  <div className="merope-ob-persona-view__editor">
                    <dt>{labels[row.key]}</dt>
                    <TextArea
                      rows={row.areaRows}
                      value={draft}
                      autoFocus
                      onChange={(event) => setDraft(event.target.value)}
                      onKeyDown={(event) => {
                        if (
                          (event.metaKey || event.ctrlKey) &&
                          event.key === 'Enter'
                        ) {
                          event.preventDefault()
                          commitEdit()
                        }
                        if (event.key === 'Escape') {
                          event.preventDefault()
                          cancelEdit()
                        }
                      }}
                    />
                    <div className="merope-ob-persona-view__actions">
                      <button
                        type="button"
                        className="merope-ob-persona-view__action is-cancel"
                        disabled={busy}
                        title={cancelLabel}
                        aria-label={cancelLabel}
                        onClick={cancelEdit}
                      >
                        <LuX aria-hidden />
                      </button>
                      <button
                        type="button"
                        className="merope-ob-persona-view__action is-save"
                        disabled={busy}
                        title={saveLabel}
                        aria-label={saveLabel}
                        onClick={commitEdit}
                      >
                        <LuCheck aria-hidden />
                      </button>
                    </div>
                  </div>
                ) : (
                  <>
                    <div className="merope-ob-persona-view__copy">
                      <dt>{labels[row.key]}</dt>
                      <dd>{displayValue(persona, row.key)}</dd>
                    </div>
                    <button
                      type="button"
                      className="merope-ob-persona-view__edit"
                      disabled={busy}
                      title={editLabel}
                      aria-label={`${editLabel} · ${labels[row.key]}`}
                      onClick={() => startEdit(row.key)}
                    >
                      <LuEdit3 aria-hidden />
                    </button>
                  </>
                )}
              </div>
            )
          })}
        </dl>
      </section>
    </div>
  )
}
