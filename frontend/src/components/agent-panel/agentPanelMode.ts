import { createStore, useStore } from '../../utils/store'

export const AGENT_PANEL_MODES = ['work', 'chat'] as const

export type AgentPanelMode = (typeof AGENT_PANEL_MODES)[number]

/**
 * The panel is one conversation with her; what she hands off shows as pieces
 * in it. Work stays reachable only where something enters it on its own (an
 * accepted proposal, her being away, a tool's own resident Agent).
 */
export const DEFAULT_AGENT_PANEL_MODE: AgentPanelMode = 'chat'

const mode = createStore<AgentPanelMode>(DEFAULT_AGENT_PANEL_MODE)

export function isAgentPanelMode(value: unknown): value is AgentPanelMode {
  return value === 'work' || value === 'chat'
}

export const getAgentPanelMode = mode.get
export const subscribeAgentPanelMode = mode.subscribe

export function setAgentPanelMode(next: AgentPanelMode): void {
  mode.set(next)
}

export function useAgentPanelMode(): AgentPanelMode {
  return useStore(mode)
}
