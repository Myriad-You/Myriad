/**
 * Agent 状态语言 —— 后端事件到状态符号的唯一翻译。
 *
 * 新 UI 的三档（岛 / Quick Overlay / Full）都从这里取状态，谁也不自己解读 SSE：
 * 状态只有一份，脸在就看脸、脸不在就看岛，两边永远说同一件事。
 *
 * 纯数据 + 纯函数：不碰 DOM、不起定时器、不认识组件。停留时长这类需要计时的事
 * 只在这里定长度（`DONE_LINGER_MS` / `ERROR_LINGER_MS`），定时器由 UI 自己起。
 */

import type { ProgressEvent } from '../../services/agent'

/** 助手此刻处于哪一档。除 `idle` 外都算「在场」，见 `agentStatusIsActive`。 */
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
  /** 岛上那一行字：当前步骤、待答问题、错误原因。没有就不显示。 */
  detail?: string
  /** 0–100。只有任务真的报过进度才有值 —— 不猜。 */
  progress?: number
}

export const IDLE_AGENT_STATUS: AgentStatusState = { status: 'idle' }

/** `✓` 停留多久后自己退回 `◇`。 */
export const DONE_LINGER_MS = 2600

/** `✗` 停留更久：失败比成功更需要被看见一眼。 */
export const ERROR_LINGER_MS = 6000

/** 需要岛留在场上的状态。移动端靠它决定要不要顶替导航岛。 */
export function agentStatusIsActive(status: AgentStatus): boolean {
  return status !== 'idle'
}

/** 一轮跑完了 —— `done` 与 `error` 都是终态，等停留时间到就回 `◇`。 */
export function agentStatusIsSettled(status: AgentStatus): boolean {
  return status === 'done' || status === 'error'
}

/**
 * 推进一个 SSE 事件。
 *
 * 只认那些真正改变「它在干什么」的事件。会话 id、标题、多 Agent 分配、调试轨迹
 * 都不改状态；`merope_state_changed` 与 `performance_plan` 更不改 —— 那是心情和
 * 表演，属于形象通道，混进来岛就会开始演戏。
 */
export function reduceAgentStatus(
  state: AgentStatusState,
  event: ProgressEvent,
): AgentStatusState {
  switch (event.type) {
    // 后端接管了，但还没排出步骤
    case 'run_started':
      return { status: 'thinking' }

    case 'planner_decision':
      return { status: 'thinking', detail: state.detail }

    case 'task_created':
      return { status: 'thinking', detail: event.message || undefined }

    case 'step_started':
      return {
        status: 'working',
        detail: event.description || undefined,
        progress: state.progress,
      }

    // 重试仍在同一步里，把原因顶上来让人知道为什么慢
    case 'step_retrying':
      return {
        status: 'working',
        detail: event.reason || undefined,
        progress: state.progress,
      }

    // 步骤结果是给 Full 层看的，岛上留着当前那行字不动
    case 'step_completed':
      return { ...state, status: 'working' }

    // 进度是 progress 事件的独家职责，别处不写，省得两个来源打架
    case 'progress':
      return { ...state, status: 'working', progress: event.progress }

    // 模型还在写思考链：岛上保持思考，不要提前跳到「在做事」
    case 'thinking_token':
      return { ...state, status: 'thinking', detail: state.detail }

    // 流式回复也算在做事，直到 task_completed 才收
    case 'summary_token':
      return { ...state, status: 'working' }

    case 'waiting_for_input':
      return { status: 'needsInput', detail: event.question || undefined }

    case 'error':
      return { status: 'error', detail: event.message || undefined }

    case 'task_completed':
      return event.success
        ? { status: 'done', progress: 100 }
        : { status: 'error' }

    default:
      return state
  }
}

/**
 * 敏感操作确认不走 SSE —— 它从响应体里来（`responseType === 'confirmation_required'`）。
 * 走这里而不是让调用方自己拼，是为了保住「状态只有一份」。
 */
export function awaitingConfirmation(prompt: string): AgentStatusState {
  return { status: 'needsInput', detail: prompt || undefined }
}

/**
 * 本地已经把话发出去了，SSE 的 `run_started` 还没到。
 * 先占住思考，输入行才不会在「已经在想」的时候闪回语音按钮。
 */
export function beginAgentRun(): AgentStatusState {
  return { status: 'thinking' }
}

/**
 * 麦克风是本地状态，不在 SSE 里。
 *
 * 录音只在空闲或终态时顶到最前：任务正跑着的时候，用户更需要看到它走到哪一步，
 * 而不是一个「我在听」。
 */
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

/**
 * 岛是全站一份；当前档没在跑时，思考/执行不该画在这一档上。
 * 等回话、完成、出错仍跟岛走 —— 那些不是「这一档还在跑」。
 */
export function agentStatusForLane(
  status: AgentStatus,
  laneLoading: boolean,
): AgentStatus {
  if ((status === 'thinking' || status === 'working') && !laneLoading) {
    return 'idle'
  }
  return status
}
