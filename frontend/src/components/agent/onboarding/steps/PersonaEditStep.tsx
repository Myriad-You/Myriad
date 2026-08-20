import type { OnboardingHeaderChrome, StructuredPersona } from '../onboardingTypes'
import { LuCheck, LuEdit3, LuX } from '@lib/icons'
import { useCallback, useEffect, useLayoutEffect, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { generationFailureMessage } from '../generationError'
import { joinList, parseList } from '../onboardingTypes'
import { ActionBar, PrimaryButton, StepBody } from '../ui/Chrome'
import { ErrorNote } from '../ui/Feedback'
import { TextArea } from '../ui/Field'

interface Props {
  persona: StructuredPersona
  busy: boolean
  onHeaderChange: (chrome: OnboardingHeaderChrome) => void
  onRegenerate: () => Promise<void>
  onSave: (persona: StructuredPersona) => Promise<void>
}

type PersonaFieldKey =
  | 'temperament'
  | 'likes'
  | 'drives'
  | 'socialStyle'
  | 'speechStyle'
  | 'summary'

function text(value: unknown): string {
  return typeof value === 'string' ? value : ''
}

function keepText(next: unknown, fallback: string): string {
  return text(next).trim() || fallback
}

export default function PersonaEditStep({
  persona,
  busy,
  onHeaderChange,
  onRegenerate,
  onSave,
}: Props) {
  const { t } = useI18n()
  const o = t.life.onboarding
  const [summary, setSummary] = useState(() => persona.summary)
  const [temperament, setTemperament] = useState(() =>
    joinList(persona.temperament),
  )
  const [likes, setLikes] = useState(() => joinList(persona.likes))
  const [drives, setDrives] = useState(() => joinList(persona.drives))
  const [socialStyle, setSocialStyle] = useState(() => persona.socialStyle)
  const [speechStyle, setSpeechStyle] = useState(() => persona.speechStyle)
  const [error, setError] = useState('')
  const [editingField, setEditingField] = useState<PersonaFieldKey | null>(null)
  const [draft, setDraft] = useState('')
  const [regenBusy, setRegenBusy] = useState(false)

  const applyDraft = useCallback((next: StructuredPersona) => {
    setSummary((current) => keepText(next.summary, current))
    setTemperament((current) => joinList(next.temperament) || current)
    setLikes((current) => joinList(next.likes) || current)
    setDrives((current) => joinList(next.drives) || current)
    setSocialStyle((current) => keepText(next.socialStyle, current))
    setSpeechStyle((current) => keepText(next.speechStyle, current))
  }, [])

  useEffect(() => {
    applyDraft(persona)
  }, [applyDraft, persona])

  const generatePersona = useCallback(async () => {
    if (regenBusy) return
    setRegenBusy(true)
    setError('')
    try {
      await onRegenerate()
      setEditingField(null)
      setDraft('')
    } catch (reason) {
      setError(
        generationFailureMessage(
          reason,
          o.regeneratePersonaFailed,
          o.generationTimeout,
        ),
      )
    } finally {
      setRegenBusy(false)
    }
  }, [o.generationTimeout, o.regeneratePersonaFailed, onRegenerate, regenBusy])

  const values: Record<PersonaFieldKey, string> = {
    temperament,
    likes,
    drives,
    socialStyle,
    speechStyle,
    summary,
  }
  const setters: Record<PersonaFieldKey, (next: string) => void> = {
    temperament: setTemperament,
    likes: setLikes,
    drives: setDrives,
    socialStyle: setSocialStyle,
    speechStyle: setSpeechStyle,
    summary: setSummary,
  }
  const optionalFields = new Set<PersonaFieldKey>([
    'likes',
    'drives',
    'socialStyle',
    'speechStyle',
  ])
  const rows: Array<{
    key: PersonaFieldKey
    label: string
    areaRows: number
  }> = [
    { key: 'temperament', label: o.fieldTemperament, areaRows: 3 },
    { key: 'likes', label: o.fieldLikes, areaRows: 3 },
    { key: 'drives', label: o.fieldDrives, areaRows: 3 },
    { key: 'socialStyle', label: o.fieldSocial, areaRows: 4 },
    { key: 'speechStyle', label: o.fieldVoice, areaRows: 4 },
    { key: 'summary', label: o.fieldSummary, areaRows: 5 },
  ]

  const shownValue = (key: PersonaFieldKey) => {
    const raw = values[key].trim()
    if (raw) return raw
    return regenBusy ? o.personaFieldGenerating : ''
  }

  const visibleRows = rows.filter((row) => {
    if (!optionalFields.has(row.key)) return true
    return Boolean(shownValue(row.key))
  })

  const startEdit = (key: PersonaFieldKey) => {
    setEditingField(key)
    setDraft(values[key])
  }
  const cancelEdit = () => {
    setEditingField(null)
    setDraft('')
  }
  const commitEdit = () => {
    if (!editingField) return
    setters[editingField](draft)
    setEditingField(null)
    setDraft('')
  }

  const blocked = busy || regenBusy || editingField !== null
  const headerDescription = regenBusy ? o.step3LeadPending : o.step3Lead

  useLayoutEffect(() => {
    onHeaderChange({
      description: headerDescription,
      action: {
        label: regenBusy ? o.regeneratingPersona : o.regeneratePersona,
        busy: regenBusy,
        disabled: blocked,
        onClick: () => void generatePersona(),
      },
    })
  }, [
    blocked,
    generatePersona,
    headerDescription,
    o.regeneratePersona,
    o.regeneratingPersona,
    onHeaderChange,
    regenBusy,
  ])

  return (
    <section aria-label={o.step3Title}>
      <StepBody>
        <div
          className={`life-ob-persona-groups${regenBusy ? ' is-incomplete' : ''}`}
        >
          <section
            className="life-ob-persona-group"
            aria-label={o.personaGroupCharacter}
          >
            <h2 className="life-ob-persona-group__title">
              {o.personaGroupCharacter}
              {regenBusy ? (
                <span className="life-ob-persona-group__draft">
                  {o.personaDraftLabel}
                </span>
              ) : null}
            </h2>
            <dl className="life-ob-persona-view">
              {visibleRows.map((row) => {
                const isEditing = editingField === row.key
                const display = shownValue(row.key)
                return (
                  <div
                    key={row.key}
                    className={`life-ob-persona-view__row${isEditing ? ' is-editing' : ''}`}
                  >
                    {isEditing ? (
                      <div className="life-ob-persona-view__editor">
                        <dt>{row.label}</dt>
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
                        <div className="life-ob-persona-view__actions">
                          <button
                            type="button"
                            className="life-ob-persona-view__action is-cancel"
                            disabled={busy}
                            title={o.cancelEdit}
                            aria-label={o.cancelEdit}
                            onClick={cancelEdit}
                          >
                            <LuX aria-hidden />
                          </button>
                          <button
                            type="button"
                            className="life-ob-persona-view__action is-save"
                            disabled={busy}
                            title={o.doneEditing}
                            aria-label={o.doneEditing}
                            onClick={commitEdit}
                          >
                            <LuCheck aria-hidden />
                          </button>
                        </div>
                      </div>
                    ) : (
                      <>
                        <div className="life-ob-persona-view__copy">
                          <dt>{row.label}</dt>
                          <dd
                            className={
                              display === o.personaFieldGenerating
                                ? 'is-pending'
                                : undefined
                            }
                          >
                            {display}
                          </dd>
                        </div>
                        <button
                          type="button"
                          className="life-ob-persona-view__edit"
                          disabled={blocked}
                          title={o.editPersona}
                          aria-label={`${o.editPersona} · ${row.label}`}
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
        {error && <ErrorNote>{error}</ErrorNote>}
      </StepBody>
      <ActionBar>
        <PrimaryButton
          label={busy ? o.saving : o.saveAndContinue}
          busy={busy}
          disabled={
            regenBusy ||
            editingField !== null ||
            !summary.trim() ||
            parseList(temperament).length === 0
          }
          onClick={() => {
            setError('')
            void onSave({
              summary: summary.trim(),
              temperament: parseList(temperament),
              likes: parseList(likes),
              drives: parseList(drives),
              socialStyle: socialStyle.trim(),
              speechStyle: speechStyle.trim(),
            }).catch((reason) => {
              setError(
                generationFailureMessage(
                  reason,
                  o.saveFailed,
                  o.generationTimeout,
                ),
              )
            })
          }}
        />
      </ActionBar>
    </section>
  )
}
