/**
 * Agent 状态的全站唯一副本。
 *
 * 岛、Quick Overlay、以后的 Full 层都从这里读；谁在跑 SSE 谁往这里写。用模块级
 * store 而不是 Context，是因为写入方（面板）和读取方（岛）在组件树上没有关系，
 * 而且面板迟早要被换掉 —— 中间那段时间新旧两边喂同一个 store 就行。
 *
 * 状态怎么算在 `agentStatus.ts`，那是纯函数。这里只管「存一份、通知订阅者、
 * 终态自己退回空闲」这三件事。
 */

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

/** 事件推出来的状态，不含麦克风。 */
let streamed: AgentStatusState = IDLE_AGENT_STATUS
let recording = false

/** 对外那一份，`withListening` 之后的结果。引用稳定，供 useSyncExternalStore 用。 */
let published: AgentStatusState = IDLE_AGENT_STATUS

const IDLE_LANES: Record<AgentPanelMode, boolean> = {
  work: false,
  chat: false,
}

/**
 * 当前档有没有占用输入行。岛状态是全站一份；终止按钮必须看「我正看着的那一档」
 * 还在不在跑，不然办事没停完时聊天档的终止会按了没反应。
 */
let laneLoading: Record<AgentPanelMode, boolean> = IDLE_LANES

/**
 * 正在等确认的那个操作。它是状态的一部分，不是另一份状态 —— 只在助手停下来
 * 等人回话时存在，状态一走它就得没，否则会留在界面上问一件已经过去的事。
 */
let pendingAction: AgentPendingAction | null = null

/**
 * 最近一次能退回去的操作。只留一条 —— 助手连着做了几件事之后还能一层层往回退，
 * 那是编辑器的语义，不是助手的；这里只保证「刚才那一下」能反悔。
 */
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
  // 引用不变，订阅者就不会因为一次无意义的事件重渲染
  if (sameState(next, published)) return
  published = next
  notify()
}

function clearSettleTimer(): void {
  if (settleTimer === null) return
  clearTimeout(settleTimer)
  settleTimer = null
}

/** 终态停留一会儿再退回 ◇；失败停得久一点，比成功更需要被看见一眼。 */
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
  // 不再等回话了，那个操作卡片就该收走
  if (wasPending && next.status !== 'needsInput') pendingAction = null
  scheduleSettle()
  if (wasPending && pendingAction === null) notify()
  publish()
}

/** 喂一个 SSE 事件。不认识的事件不会改变任何东西。 */
export function pushAgentStatusEvent(event: ProgressEvent): void {
  commit(reduceAgentStatus(streamed, event))
}

/** 麦克风开关。录音只在闲着的时候顶到最前，规则在 `withListening` 里。 */
export function setAgentStatusRecording(value: boolean): void {
  if (recording === value) return
  recording = value
  publish()
}

/** 敏感操作确认 —— 它从响应体来，不在 SSE 里。 */
export function setAgentStatusAwaitingConfirmation(prompt: string): void {
  commit(awaitingConfirmation(prompt))
}

/** 话已经出口，SSE 还没接管。已经在跑就不要把执行打回思考。 */
export function setAgentStatusThinking(): void {
  if (streamed.status === 'thinking' || streamed.status === 'working') return
  commit(beginAgentRun())
}

/**
 * 摆出一个待确认的操作。同时把状态推到「等你回话」—— 两者本来就是一件事，
 * 分开设置迟早会对不上。
 */
export function setAgentPendingAction(action: AgentPendingAction): void {
  pendingAction = action
  commit(awaitingConfirmation(action.prompt))
  notify()
}

/** 用户回过话了、或者过期了。状态交给随后的事件去改，这里只收卡片。 */
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

/**
 * 记下一次可撤销的操作。新的顶掉旧的 —— 只留「刚才那一下」。
 */
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

/** 撤销按过了、或者过期了。 */
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

/** 用户中断、开新会话：立刻回到 ◇，不走停留。 */
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

/** SSR 永远是空闲 —— 服务端没有正在跑的任务。 */
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

/** 这一档开始或结束占用输入行。岛状态没变也要叫醒订阅者。 */
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
