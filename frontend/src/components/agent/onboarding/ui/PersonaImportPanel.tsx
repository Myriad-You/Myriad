import type { StructuredPersona } from '../onboardingTypes'
import { useState } from 'react'
import { useI18n } from '../../../../contexts/I18nContext'
import { agentService } from '../../../../services/agent'
import { InputItem, SettingsButton } from '../../../settings'
import { generationFailureMessage } from '../generationError'
import { personaFromApi } from '../onboardingTypes'
import { ErrorNote } from './Feedback'
import { TextArea } from './Field'

interface Props {
  appearance?: 'onboarding' | 'settings'
  name: string
  gender?: string
  disabled?: boolean
  ready?: boolean
  onImported: (persona: StructuredPersona) => void | Promise<void>
}

export default function PersonaImportPanel({
  appearance = 'onboarding',
  name,
  gender,
  disabled = false,
  ready = false,
  onImported,
}: Props) {
  const { t, locale } = useI18n()
  const o = t.agentPersona.onboarding
  const [source, setSource] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  const importPersona = async () => {
    const text = source.trim()
    if (!text || busy || disabled) return
    setBusy(true)
    setError('')
    try {
      const result = await agentService.importPersona({
        source: text,
        name,
        gender,
        language: locale,
      })
      await onImported(personaFromApi(result.persona))
      setSource('')
    } catch (reason) {
      setError(
        generationFailureMessage(reason, o.importPersonaFailed, o.generationTimeout, {
          pro_unavailable: o.proUnavailable,
          import_source_required: o.importPersonaEmpty,
        }),
      )
    } finally {
      setBusy(false)
    }
  }

  const submitLabel = busy ? o.importPersonaBusy : o.importPersonaSubmit
  const blocked = disabled || busy

  if (appearance === 'settings') {
    return (
      <div className="merope-motion-import">
        <InputItem
          itemKey="persona-import"
          label={o.importPersona}
          description={o.importPersonaHint}
          value={source}
          multiline
          rows={4}
          disabled={blocked}
          placeholder={o.importPersonaPlaceholder}
          error={error || undefined}
          onChange={setSource}
        />
        <SettingsButton
          type="button"
          size="sm"
          disabled={blocked || !source.trim()}
          loading={busy}
          onClick={() => void importPersona()}
        >
          {submitLabel}
        </SettingsButton>
      </div>
    )
  }

  return (
    <div className="merope-ob-import__persona">
      <span className="merope-ob-field__label">{o.importPersona}</span>
      {ready ? (
        <p className="merope-ob-import__ready" aria-live="polite">
          {o.importPersonaReady}
        </p>
      ) : (
        <p className="merope-ob-field__hint">{o.importPersonaHint}</p>
      )}
      <TextArea
        rows={6}
        maxLength={6000}
        value={source}
        disabled={blocked}
        placeholder={o.importPersonaPlaceholder}
        onChange={(event) => setSource(event.target.value)}
      />
      <div className="merope-ob-field__actions">
        <button
          type="button"
          className="merope-ob-import__submit"
          disabled={blocked || !source.trim()}
          onClick={() => void importPersona()}
        >
          {submitLabel}
        </button>
      </div>
      {error ? <ErrorNote>{error}</ErrorNote> : null}
    </div>
  )
}
