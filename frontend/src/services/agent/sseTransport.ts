import type {
  AgentResponse,
  ErrorEvent,
  ProgressCallback,
  ProgressEvent,
  TaskCompletedEvent,
  TaskDetail,
  TaskInfo,
} from './types'

import { hostLocaleHeaders } from '../../i18n/hostLocaleHeaders'
import { currentCopy } from '../../i18n/localeCopy'
import { authSubject } from '../../utils/authSubject'
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

/** HTTP 200 already; distinguish quota via error.code. */
export class AgentStreamError extends Error {
  readonly code: string

  constructor(message: string, code: string) {
    super(message)
    this.name = 'AgentStreamError'
    this.code = code
  }

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

export type StreamAbortIntent = 'user' | 'replace' | 'timeout'

const controllerIntents = new WeakMap<AbortController, StreamAbortIntent>()

/** Yield between token reads; React 18 batches a sync for-loop. */
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

export function decideStreamDropAction(input: {
  hasFinalResponse: boolean
  capturedRunId: string | null
  capturedTaskId: string | null
  abortIntent: StreamAbortIntent | null | undefined
  hasStreamError: boolean
}): StreamDropAction {
  if (input.hasFinalResponse) return 'use_final'
  // User abort must not re-subscribe the same run.
  if (input.abortIntent === 'user') return 'reject_user_abort'
  if (input.abortIntent === 'replace') return 'reject_replace'
  // Transport drop: resume without re-POST.
  if (input.capturedRunId) return 'resume_run'
  if (input.capturedTaskId) return 'poll_task'
  if (input.hasStreamError) return 'reject_error'
  return 'reject_empty'
}

interface ExecuteSseOptions {
  signal?: AbortSignal
  url: string
  method: 'GET' | 'POST'
  body?: unknown
  onProgress?: ProgressCallback
  abortPrevious: boolean
  activeControllers: Set<AbortController>
  /** Dedupe replayed sequences across resume. */
  seenSequences?: Map<string, number>
  pollTaskUntilComplete: (
    taskId: string,
    options: {
      intervalMs: number
      timeoutMs: number
      signal?: AbortSignal
      onProgress?: (task: TaskDetail) => void
    },
  ) => Promise<TaskDetail>
}

/** user: no resume. replace: new request supersedes. */
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
  signal = authSubject.signal,
}: ExecuteSseOptions): Promise<AgentResponse> {
  signal.throwIfAborted()
  if (abortPrevious) abortSseSubscriptions(activeControllers, 'replace')
  const seen = seenSequences ?? new Map<string, number>()

  // Cookie POST: CSRF; refresh once on 403.
  let csrfToken: string | null = null
  let csrfRetried = false
  let cleanup = () => {}

  return new Promise<AgentResponse>((resolve, reject) => {
    // Own preparation, transport and recovery, not just the fetch lifetime.
    const controller = new AbortController()
    const transport = new AbortController()
    const requestSignal = AbortSignal.any([controller.signal, transport.signal])
    activeControllers.add(controller)
    const timeoutId = setTimeout(() => {
      controllerIntents.set(transport, 'timeout')
      transport.abort()
    }, 600000)
    const abort = () => {
      reject(new Error(controllerIntents.get(controller) === 'replace'
        ? STREAM_SUPERSEDED_MESSAGE : 'Request interrupted by user'))
    }
    const invalidate = () => {
      controllerIntents.set(controller, 'user')
      controller.abort()
    }
    controller.signal.addEventListener('abort', abort, { once: true })
    signal.addEventListener('abort', invalidate, { once: true })
    cleanup = () => {
      clearTimeout(timeoutId)
      activeControllers.delete(controller)
      signal.removeEventListener('abort', invalidate)
      controller.signal.removeEventListener('abort', abort)
    }
    const readAbortIntent = (): StreamAbortIntent | null =>
      controllerIntents.get(controller) ?? controllerIntents.get(transport) ?? null

    const buildHeaders = (): Record<string, string> => {
      const headers: Record<string, string> = {
        Accept: 'text/event-stream',
        'Cache-Control': 'no-cache',
        'Content-Type': 'application/json',
        ...hostLocaleHeaders(),
      }
      if (csrfToken) headers['X-CSRF-Token'] = csrfToken
      return headers
    }

    const startFetch = (): Promise<Response> => {
      requestSignal.throwIfAborted()
      return fetch(url, {
        method,
        headers: buildHeaders(),
        body: body ? JSON.stringify(body) : undefined,
        signal: requestSignal,
        credentials: 'include',
      })
    }

    const isCsrfBody = (status: number, text: string): boolean => {
      if (status !== 403) return false
      return text.toLowerCase().includes('csrf')
    }

    Promise.resolve().then(async () => {
      controller.signal.throwIfAborted()
      csrfToken = method === 'POST' ? await getCSRFToken() : null
      return startFetch()
    })
      .then(async (response) => {
        controller.signal.throwIfAborted()
        // Do not clone().text(); it drains SSE.
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
        controller.signal.throwIfAborted()
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
            requestSignal.throwIfAborted()
            if (value) buffer += decoder.decode(value, { stream: !done })

            const lines = buffer.split('\n')
            buffer = done ? '' : lines.pop() || ''
            for (const line of lines) {
              requestSignal.throwIfAborted()
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
          clearTimeout(timeoutId)
        }

        controller.signal.throwIfAborted()
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
                  signal: controller.signal,
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
                signal: controller.signal,
                onProgress: onProgress
                  ? (current) => {
                      if (controller.signal.aborted) return
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
              controller.signal.throwIfAborted()
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
  }).finally(() => cleanup())
}

function buildPolledResponse(task: TaskDetail): AgentResponse {
  const stepResults = Object.values(task.results ?? {}) as Array<{
    success?: boolean
    output?: unknown
    error?: string
  }>
  const data =
    stepResults.findLast((result) => result.success)?.output ??
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
