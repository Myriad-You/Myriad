import type { AgentIntention } from '../../services/agent/agentApi'
import React, { useCallback, useEffect, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'

export interface AgentPanelIntentionProps {
  enabled: boolean
  onAccept: (intentionId: string, input: string) => void
}

/** A review boundary: autonomous observations stop here until the user accepts. */
export const AgentPanelIntention: React.FC<AgentPanelIntentionProps> = ({
  enabled,
  onAccept,
}) => {
  const { isAuthenticated } = useAuth()
  const { t } = useI18n()
  const [intentions, setIntentions] = useState<AgentIntention[]>([])
  const [busyId, setBusyId] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)

  const refresh = useCallback(async () => {
    if (!enabled || !isAuthenticated) {
      setIntentions([])
      return
    }
    try {
      setIntentions(await agentService.listIntentions())
      setFailed(false)
    } catch {
      // This is ambient UI; a failed background refresh must not interrupt chat.
      setFailed(true)
    }
  }, [enabled, isAuthenticated])

  useEffect(() => {
    void refresh()
    if (!enabled || !isAuthenticated) return
    const timer = window.setInterval(() => void refresh(), 20_000)
    const onFocus = () => void refresh()
    window.addEventListener('focus', onFocus)
    return () => {
      window.clearInterval(timer)
      window.removeEventListener('focus', onFocus)
    }
  }, [enabled, isAuthenticated, refresh])

  const intention = intentions[0]
  if (!intention) return null

  const accept = async () => {
    setBusyId(intention.id)
    setFailed(false)
    try {
      const accepted = await agentService.acceptIntention(intention.id)
      setIntentions((current) =>
        current.filter((item) => item.id !== intention.id),
      )
      onAccept(accepted.intention.id, accepted.work.input)
    } catch {
      setFailed(true)
    } finally {
      setBusyId(null)
    }
  }

  const dismiss = async () => {
    setBusyId(intention.id)
    setFailed(false)
    try {
      await agentService.dismissIntention(intention.id)
      setIntentions((current) =>
        current.filter((item) => item.id !== intention.id),
      )
    } catch {
      setFailed(true)
    } finally {
      setBusyId(null)
    }
  }

  const busy = busyId === intention.id
  return (
    <section className="agent-panel-intention glass" aria-live="polite">
      <div className="agent-panel-intention-copy">
        <span>{t.agentPanel.intention.kicker}</span>
        <strong>{intention.proposal.title}</strong>
        <p>{intention.proposal.instruction}</p>
        <small>{intention.proposal.expected_outcome}</small>
        {failed ? (
          <small data-tone="danger">{t.agentPanel.intention.failed}</small>
        ) : null}
      </div>
      <div className="agent-panel-intention-actions">
        <button type="button" disabled={busy} onClick={() => void dismiss()}>
          {t.agentPanel.intention.dismiss}
        </button>
        <button
          type="button"
          className="agent-panel-intention-accept"
          disabled={busy}
          onClick={() => void accept()}
        >
          {intention.status === 'accepted'
            ? t.agentPanel.intention.continue
            : t.agentPanel.intention.accept}
        </button>
      </div>
    </section>
  )
}
