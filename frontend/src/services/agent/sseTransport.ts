/**
 * Agent SSE 订阅传输层。
 *
 * 只负责读取/重连后端 run 事件；它不会创建、取消或拥有任务生命周期。
 * 用户主动中断与网络断线分开处理：前者不自动 re-subscribe，后者会。
 */
import type {
  AgentResponse,
  ErrorEvent,
  ProgressCallback,
  ProgressEvent,
  TaskCompletedEvent,
  TaskDetail,
  TaskInfo,
} from './types'

import { currentCopy } from '../../i18n/localeCopy'
import { clearCSRFToken, getCSRFToken } from '../../utils/csrf'
import { isUselessErrorText } from '../../utils/userFacingError'
import { ApiError, parseApiErrorBody } from '../api'
import { messageFromStepOutput } from './taskEnvelope'
import {
  acceptRunSequence,
  STREAM_SUPERSEDED_MESSAGE,
} from './turnIdentity'

export { messageFromStepOutput }

export function agentHttpFailure(status: number, text: string): ApiError {
  let parsed: unknown = text
  if (text.trim()) {
    try {
      parsed = JSON.parse(text)
    } catch {
      parsed = text
    }
  }
  const body = parseApiErrorBody(
    typeof parsed === 'string' ? undefined : parsed,
    status,
  )
  const rawText = text.trim()
  if (typeof parsed === 'string' && rawText && !isUselessErrorText(rawText)) {
    return new ApiError(rawText, status, body.code, body.details, body.hint)
  }
  const message = !isUselessErrorText(body.message)
    ? body.message
    : rawText && !isUselessErrorText(rawText)
      ? rawText
      : body.message
  return new ApiError(message, status, body.code, body.details, body.hint)
}

/**
 * A backend `error` event, keeping its `code`.
 *
 * The stream is already HTTP 200 by the time anything can fail, so the code is
 * the only way a caller can tell an AI budget rejection (cooldown, daily call
 * or token limit) from a processing failure. Rejecting with a bare `Error`
 * dropped it and left the UI string-matching the message.
 */
export class AgentStreamError extends Error {
  readonly code: string

  constructor(message: string, code: string) {
    super(message)
    this.name = 'AgentStreamError'
    this.code = code
  }

  /** Whether this is an AI quota/cooldown rejection rather than a fault. */
  get isQuotaRejection(): boolean {
    return QUOTA_CODES.has(this.code)
  }
}

const QUOTA_CODES = new Set([
  'AI_COOLDOWN_ACTIVE',
  'AI_DAILY_CALL_LIMIT',
  'AI_ANONYMOUS_DAILY_CALL_LIMIT',
  'AI_DAILY_TOKEN_LIMIT',
  'AI_ANONYMOUS_DAILY_TOKEN_LIMIT',
  'AI_QUOTA_EXCEEDED',
])

/** Why a stream AbortController was aborted. */
export type StreamAbortIntent = 'user' | 'replace' | 'timeout'

const controllerIntents = new WeakMap<AbortController, StreamAbortIntent>()

/** Token events must paint between reads; React 18 batches a sync for-loop. */
export function shouldYieldSsePaint(type: string): boolean {
  return type === 'thinking_token' || type === 'summary_token'
}

export type StreamDropAction =
  | 'use_final'
  | 'resume_run'
  | 'poll_task'
  | 'reject_user_abort'
  | 'reject_replace'
  | 'reject_error'
  | 'reject_empty'

/**
 * Pure decision for what to do when an SSE body ends without a final response.
 * Unit-tested; called by the real `executeSSERequest` path.
 */
export function decideStreamDropAction(input: {
  hasFinalResponse: boolean
  capturedRunId: string | null
  capturedTaskId: string | null
  abortIntent: StreamAbortIntent | null | undefined
  hasStreamError: boolean
}): StreamDropAction {
  if (input.hasFinalResponse) return 'use_final'
  // Intentional client stop must never re-subscribe the same run.
  if (input.abortIntent === 'user') return 'reject_user_abort'
  if (input.abortIntent === 'replace') return 'reject_replace'
  // Transport drop / idle timeout / server close → recover without re-POSTing.
  if (input.capturedRunId) return 'resume_run'
  if (input.capturedTaskId) return 'poll_task'
  if (input.hasStreamError) return 'reject_error'
  return 'reject_empty'
}

interface ExecuteSseOptions {
  url: string
  method: 'GET' | 'POST'
  body?: unknown
  onProgress?: ProgressCallback
  abortPrevious: boolean
  activeControllers: Set<AbortController>
  /** Survives transport resume so replayed sequences are not applied twice. */
  seenSequences?: Map<string, number>
  pollTaskUntilComplete: (
    taskId: string,
    options: {
      intervalMs: number
      timeoutMs: number
      onProgress?: (task: TaskDetail) => void
    },
  ) => Promise<TaskDetail>
}

/**
 * Abort all active SSE subscriptions.
 * @param intent - `user` = intentional interrupt (no resume); `replace` = new request supersedes.
 */
export function abortSseSubscriptions(
  activeControllers: Set<AbortController>,
  intent: StreamAbortIntent = 'user',
): void {
  for (const controller of activeControllers) {
    controllerIntents.set(controller, intent)
    controller.abort()
  }
  activeControllers.clear()
}

export async function executeSSERequest({
  url,
  method,
  body,
  onProgress,
  abortPrevious,
  activeControllers,
  seenSequences,
  pollTaskUntilComplete,
}: ExecuteSseOptions): Promise<AgentResponse> {
  if (abortPrevious) abortSseSubscriptions(activeControllers, 'replace')
  const seen = seenSequences ?? new Map<string, number>()

  // Cookie sessions need CSRF on POST; match lib/api — refresh once on 403 CSRF.
  let csrfToken = method === 'POST' ? await getCSRFToken() : null
  let csrfRetried = false

  return new Promise((resolve, reject) => {
    const controller = new AbortController()
    activeControllers.add(controller)
    const timeoutId = setTimeout(() => {
      controllerIntents.set(controller, 'timeout')
      controller.abort()
    }, 600000)
    const cleanup = () => {
      clearTimeout(timeoutId)
      activeControllers.delete(controller)
    }
    const readAbortIntent = (): StreamAbortIntent | null =>
      controllerIntents.get(controller) ?? null

    const buildHeaders = (): Record<string, string> => {
      const headers: Record<string, string> = {
        Accept: 'text/event-stream',
        'Cache-Control': 'no-cache',
        'Content-Type': 'application/json',
      }
      if (csrfToken) headers['X-CSRF-Token'] = csrfToken
      return headers
    }

    const startFetch = (): Promise<Response> =>
      fetch(url, {
        method,
        headers: buildHeaders(),
        body: body ? JSON.stringify(body) : undefined,
        signal: controller.signal,
        credentials: 'include',
      })

    const isCsrfBody = (status: number, text: string): boolean => {
      if (status !== 403) return false
      return text.toLowerCase().includes('csrf')
    }

    startFetch()
      .then(async (response) => {
        // 必须先看 status。`clone().text()` 会把 SSE 整条流读完，
        // 200 的进度事件就永远攒到结束才进 getReader。
        if (method === 'POST' && !csrfRetried && response.status === 403) {
          const text = await response.text()
          if (isCsrfBody(response.status, text)) {
            console.warn(
              '[Agent SSE] CSRF rejection — refreshing token and retrying once',
            )
            clearCSRFToken()
            csrfToken = await getCSRFToken(true)
            csrfRetried = true
            if (csrfToken) {
              return startFetch()
            }
          }
          throw agentHttpFailure(response.status, text)
        }
        return response
      })
      .then(async (response) => {
        if (!response.ok) {
          throw agentHttpFailure(response.status, await response.text())
        }

        const reader = response.body?.getReader()
        if (!reader) throw new Error(currentCopy().errors.streamUnreadable)

        const decoder = new TextDecoder()
        let buffer = ''
        let finalResponse: AgentResponse | null = null
        let streamError: unknown = null
        let capturedTaskId: string | null = null
        let capturedRunId: string | null = null
        let currentSequence: number | null = null

        try {
          while (true) {
            const { done, value } = await reader.read()
            if (value) buffer += decoder.decode(value, { stream: !done })

            const lines = buffer.split('\n')
            buffer = done ? '' : lines.pop() || ''
            for (const line of lines) {
              if (line.startsWith('id:')) {
                const parsed = Number.parseInt(line.slice(3).trim(), 10)
                currentSequence = Number.isFinite(parsed) ? parsed : currentSequence
                continue
              }
              if (!line.startsWith('data:')) continue
              const data = line.slice(line.startsWith('data: ') ? 6 : 5).trim()
              if (!data) continue

              try {
                const event: ProgressEvent = JSON.parse(data)
                if (event.type === 'run_started' && event.runId) {
                  capturedRunId = event.runId
                }
                if (event.type === 'task_created' && event.taskId) {
                  capturedTaskId = event.taskId
                }
                if (
                  capturedRunId &&
                  currentSequence != null &&
                  !acceptRunSequence(seen, capturedRunId, currentSequence)
                ) {
                  continue
                }

                onProgress?.(event)
                if (shouldYieldSsePaint(event.type)) {
                  await new Promise<void>((resolve) => {
                    if (typeof requestAnimationFrame === 'function') {
                      requestAnimationFrame(() => resolve())
                    } else {
                      setTimeout(resolve, 0)
                    }
                  })
                }
                if (event.type === 'task_completed') {
                  finalResponse = (event as TaskCompletedEvent).response
                } else if (event.type === 'error') {
                  const errorEvent = event as ErrorEvent
                  reject(
                    new AgentStreamError(
                      errorEvent.message,
                      errorEvent.code || 'PROCESSING_ERROR',
                    ),
                  )
                  return
                }
              } catch (parseError) {
                console.warn(
                  '[AgentService] Failed to parse SSE event:',
                  parseError,
                )
              }
            }
            if (done) break
          }
        } catch (error) {
          streamError = error
        } finally {
          reader.releaseLock()
          cleanup()
        }

        const action = decideStreamDropAction({
          hasFinalResponse: !!finalResponse,
          capturedRunId,
          capturedTaskId,
          abortIntent: readAbortIntent(),
          hasStreamError: streamError != null,
        })

        switch (action) {
          case 'use_final':
            resolve(finalResponse!)
            return
          case 'reject_user_abort':
            reject(new Error('Request interrupted by user'))
            return
          case 'reject_replace':
            reject(new Error(STREAM_SUPERSEDED_MESSAGE))
            return
          case 'resume_run':
            try {
              resolve(
                await executeSSERequest({
                  url: `/api/agent/runs/${encodeURIComponent(capturedRunId!)}/stream`,
                  method: 'GET',
                  onProgress,
                  abortPrevious: false,
                  activeControllers,
                  seenSequences: seen,
                  pollTaskUntilComplete,
                }),
              )
            } catch (resumeError) {
              reject(resumeError)
            }
            return
          case 'poll_task':
            try {
              const task = await pollTaskUntilComplete(capturedTaskId!, {
                intervalMs: 2000,
                timeoutMs: 300000,
                onProgress: onProgress
                  ? (current) => {
                      onProgress({
                        type: 'progress',
                        progress: current.progress,
                        completedSteps: 0,
                        totalSteps: 0,
                        message: '',
                      })
                    }
                  : undefined,
              })
              if (
                task.status === 'completed' ||
                task.status === 'waiting_for_input'
              ) {
                resolve(buildPolledResponse(task))
              } else {
                reject(
                  new Error(
                    `Task ${capturedTaskId} ended with status ${task.status}`,
                  ),
                )
              }
            } catch (pollError) {
              reject(pollError)
            }
            return
          case 'reject_error':
            reject(streamError)
            return
          case 'reject_empty':
          default:
            reject(new Error('No completion response received'))
        }
      })
      .catch((error) => {
        cleanup()
        const intent = readAbortIntent()
        if (error.name === 'AbortError') {
          if (intent === 'user') {
            reject(new Error('Request interrupted by user'))
          } else if (intent === 'replace') {
            reject(new Error(STREAM_SUPERSEDED_MESSAGE))
          } else {
            reject(new Error('Request timed out or interrupted'))
          }
        } else {
          reject(error)
        }
      })
  })
}

function buildPolledResponse(task: TaskDetail): AgentResponse {
  const stepResults = Object.values(task.results ?? {}) as Array<{
    success?: boolean
    output?: unknown
    error?: string
  }>
  const data =
    stepResults.filter((result) => result.success).at(-1)?.output ??
    task.results
  const message =
    messageFromStepOutput(data) ??
    (task.status === 'completed'
      ? 'Task completed'
      : task.status === 'waiting_for_input'
        ? 'Waiting for input'
        : stepResults.find((result) => result.error)?.error ||
          `Task ${task.status}`)

  return {
    success: task.status === 'completed' || task.status === 'waiting_for_input',
    responseType:
      task.status === 'waiting_for_input'
        ? 'task_progress'
        : task.status === 'completed'
          ? 'task_completed'
          : 'error',
    message,
    data,
    suggestions: [],
    task: {
      taskId: task.taskId,
      status: task.status as TaskInfo['status'],
      progress: task.progress,
      pendingQuestion: task.pendingQuestion,
      ...(task.stepHistory?.length ? { stepHistory: task.stepHistory } : {}),
    },
  }
}
