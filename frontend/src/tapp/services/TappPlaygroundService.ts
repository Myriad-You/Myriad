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
export interface PlaygroundMemoryTurn {
  instruction: string
  explanation: string
  origin?: 'user' | 'runtime-repair'
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

export async function generatePlaygroundProject(
  request: GeneratePlaygroundRequest,
  retryOnCsrf = true,
): Promise<GeneratePlaygroundResponse> {
  const csrfToken = await getCSRFToken()
  const response = await fetch(`${API_URL}/api/tapp-playground/generate`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(csrfToken ? { 'X-CSRF-Token': csrfToken } : {}),
    },
    body: JSON.stringify(request),
    credentials: 'include',
    // Keep equal to PLAYGROUND_PROXY_TIMEOUT_MS in frontend/astro.config.mjs
    // (planner + up to 3 repairs; each model call may take up to 720s).
    signal: AbortSignal.timeout(20 * 60 * 1000),
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
      return generatePlaygroundProject(request, false)
    }
    throw new Error(String(message))
  }
  return response.json()
}
