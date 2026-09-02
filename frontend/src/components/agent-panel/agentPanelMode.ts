/**
 * 面板双模式：办事是现在这条 Agent 路径，聊天是对着人设说话。
 *
 * 先只存在界面上。输入行、Tab、形象槽都读这里；真正分路径是下一步。
 * 默认办事 —— 现在的面板行为不能因为多了一档就换掉。
 */

import { useSyncExternalStore } from 'react'
import { isImeComposing } from '../../utils/ime'

export const AGENT_PANEL_MODES = ['work', 'chat'] as const

export type AgentPanelMode = (typeof AGENT_PANEL_MODES)[number]

export const DEFAULT_AGENT_PANEL_MODE: AgentPanelMode = 'work'

let mode: AgentPanelMode = DEFAULT_AGENT_PANEL_MODE

const listeners = new Set<() => void>()

function notify(): void {
  for (const listener of listeners) listener()
}

export function isAgentPanelMode(value: unknown): value is AgentPanelMode {
  return value === 'work' || value === 'chat'
}

export function getAgentPanelMode(): AgentPanelMode {
  return mode
}

export function setAgentPanelMode(next: AgentPanelMode): void {
  if (mode === next) return
  mode = next
  notify()
}

export function cycleAgentPanelMode(step = 1): AgentPanelMode {
  const index = AGENT_PANEL_MODES.indexOf(mode)
  const next =
    AGENT_PANEL_MODES[
      (index + step + AGENT_PANEL_MODES.length) % AGENT_PANEL_MODES.length
    ]
  setAgentPanelMode(next)
  return next
}

export function subscribeAgentPanelMode(onStoreChange: () => void): () => void {
  listeners.add(onStoreChange)
  return () => {
    listeners.delete(onStoreChange)
  }
}

export function useAgentPanelMode(): AgentPanelMode {
  return useSyncExternalStore(subscribeAgentPanelMode, getAgentPanelMode)
}

/**
 * 面板开着、焦点在外壳里时，Tab 用来换模式，不再走浏览器的焦点环。
 * 组字途中、已经有人 preventDefault、或焦点在面板外，都不管。
 */
export function shouldCaptureModeTab(
  event: KeyboardEvent,
  root: { contains: (node: Node) => boolean } | null,
): boolean {
  if (event.key !== 'Tab') return false
  if (event.altKey || event.metaKey || event.ctrlKey) return false
  if (event.defaultPrevented) return false
  if (isImeComposing(event)) return false
  if (!root) return false
  const target = event.target
  if (target == null) return false
  return root.contains(target as Node)
}
