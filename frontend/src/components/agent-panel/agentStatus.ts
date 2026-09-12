import type { ProgressEvent } from '../../services/agent'
import { userFacingError } from '../../utils/userFacingError'

function facingDetail(raw: string | undefined): string | undefined {
  const text = raw?.trim()
  if (!text) return undefined
  return userFacingError(text)
}

export type AgentStatus =
  | 'idle'
  | 'listening'
  | 'thinking'
  | 'working'
  | 'needsInput'
  | 'done'
  | 'error'

export interface AgentStatusState {
  status: AgentStatus
  detail?: string
  /** Reported progress only; never inferred. */
  progress?: number
}

export const IDLE_AGENT_STATUS: AgentStatusState = { status: 'idle' }

export const DONE_LINGER_MS = 2600

export const ERROR_LINGER_MS = 6000

export function agentStatusIsActive(status: AgentStatus): boolean {
  return status !== 'idle'
}

export function agentStatusIsSettled(status: AgentStatus): boolean {
  return status === 'done' || status === 'error'
}

export function reduceAgentStatus(
  state: AgentStatusState,
  event: ProgressEvent,
): AgentStatusState {
  switch (event.type) {
    case 'run_started':
      return { status: 'thinking' }

    case 'planner_decision':
      return { status: 'thinking', detail: state.detail }

    case 'task_created':
      return { status: 'thinking', detail: facingDetail(event.message) }

    case 'step_started':
      return {
        status: 'working',
        detail: facingDetail(event.description),
        progress: state.progress,
      }

    case 'step_retrying':
      return {
        status: 'working',
        detail: facingDetail(event.reason),
        progress: state.progress,
      }

    case 'step_completed':
      return { ...state, status: 'working' }

    case 'progress':
      return { ...state, status: 'working', progress: event.progress }

    case 'thinking_token':
      return { ...state, status: 'thinking', detail: state.detail }

    case 'summary_token':
      return { ...state, status: 'working' }

    case 'waiting_for_input':
      return { status: 'needsInput', detail: facingDetail(event.question) }

    case 'error':
      return { status: 'error', detail: facingDetail(event.message) }

    case 'task_completed':
      return event.success
        ? { status: 'done', progress: 100 }
        : { status: 'error' }

    default:
      return state
  }
}

export function awaitingConfirmation(prompt: string): AgentStatusState {
  return { status: 'needsInput', detail: facingDetail(prompt) }
}

/** Occupy thinking before `run_started` so the mic does not flash back. */
export function beginAgentRun(): AgentStatusState {
  return { status: 'thinking' }
}

/** Listening only when idle or settled; a running task keeps its step. */
export function withListening(
  state: AgentStatusState,
  recording: boolean,
): AgentStatusState {
  if (!recording) return state
  if (state.status === 'idle' || agentStatusIsSettled(state.status)) {
    return { status: 'listening' }
  }
  return state
}

/** Hide thinking/working when this lane is not loading. */
export function agentStatusForLane(
  status: AgentStatus,
  laneLoading: boolean,
): AgentStatus {
  if ((status === 'thinking' || status === 'working') && !laneLoading) {
    return 'idle'
  }
  return status
}
