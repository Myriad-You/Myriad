import type { TappCodeStructure, TappManifest } from '../types'
import { API_URL } from '../../config'
import { getCSRFToken } from '../../utils/csrf'

export interface TappPlaygroundProject {
  manifest: TappManifest
  code: TappCodeStructure & {
    page: string
    styles: string
    pageHtml: string
  }
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

/**
 * One turn in the multi-turn modification memory chain sent to the agent.
 * Successful turns include a full project snapshot; failed tails may omit it.
 */
export type PlaygroundRevisionOrigin = 'user' | 'runtime-repair' | 'manual'

export interface PlaygroundMemoryTurn {
  instruction: string
  explanation: string
  origin?: PlaygroundRevisionOrigin
  createdAt: number
  warnings?: string[]
  validation?: PlaygroundValidationReport
  /** Full project snapshot after this turn (required for successful turns). */
  project?: TappPlaygroundProject
  /** Marks a failed attempt tail entry. */
  failed?: boolean
  error?: string
}

export interface GeneratePlaygroundRequest {
  instruction: string
  currentProject?: TappPlaygroundProject
  runtimeFeedback?: string[]
  /** Chronological multi-turn memory (revisions + optional failed tail). */
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

export interface GeneratePlaygroundOptions {
  /** Optional abort signal (user cancel). Combined with the request timeout. */
  signal?: AbortSignal
  retryOnCsrf?: boolean
}

/**
 * Combine a user AbortController with a hard timeout so either can abort the fetch.
 * Caller owns `userSignal` lifecycle; timeout is internal.
 */
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
      controller.abort(new DOMException('The operation was aborted due to timeout.', 'TimeoutError'))
    }
  }, timeoutMs)
  const cleanup = () => {
    window.clearTimeout(timeoutId)
    userSignal?.removeEventListener('abort', onUserAbort)
  }
  return { signal: controller.signal, cleanup }
}

export async function generatePlaygroundProject(
  request: GeneratePlaygroundRequest,
  options: GeneratePlaygroundOptions | boolean = true,
): Promise<GeneratePlaygroundResponse> {
  // Back-compat: second arg was `retryOnCsrf = true`.
  const opts: GeneratePlaygroundOptions =
    typeof options === 'boolean'
      ? { retryOnCsrf: options }
      : { retryOnCsrf: true, ...options }
  const retryOnCsrf = opts.retryOnCsrf !== false

  const csrfToken = await getCSRFToken()
  // Keep equal to PLAYGROUND_PROXY_TIMEOUT_MS in frontend/astro.config.mjs
  // (planner + up to 3 repairs; each model call may take up to 720s).
  const { signal, cleanup } = combineAbortSignals(opts.signal, 20 * 60 * 1000)
  try {
    const response = await fetch(`${API_URL}/api/tapp-playground/generate`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(csrfToken ? { 'X-CSRF-Token': csrfToken } : {}),
      },
      body: JSON.stringify(request),
      credentials: 'include',
      signal,
    })
    if (!response.ok) {
      const error = await response.json().catch(() => ({}))
      const message = error.message || error.error || `HTTP ${response.status}`
      if (
        response.status === 403 &&
        retryOnCsrf &&
        /csrf/i.test(String(message))
      ) {
        await getCSRFToken(true)
        return generatePlaygroundProject(request, {
          ...opts,
          retryOnCsrf: false,
        })
      }
      throw new Error(String(message))
    }
    return response.json()
  } finally {
    cleanup()
  }
}
