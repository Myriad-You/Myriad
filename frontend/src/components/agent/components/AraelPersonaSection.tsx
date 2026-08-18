import React, { useCallback, useEffect, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { invalidatePublicConfigCache } from '../../../utils/requestDedup'

export const AraelPersonaSection: React.FC<{
  isOwner: boolean
  onPersonaSaved?: (name: string) => void
}> = ({ isOwner, onPersonaSaved }) => {
  const { t } = useI18n()
  const a = t.arael
  const [name, setName] = useState('')
  const [personality, setPersonality] = useState('')
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [disabled, setDisabled] = useState(false)
  const [canEdit, setCanEdit] = useState(false)
  const [doNotDisturb, setDoNotDisturb] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const persona = await agentService.getPersona()
      if (!persona) {
        setDisabled(true)
        setLoading(false)
        return
      }
      setDisabled(false)
      setName(persona.name ?? '')
      setDoNotDisturb(persona.doNotDisturb === true)
      setCanEdit(isOwner)
      setPersonality(
        typeof persona.personality === 'string' ? persona.personality : '',
      )
    } catch (e) {
      setDisabled(false)
      setError(e instanceof Error ? e.message : a.manageLoadError)
    } finally {
      setLoading(false)
    }
  }, [a.manageLoadError, isOwner])

  useEffect(() => {
    void load()
  }, [load])

  const save = useCallback(async () => {
    setSaving(true)
    setError(null)
    try {
      const saved = await agentService.putPersona({
        name: name.trim(),
        personality,
      })
      setName(saved.name)
      setPersonality(saved.personality ?? '')
      onPersonaSaved?.(saved.name || 'Arael')
      invalidatePublicConfigCache()
      window.dispatchEvent(new CustomEvent('arael-persona-updated'))
    } catch (e) {
      setError(e instanceof Error ? e.message : a.manageActionError)
    } finally {
      setSaving(false)
    }
  }, [name, personality, onPersonaSaved, a.manageActionError])

  const reset = useCallback(async () => {
    setSaving(true)
    setError(null)
    try {
      await agentService.deletePersona()
      setName('Arael')
      setPersonality('')
      onPersonaSaved?.('Arael')
      invalidatePublicConfigCache()
      window.dispatchEvent(new CustomEvent('arael-persona-updated'))
    } catch (e) {
      setError(e instanceof Error ? e.message : a.manageActionError)
    } finally {
      setSaving(false)
    }
  }, [onPersonaSaved, a.manageActionError])

  const toggleDnd = useCallback(async () => {
    const next = !doNotDisturb
    setDoNotDisturb(next)
    setError(null)
    try {
      const saved = await agentService.putAddressee({ doNotDisturb: next })
      setDoNotDisturb(saved.doNotDisturb)
    } catch (e) {
      setDoNotDisturb(!next)
      setError(e instanceof Error ? e.message : a.manageActionError)
    }
  }, [doNotDisturb, a.manageActionError])

  if (loading) return null
  if (disabled) {
    return <div className="arael-manage-empty">{a.personaLifeOff}</div>
  }
  if (error && !name && !canEdit) {
    return (
      <div className="arael-manage-error" role="alert">
        <span>{error}</span>
        <button
          type="button"
          className="arael-manage-error-retry"
          onClick={() => void load()}
        >
          {t.common.retry}
        </button>
      </div>
    )
  }

  const owner = isOwner && canEdit

  return (
    <div className="arael-persona-form">
      <label className="arael-persona-label">
        {a.personaName}
        <input
          className="arael-hb-input"
          value={name}
          disabled={!owner || saving}
          onChange={(e) => setName(e.target.value)}
          placeholder="Arael"
        />
      </label>
      {owner ? (
        <label className="arael-persona-label">
          {a.personaPersonality}
          <textarea
            className="arael-hb-input arael-persona-textarea"
            value={personality}
            disabled={saving}
            onChange={(e) => setPersonality(e.target.value)}
            rows={5}
            placeholder={a.personaPersonalityHint}
          />
        </label>
      ) : null}
      <label className="arael-persona-dnd">
        <input
          type="checkbox"
          checked={doNotDisturb}
          disabled={saving}
          onChange={() => void toggleDnd()}
        />
        <span>{a.personaDoNotDisturb}</span>
      </label>
      {error ? (
        <div className="arael-manage-error" role="alert">
          {error}
        </div>
      ) : null}
      {owner ? (
        <div className="arael-persona-actions">
          <button
            type="button"
            className="arael-persona-save"
            disabled={saving}
            onClick={() => void save()}
          >
            {a.personaSave}
          </button>
          <button
            type="button"
            className="arael-persona-reset"
            disabled={saving}
            onClick={() => void reset()}
          >
            {a.personaReset}
          </button>
        </div>
      ) : (
        <p className="arael-persona-readonly">{a.personaOwnerOnly}</p>
      )}
    </div>
  )
}
