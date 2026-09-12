export type AgentPanelStage = 'island' | 'overlay' | 'full'

export type AgentPanelPhase = 'settled' | 'opening' | 'closing'

export interface AgentPanelStageState {
  stage: AgentPanelStage
  phase: AgentPanelPhase
}

export type AgentPanelStageAction =
  | { type: 'open'; stage: Exclude<AgentPanelStage, 'island'> }
  | { type: 'close' }
  | { type: 'toggle'; stage: Exclude<AgentPanelStage, 'island'> }
  | { type: 'settle' }

export const INITIAL_AGENT_PANEL_STAGE: AgentPanelStageState = {
  stage: 'island',
  phase: 'settled',
}

/** CSS `--agent-move`. */
export const AGENT_PANEL_ENTER_MS = 480

export const AGENT_PANEL_EXIT_MS = AGENT_PANEL_ENTER_MS

/** Slack if transitionend is dropped. */
export const AGENT_PANEL_SETTLE_SLACK_MS = 140

/** CSS `--agent-stagger-step`. */
export const AGENT_ROW_STAGGER_MS = 72

export const AGENT_ROW_STAGGER_MAX = 8

/** CSS `--agent-row-exit`. */
export const AGENT_ROW_EXIT_MS = 320

export function agentPanelStaggerSteps(count: number): number {
  return Math.min(Math.max(Math.floor(count), 0), AGENT_ROW_STAGGER_MAX + 1)
}

export function agentPanelRowWaveMs(
  count = AGENT_ROW_STAGGER_MAX + 1,
): number {
  return AGENT_ROW_EXIT_MS + AGENT_ROW_STAGGER_MS * agentPanelStaggerSteps(count)
}

export function agentPanelStageReducer(
  state: AgentPanelStageState,
  action: AgentPanelStageAction,
): AgentPanelStageState {
  switch (action.type) {
    case 'open': {
      if (state.stage === action.stage && state.phase !== 'closing') {
        return state.phase === 'settled'
          ? state
          : { stage: action.stage, phase: 'opening' }
      }
      return { stage: action.stage, phase: 'opening' }
    }

    case 'toggle':
      return state.stage === action.stage && state.phase !== 'closing'
        ? agentPanelStageReducer(state, { type: 'close' })
        : agentPanelStageReducer(state, { type: 'open', stage: action.stage })

    case 'close': {
      if (state.stage === 'island') return state
      return { stage: state.stage, phase: 'closing' }
    }

    case 'settle':
      if (state.phase === 'settled') return state
      return state.phase === 'closing'
        ? INITIAL_AGENT_PANEL_STAGE
        : { stage: state.stage, phase: 'settled' }

    default:
      return state
  }
}

export function agentPanelShowsStage(
  state: AgentPanelStageState,
  stage: Exclude<AgentPanelStage, 'island'>,
): boolean {
  return state.stage === stage
}

export function agentPanelIsOpen(state: AgentPanelStageState): boolean {
  return state.stage !== 'island' && state.phase !== 'closing'
}

export function agentPanelSettleTimeoutMs(
  stage: AgentPanelStage = 'overlay',
  count?: number,
): number {
  if (stage !== 'full') return AGENT_PANEL_EXIT_MS + AGENT_PANEL_SETTLE_SLACK_MS
  const steps = agentPanelStaggerSteps(count ?? AGENT_ROW_STAGGER_MAX + 1)
  return (
    AGENT_ROW_EXIT_MS +
    AGENT_ROW_STAGGER_MS * (steps + 1) +
    AGENT_PANEL_SETTLE_SLACK_MS
  )
}
