import type { OnboardingHeaderChrome, StructuredPersona } from '../onboardingTypes'
import { LuCheck, LuEdit3, LuX } from '@lib/icons'
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { generationFailureMessage } from '../generationError'
import {
  incompletePersonaFields,
  joinList,
  parseList,
  structuredPersonaIsComplete,
} from '../onboardingTypes'
import { ActionBar, PrimaryButton, StepBody } from '../ui/Chrome'
import { ErrorNote } from '../ui/Feedback'
import { TextArea } from '../ui/Field'
import PersonaImportPanel from '../ui/PersonaImportPanel'

interface Props {
  persona: StructuredPersona
  name: string
  gender?: string
  busy: boolean
  claimAutoGenerate: () => boolean
  onHeaderChange: (chrome: OnboardingHeaderChrome) => void
  onRegenerate: () => Promise<void>
  onImported: (persona: StructuredPersona) => void
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
  name,
  gender,
  busy,
  claimAutoGenerate,
  onHeaderChange,
  onRegenerate,
  onImported,
  onSave,
}: Props) {
  const { t, locale } = useI18n()
  const o = t.agentPersona.onboarding
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
  const regenBusyRef = useRef(false)

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
    if (regenBusyRef.current) return
    regenBusyRef.current = true
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
          {
            pro_unavailable: o.proUnavailable,
            persona_draft_failed: o.regeneratePersonaFailed,
            persona_unusable: o.personaUnusable,
          },
        ),
      )
    } finally {
      regenBusyRef.current = false
      setRegenBusy(false)
    }
  }, [
    o.generationTimeout,
    o.personaUnusable,
    o.proUnavailable,
    o.regeneratePersonaFailed,
    onRegenerate,
  ])

  useEffect(() => {
    if (structuredPersonaIsComplete(persona)) return
    if (!claimAutoGenerate()) return
    void generatePersona()
  }, [])

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

  const draftPersona = {
    summary: summary.trim(),
    temperament: parseList(temperament),
    likes: parseList(likes),
    drives: parseList(drives),
    socialStyle: socialStyle.trim(),
    speechStyle: speechStyle.trim(),
  }
  const missingFields = incompletePersonaFields(draftPersona)
  const fieldLabels: Record<(typeof missingFields)[number], string> = {
    temperament: o.fieldTemperament,
    likes: o.fieldLikes,
    drives: o.fieldDrives,
    socialStyle: o.fieldSocial,
    speechStyle: o.fieldVoice,
    summary: o.fieldSummary,
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
        <PersonaImportPanel
          name={name}
          gender={gender}
          disabled={busy || editingField !== null}
          onImported={(next) => {
            applyDraft(next)
            onImported(next)
          }}
        />
        <div
          className={`merope-ob-persona-groups${regenBusy ? ' is-incomplete' : ''}`}
        >
          <section className="merope-ob-persona-group" aria-label={o.step3Title}>
            <dl className="merope-ob-persona-view">
              {rows.map((row) => {
                const isEditing = editingField === row.key
                const display = shownValue(row.key)
                return (
                  <div
                    key={row.key}
                    className={`merope-ob-persona-view__row${isEditing ? ' is-editing' : ''}`}
                  >
                    {isEditing ? (
                      <div className="merope-ob-persona-view__editor">
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
                        <div className="merope-ob-persona-view__actions">
                          <button
                            type="button"
                            className="merope-ob-persona-view__action is-cancel"
                            disabled={busy}
                            title={o.cancelEdit}
                            aria-label={o.cancelEdit}
                            onClick={cancelEdit}
                          >
                            <LuX aria-hidden />
                          </button>
                          <button
                            type="button"
                            className="merope-ob-persona-view__action is-save"
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
                        <div className="merope-ob-persona-view__copy">
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
                          className="merope-ob-persona-view__edit"
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
        {!error && missingFields.length > 0 && !regenBusy ? (
          <ErrorNote>
            {o.personaIncompleteHint.replace(
              '{fields}',
              missingFields
                .map((key) => fieldLabels[key])
                .join(locale.startsWith('en') ? ', ' : '、'),
            )}
          </ErrorNote>
        ) : null}
      </StepBody>
      <ActionBar>
        <PrimaryButton
          label={
            regenBusy ? o.regeneratingPersona : busy ? o.saving : o.next
          }
          busy={busy || regenBusy}
          disabled={
            editingField !== null ||
            !structuredPersonaIsComplete(draftPersona)
          }
          onClick={() => {
            setError('')
            void onSave(draftPersona).catch((reason) => {
              setError(
                generationFailureMessage(
                  reason,
                  o.saveFailed,
                  o.generationTimeout,
                  {
                    pro_unavailable: o.proUnavailable,
                    persona_contract_invalid: o.saveFailed,
                    persona_draft_failed: o.regeneratePersonaFailed,
                    persona_unusable: o.personaUnusable,
                  },
                ),
              )
            })
          }}
        />
      </ActionBar>
    </section>
  )
}
