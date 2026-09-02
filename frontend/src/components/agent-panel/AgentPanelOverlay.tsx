/**
 * Quick Overlay —— 两档里较小的那一档。
 *
 * 关联动作跟在输入框上面的上下文贴后面。等人拍板时整块换成操作卡片。
 *
 * 它不执行任何东西 —— 话递给 `dispatchAgentPanelSubmit`，谁在执行谁接住。
 */

import type { AgentPendingAction } from './agentAction'
import React from 'react'
import { AgentPanelActionCard } from './AgentPanelActionCard'

export interface AgentPanelOverlayProps {
  /** 有待确认的操作时，这一档整块让给操作卡片 */
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
