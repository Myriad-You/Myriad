import { isImeComposing } from '../../utils/ime'
import { createStore, useStore } from '../../utils/store'

export const AGENT_PANEL_MODES = ['work', 'chat'] as const

export type AgentPanelMode = (typeof AGENT_PANEL_MODES)[number]

export const DEFAULT_AGENT_PANEL_MODE: AgentPanelMode = 'work'

const mode = createStore<AgentPanelMode>(DEFAULT_AGENT_PANEL_MODE)

export function isAgentPanelMode(value: unknown): value is AgentPanelMode {
  return value === 'work' || value === 'chat'
}

export const getAgentPanelMode = mode.get
export const subscribeAgentPanelMode = mode.subscribe

export function setAgentPanelMode(next: AgentPanelMode): void {
  mode.set(next)
}

export function cycleAgentPanelMode(step = 1): AgentPanelMode {
  const index = AGENT_PANEL_MODES.indexOf(mode.get())
  const next =
    AGENT_PANEL_MODES[
      (index + step + AGENT_PANEL_MODES.length) % AGENT_PANEL_MODES.length
    ]
  setAgentPanelMode(next)
  return next
}

export function useAgentPanelMode(): AgentPanelMode {
  return useStore(mode)
}

/** Tab cycles modes inside the shell; skip IME, defaultPrevented, and outside focus. */
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
