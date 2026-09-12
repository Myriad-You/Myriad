import type { AgentPendingAction } from './agentAction'
import React from 'react'
import { AgentPanelActionCard } from './AgentPanelActionCard'

export interface AgentPanelOverlayProps {
  pendingAction: AgentPendingAction | null
  onDecide: (approved: boolean) => void
}

export const AgentPanelOverlay: React.FC<AgentPanelOverlayProps> = ({
  pendingAction,
  onDecide,
}) => {
  if (!pendingAction) return null

  return (
    <div className="agent-panel-overlay glass">
      <AgentPanelActionCard action={pendingAction} onDecide={onDecide} />
    </div>
  )
}
