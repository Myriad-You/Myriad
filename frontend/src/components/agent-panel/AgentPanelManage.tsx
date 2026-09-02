/**
 * 面板里只剩「跟当前这个人相处」的开关。定时、技能、记忆已经迁到「设置 · AI」。
 */

import React, { useEffect, useState } from 'react'
import { useAuth } from '../../contexts/AuthContext'
import { useI18n } from '../../contexts/I18nContext'
import { agentService } from '../../services/agent'
import { AgentPanelTurnTrace } from './AgentPanelTurnTrace'
import { AgentPresence } from './useAgentPresence'

export function applyAutonomyToggle(
  enabled: boolean,
  service: {
    putAutonomyGrant: (permissions?: string[]) => Promise<unknown>
    revokeAutonomyGrant: () => Promise<unknown>
  },
): Promise<unknown> {
  return enabled
    ? service.putAutonomyGrant([])
    : service.revokeAutonomyGrant()
}

export const AgentPanelManage: React.FC = () => {
  const { t } = useI18n()
  const { isAuthenticated } = useAuth()
  const [note, setNote] = useState<string | null>(null)
  const [autonomy, setAutonomy] = useState<boolean | null>(null)
  /**
   * 勿扰是「跟当前这个人相处」的状态，不是站长的全站设置 —— 所以它在这里，
   * 而不是跟名字性格一起去 /config。
   */
  const [doNotDisturb, setDoNotDisturb] = useState<boolean | null>(null)

  useEffect(() => {
    if (!isAuthenticated) return
    let cancelled = false
    void (async () => {
      try {
        const persona = await agentService.getPersona()
        const grant = await agentService.getAutonomyGrant().catch(() => ({
          grant: null,
        }))
        if (!cancelled) {
          setDoNotDisturb(persona?.doNotDisturb === true)
          setAutonomy(grant.grant != null && grant.grant.revoked !== true)
        }
      } catch {
        // 读不到就不摆这个开关，别给一个点了没反应的东西
      }
    })()
    return () => {
      cancelled = true
    }
  }, [isAuthenticated])

  return (
    <div className="agent-panel-manage">
      <AgentPresence open={!!note} kind="row" from="self">
        <span className="agent-panel-tag" data-block="true" data-tone="alert">
          <span className="agent-panel-tag-text">{note}</span>
        </span>
      </AgentPresence>

      {doNotDisturb !== null && (
        <div
          className="agent-panel-tag agent-panel-manage-row"
          data-block="true"
        >
          <span className="agent-panel-tag-text">
            {t.agentPanel.manage.doNotDisturb}
          </span>
          <button
            type="button"
            className="agent-panel-manage-toggle"
            data-on={doNotDisturb ? 'true' : 'false'}
            aria-pressed={doNotDisturb}
            aria-label={t.agentPanel.manage.doNotDisturb}
            onClick={() => {
              const next = !doNotDisturb
              setDoNotDisturb(next)
              void agentService
                .putAddressee({ doNotDisturb: next })
                .catch(() => {
                  setDoNotDisturb(!next)
                  setNote(t.agentPanel.manage.actionFailed)
                })
            }}
          >
            <span className="agent-panel-manage-knob" />
          </button>
        </div>
      )}

      {autonomy !== null && (
        <div
          className="agent-panel-tag agent-panel-manage-row"
          data-block="true"
        >
          <span className="agent-panel-tag-text">
            {t.agentPanel.manage.autonomyAllow}
          </span>
          <button
            type="button"
            className="agent-panel-manage-toggle"
            data-on={autonomy ? 'true' : 'false'}
            aria-pressed={autonomy}
            aria-label={t.agentPanel.manage.autonomyAllow}
            onClick={() => {
              const next = !autonomy
              setAutonomy(next)
              void applyAutonomyToggle(next, agentService).catch(() => {
                setAutonomy(!next)
                setNote(t.agentPanel.manage.actionFailed)
              })
            }}
          >
            <span className="agent-panel-manage-knob" />
          </button>
        </div>
      )}

      <AgentPanelTurnTrace />

      <span className="agent-panel-tag" data-block="true">
        <span className="agent-panel-tag-text">
          {t.agentPanel.manage.personaElsewhere}
        </span>
      </span>
    </div>
  )
}
