import type { AgentPendingAction } from './agentAction'
import React, { useEffect, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  agentActionExpired,
  agentActionImpacts,
  agentActionRemainingSeconds,
} from './agentAction'

export interface AgentPanelActionCardProps {
  action: AgentPendingAction
  onDecide: (approved: boolean) => void
}

export const AgentPanelActionCard: React.FC<AgentPanelActionCardProps> = ({
  action,
  onDecide,
}) => {
  const { t, format } = useI18n()
  const [nowMs, setNowMs] = useState(() => Date.now())

  useEffect(() => {
    if (action.expiresAtMs === null) return
    const timer = setInterval(() => setNowMs(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [action.expiresAtMs])

  const remaining = agentActionRemainingSeconds(action, nowMs)
  const expired = agentActionExpired(action, nowMs)
  const showsDetail = action.tier !== 'light'
  const impacts = action.tier === 'explicit' ? agentActionImpacts(action) : []
  const riskTone =
    action.tier === 'explicit'
      ? 'alert'
      : action.tier === 'preview'
        ? 'primary'
        : undefined

  return (
    <div
      className="agent-panel-action"
      data-tier={action.tier}
      data-risk={action.risk}
      role="alertdialog"
      aria-label={action.prompt}
    >
      <div className="agent-panel-action-head">
        <span className="agent-panel-tag" data-tone={riskTone}>
          <span className="agent-panel-tag-text">
            {t.agentPanel.action.risk[action.risk]}
          </span>
        </span>
        {remaining !== null && (
          <span
            className="agent-panel-action-ttl"
            data-expired={expired ? 'true' : 'false'}
          >
            {expired
              ? t.agentPanel.action.expired
              : format(t.agentPanel.action.expiresIn, { seconds: remaining })}
          </span>
        )}
      </div>

      <p className="agent-panel-action-prompt">{action.prompt}</p>

      {showsDetail && action.steps.length > 0 && (
        <div className="agent-panel-action-section">
          <span className="agent-panel-action-label">
            {t.agentPanel.action.steps}
          </span>
          <ul className="agent-panel-action-list">
            {action.steps.map((step) => (
              <li key={step.id}>
                <span className="agent-panel-action-step-name">
                  {step.name}
                </span>
                {step.message ? (
                  <span className="agent-panel-action-step-note">
                    {step.message}
                  </span>
                ) : null}
              </li>
            ))}
          </ul>
        </div>
      )}

      {impacts.length > 0 && (
        <div className="agent-panel-action-section agent-panel-action-impact">
          <span className="agent-panel-action-label">
            {t.agentPanel.action.impact}
          </span>
          <ul className="agent-panel-action-list">
            {impacts.map((impact) => (
              <li key={impact}>{impact}</li>
            ))}
          </ul>
        </div>
      )}

      <div className="agent-panel-action-buttons">
        <button
          type="button"
          className="agent-panel-tag"
          onClick={() => onDecide(false)}
          autoFocus={action.tier !== 'light'}
        >
          <span className="agent-panel-tag-text">
            {t.agentPanel.action.cancel}
          </span>
        </button>
        <button
          type="button"
          className="agent-panel-tag agent-panel-tag-strong"
          data-tone={action.tier === 'explicit' ? 'alert' : 'primary'}
          onClick={() => onDecide(true)}
          disabled={expired}
          autoFocus={action.tier === 'light'}
        >
          <span className="agent-panel-tag-text">
            {t.agentPanel.action.confirm}
          </span>
        </button>
      </div>
    </div>
  )
}
