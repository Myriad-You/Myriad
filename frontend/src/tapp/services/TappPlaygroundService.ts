import type { TappManifest } from '../types'
import { API_URL } from '../../config'
import { hostLocaleHeaders } from '../../i18n/hostLocaleHeaders'
import { currentCopy } from '../../i18n/localeCopy'
import { ApiError, parseApiErrorBody } from '../../services/api'
import { getCSRFToken } from '../../utils/csrf'

/** 编辑态按层分框，不是运行时模块表。打包映射到 core.js / page/index.js / widget/index.js。 */
export interface TappPlaygroundCode {
  core: string
  page: string
  widget?: string
  styles: string
  pageHtml: string
  widgetHtml?: string
  widgetCSS?: string
  pageCSS?: string
  i18n?: Record<string, unknown>
  assets?: Record<string, string>
}

export interface TappPlaygroundProject {
  manifest: TappManifest
  code: TappPlaygroundCode
}

export interface PlaygroundAgentStep {
  tool: string
  status: 'success' | 'failed' | 'fallback' | 'running'
  summary: string
}

export interface PlaygroundKnowledgeSource {
  document: string
  section: string
  excerpt: string
}

export interface PlaygroundValidationReport {
  passed: boolean
  attempts: number
  checks: string[]
}

export type PlaygroundRevisionOrigin = 'user' | 'runtime-repair' | 'manual'

export interface PlaygroundMemoryTurn {
  instruction: string
  explanation: string
  origin?: PlaygroundRevisionOrigin
  createdAt: number
  warnings?: string[]
  validation?: PlaygroundValidationReport
  project?: TappPlaygroundProject
  failed?: boolean
  error?: string
}

export interface GeneratePlaygroundRequest {
  instruction: string
  currentProject?: TappPlaygroundProject
  runtimeFeedback?: string[]
  history?: PlaygroundMemoryTurn[]
}

export interface GeneratePlaygroundResponse {
  project: TappPlaygroundProject
  explanation: string
  warnings: string[]
  modelTier: 'pro'
  agentTrace: PlaygroundAgentStep[]
  knowledgeSources: PlaygroundKnowledgeSource[]
  validation: PlaygroundValidationReport
}

export interface PlaygroundStreamStepEvent {
  type: 'step'
  tool: string
  status: string
  summary: string
}

export interface PlaygroundStreamDoneEvent {
  type: 'done'
  response: GeneratePlaygroundResponse
}

export interface PlaygroundStreamErrorEvent {
  type: 'error'
  message: string
}

export type PlaygroundStreamEvent =
  | PlaygroundStreamStepEvent
  | PlaygroundStreamDoneEvent
  | PlaygroundStreamErrorEvent

export interface GeneratePlaygroundOptions {
  signal?: AbortSignal
  retryOnCsrf?: boolean
  preferStream?: boolean
  onStep?: (step: PlaygroundAgentStep) => void
}

function combineAbortSignals(
  userSignal: AbortSignal | undefined,
  timeoutMs: number,
): { signal: AbortSignal; cleanup: () => void } {
  const controller = new AbortController()
  const onUserAbort = () => {
    if (!controller.signal.aborted) {
      controller.abort(
        userSignal?.reason instanceof DOMException
          ? userSignal.reason
          : new DOMException('The operation was aborted.', 'AbortError'),
      )
    }
  }
  if (userSignal) {
    if (userSignal.aborted) {
      onUserAbort()
    } else {
      userSignal.addEventListener('abort', onUserAbort, { once: true })
    }
  }
  const timeoutId = window.setTimeout(() => {
    if (!controller.signal.aborted) {
      controller.abort(
        new DOMException(
          'The operation was aborted due to timeout.',
          'TimeoutError',
        ),
      )
    }
  }, timeoutMs)
  const cleanup = () => {
    window.clearTimeout(timeoutId)
    userSignal?.removeEventListener('abort', onUserAbort)
  }
  return { signal: controller.signal, cleanup }
}

function buildGenerateHeaders(csrfToken: string | null): HeadersInit {
  return {
    'Content-Type': 'application/json',
    ...hostLocaleHeaders(),
    ...(csrfToken ? { 'X-CSRF-Token': csrfToken } : {}),
  }
}

async function throwIfGenerateHttpError(
  response: Response,
  retryOnCsrf: boolean,
): Promise<never> {
  const error = await response.json().catch(() => ({}))
  const parsed = parseApiErrorBody(error, response.status)
  const message =
    parsed.message !== `API Error: ${response.status}`
      ? parsed.message
      : `HTTP ${response.status}`
  if (response.status === 403 && retryOnCsrf && /csrf/i.test(message)) {
    await getCSRFToken(true)
    throw Object.assign(new ApiError(message, response.status, parsed.code), {
      __csrfRetry: true as const,
    })
  }
  throw new ApiError(message, response.status, parsed.code, parsed.details, parsed.hint)
}

function isPlaygroundStreamEvent(value: unknown): value is PlaygroundStreamEvent {
  if (!value || typeof value !== 'object') return false
  const type = (value as { type?: unknown }).type
  return type === 'step' || type === 'done' || type === 'error'
}

export function consumeSseDataEvents(buffer: string): {
  events: unknown[]
  rest: string
} {
  const events: unknown[] = []
  let rest = buffer
  rest = rest.replaceAll('\r\n', '\n')
  while (true) {
    const sep = rest.indexOf('\n\n')
    if (sep < 0) break
    const frame = rest.slice(0, sep)
    rest = rest.slice(sep + 2)
    const dataLines: string[] = []
    for (const line of frame.split('\n')) {
      if (line.startsWith('data:')) {
        dataLines.push(line.slice(5).trimStart())
      }
    }
    if (dataLines.length === 0) continue
    const payload = dataLines.join('\n')
    if (!payload || payload === 'keepalive') continue
    try {
      events.push(JSON.parse(payload))
    } catch {
    }
  }
  return { events, rest }
}

async function generateViaStream(
  request: GeneratePlaygroundRequest,
  opts: GeneratePlaygroundOptions,
  signal: AbortSignal,
): Promise<GeneratePlaygroundResponse> {
  const csrfToken = await getCSRFToken()
  const response = await fetch(
    `${API_URL}/api/tapp-playground/generate-stream`,
    {
      method: 'POST',
      headers: buildGenerateHeaders(csrfToken),
      body: JSON.stringify(request),
      credentials: 'include',
      signal,
    },
  )

  if (!response.ok) {
    if (response.status === 404 || response.status === 405) {
      throw Object.assign(new Error(`HTTP ${response.status}`), {
        __streamUnavailable: true as const,
      })
    }
    await throwIfGenerateHttpError(response, opts.retryOnCsrf !== false)
  }

  if (!response.body) {
    throw Object.assign(new Error('Stream body unavailable'), {
      __streamUnavailable: true as const,
    })
  }

  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let finalResponse: GeneratePlaygroundResponse | null = null
  let streamError: string | null = null

  while (true) {
    const { done, value } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    const { events, rest } = consumeSseDataEvents(buffer)
    buffer = rest
    for (const raw of events) {
      if (!isPlaygroundStreamEvent(raw)) continue
      if (raw.type === 'step') {
        opts.onStep?.({
          tool: raw.tool,
          status: (raw.status as PlaygroundAgentStep['status']) || 'running',
          summary: raw.summary,
        })
      } else if (raw.type === 'done') {
        finalResponse = raw.response
      } else if (raw.type === 'error') {
        streamError = raw.message || currentCopy().tapp.playgroundGenerateFailed
      }
    }
    if (finalResponse || streamError) {
      try {
        await reader.cancel()
      } catch {
      }
      break
    }
  }

  if (buffer.trim()) {
    const { events } = consumeSseDataEvents(`${buffer}\n\n`)
    for (const raw of events) {
      if (!isPlaygroundStreamEvent(raw)) continue
      if (raw.type === 'step') {
        opts.onStep?.({
          tool: raw.tool,
          status: (raw.status as PlaygroundAgentStep['status']) || 'running',
          summary: raw.summary,
        })
      } else if (raw.type === 'done') {
        finalResponse = raw.response
      } else if (raw.type === 'error') {
        streamError = raw.message || currentCopy().tapp.playgroundGenerateFailed
      }
    }
  }

  if (streamError) {
    if (/cancelled/i.test(streamError) && signal.aborted) {
      throw new DOMException('The operation was aborted.', 'AbortError')
    }
    throw new Error(streamError)
  }
  if (!finalResponse) {
    if (signal.aborted) {
      throw new DOMException('The operation was aborted.', 'AbortError')
    }
    throw new Error(currentCopy().tapp.playgroundStreamIncompleteHint)
  }
  return finalResponse
}

async function generateViaOneShot(
  request: GeneratePlaygroundRequest,
  opts: GeneratePlaygroundOptions,
  signal: AbortSignal,
): Promise<GeneratePlaygroundResponse> {
  const csrfToken = await getCSRFToken()
  const response = await fetch(`${API_URL}/api/tapp-playground/generate`, {
    method: 'POST',
    headers: buildGenerateHeaders(csrfToken),
    body: JSON.stringify(request),
    credentials: 'include',
    signal,
  })
  if (!response.ok) {
    await throwIfGenerateHttpError(response, opts.retryOnCsrf !== false)
  }
  return response.json()
}

export async function generatePlaygroundProject(
  request: GeneratePlaygroundRequest,
  options: GeneratePlaygroundOptions | boolean = true,
): Promise<GeneratePlaygroundResponse> {
  const opts: GeneratePlaygroundOptions =
    typeof options === 'boolean'
      ? { retryOnCsrf: options }
      : { retryOnCsrf: true, ...options }
  const retryOnCsrf = opts.retryOnCsrf !== false
  const preferStream = opts.preferStream !== false

  const { signal, cleanup } = combineAbortSignals(opts.signal, 30 * 60 * 1000)
  try {
    if (preferStream) {
      try {
        return await generateViaStream(request, { ...opts, retryOnCsrf }, signal)
      } catch (error) {
        if (
          error &&
          typeof error === 'object' &&
          Object.hasOwn(error, '__csrfRetry') &&
          retryOnCsrf
        ) {
          return generatePlaygroundProject(request, {
            ...opts,
            retryOnCsrf: false,
          })
        }
        if (
          error &&
          typeof error === 'object' &&
          Object.hasOwn(error, '__streamUnavailable')
        ) {
          // fall through to the one-shot generator
        } else if (
          error instanceof DOMException &&
          (error.name === 'AbortError' || error.name === 'TimeoutError')
        ) {
          throw error
        } else if (
          // Network / CORS / missing route: fall back once.
          error instanceof TypeError ||
          (error instanceof Error &&
            /failed to fetch|networkerror|load failed|HTTP 404|HTTP 405/i.test(
              error.message,
            ))
        ) {
          // fall through to the one-shot generator
        } else {
          throw error
        }
      }
    }

    try {
      return await generateViaOneShot(
        request,
        { ...opts, retryOnCsrf },
        signal,
      )
    } catch (error) {
      if (
        error &&
        typeof error === 'object' &&
        Object.hasOwn(error, '__csrfRetry') &&
        retryOnCsrf
      ) {
        return generatePlaygroundProject(request, {
          ...opts,
          preferStream: false,
          retryOnCsrf: false,
        })
      }
      throw error
    }
  } finally {
    cleanup()
  }
}
