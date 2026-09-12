import type { ProgressEvent } from '../../services/agent'
import type { AgentPendingAction } from './agentAction'
import type { AgentPanelMode } from './agentPanelMode'
import type { AgentStatusState } from './agentStatus'
import type { AgentUndoOffer } from './agentUndo'
import { useSyncExternalStore } from 'react'
import {
  agentStatusIsSettled,
  awaitingConfirmation,
  beginAgentRun,
  DONE_LINGER_MS,
  ERROR_LINGER_MS,
  IDLE_AGENT_STATUS,
  reduceAgentStatus,
  withListening,
} from './agentStatus'

let streamed: AgentStatusState = IDLE_AGENT_STATUS
let recording = false

let published: AgentStatusState = IDLE_AGENT_STATUS

const IDLE_LANES: Record<AgentPanelMode, boolean> = {
  work: false,
  chat: false,
}

/** Per-lane occupancy; the island is global so stop must read this lane. */
let laneLoading: Record<AgentPanelMode, boolean> = IDLE_LANES

let pendingAction: AgentPendingAction | null = null

let undoOffer: AgentUndoOffer | null = null

const listeners = new Set<() => void>()
let settleTimer: ReturnType<typeof setTimeout> | null = null
let undoTimer: ReturnType<typeof setTimeout> | null = null

function sameState(a: AgentStatusState, b: AgentStatusState): boolean {
  return (
    a.status === b.status && a.detail === b.detail && a.progress === b.progress
  )
}

function notify(): void {
  for (const listener of listeners) listener()
}

function publish(): void {
  const next = withListening(streamed, recording)
  if (sameState(next, published)) return
  published = next
  notify()
}

function clearSettleTimer(): void {
  if (settleTimer === null) return
  clearTimeout(settleTimer)
  settleTimer = null
}

function scheduleSettle(): void {
  clearSettleTimer()
  if (!agentStatusIsSettled(streamed.status)) return
  const linger = streamed.status === 'done' ? DONE_LINGER_MS : ERROR_LINGER_MS
  settleTimer = setTimeout(() => {
    settleTimer = null
    streamed = IDLE_AGENT_STATUS
    publish()
  }, linger)
}

function commit(next: AgentStatusState): void {
  const wasPending = pendingAction !== null
  streamed = next
  if (wasPending && next.status !== 'needsInput') pendingAction = null
  scheduleSettle()
  if (wasPending && pendingAction === null) notify()
  publish()
}

export function pushAgentStatusEvent(event: ProgressEvent): void {
  commit(reduceAgentStatus(streamed, event))
}

export function setAgentStatusRecording(value: boolean): void {
  if (recording === value) return
  recording = value
  publish()
}

export function setAgentStatusAwaitingConfirmation(prompt: string): void {
  commit(awaitingConfirmation(prompt))
}

/** Do not knock working back to thinking. */
export function setAgentStatusThinking(): void {
  if (streamed.status === 'thinking' || streamed.status === 'working') return
  commit(beginAgentRun())
}

export function setAgentPendingAction(action: AgentPendingAction): void {
  pendingAction = action
  commit(awaitingConfirmation(action.prompt))
  notify()
}

export function clearAgentPendingAction(id?: string): void {
  if (pendingAction === null) return
  if (id && pendingAction.id !== id) return
  pendingAction = null
  notify()
}

export function getAgentPendingActionSnapshot(): AgentPendingAction | null {
  return pendingAction
}

export function getServerAgentPendingActionSnapshot(): AgentPendingAction | null {
  return null
}

export function useAgentPendingAction(): AgentPendingAction | null {
  return useSyncExternalStore(
    subscribeAgentStatus,
    getAgentPendingActionSnapshot,
    getServerAgentPendingActionSnapshot,
  )
}

function clearUndoTimer(): void {
  if (undoTimer === null) return
  clearTimeout(undoTimer)
  undoTimer = null
}

export function setAgentUndoOffer(offer: AgentUndoOffer): void {
  clearUndoTimer()
  undoOffer = offer
  const linger = Math.max(0, offer.expiresAtMs - Date.now())
  undoTimer = setTimeout(() => {
    undoTimer = null
    undoOffer = null
    notify()
  }, linger)
  notify()
}

export function clearAgentUndoOffer(id?: string): void {
  if (undoOffer === null) return
  if (id && undoOffer.id !== id) return
  clearUndoTimer()
  undoOffer = null
  notify()
}

export function getAgentUndoOfferSnapshot(): AgentUndoOffer | null {
  return undoOffer
}

export function getServerAgentUndoOfferSnapshot(): AgentUndoOffer | null {
  return null
}

export function useAgentUndoOffer(): AgentUndoOffer | null {
  return useSyncExternalStore(
    subscribeAgentStatus,
    getAgentUndoOfferSnapshot,
    getServerAgentUndoOfferSnapshot,
  )
}

/** Skip linger. */
export function resetAgentStatus(): void {
  clearSettleTimer()
  clearUndoTimer()
  pendingAction = null
  undoOffer = null
  commit(IDLE_AGENT_STATUS)
}

export function subscribeAgentStatus(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function getAgentStatusSnapshot(): AgentStatusState {
  return published
}

export function getServerAgentStatusSnapshot(): AgentStatusState {
  return IDLE_AGENT_STATUS
}

export function useAgentStatus(): AgentStatusState {
  return useSyncExternalStore(
    subscribeAgentStatus,
    getAgentStatusSnapshot,
    getServerAgentStatusSnapshot,
  )
}

export function setAgentLaneLoading(
  mode: AgentPanelMode,
  loading: boolean,
): void {
  if (laneLoading[mode] === loading) return
  laneLoading = { ...laneLoading, [mode]: loading }
  notify()
}

export function getAgentLaneLoading(mode: AgentPanelMode): boolean {
  return laneLoading[mode]
}

export function getAgentLaneLoadingSnapshot(): Record<AgentPanelMode, boolean> {
  return laneLoading
}

export function getServerAgentLaneLoadingSnapshot(): Record<
  AgentPanelMode,
  boolean
> {
  return IDLE_LANES
}

export function useAgentLaneLoading(mode: AgentPanelMode): boolean {
  const lanes = useSyncExternalStore(
    subscribeAgentStatus,
    getAgentLaneLoadingSnapshot,
    getServerAgentLaneLoadingSnapshot,
  )
  return lanes[mode]
}
