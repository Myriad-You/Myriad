import type { PageContent } from '../../../contexts/PageContentContext'
import type { PerformanceDirective } from '../../../services/agent/types'
import type { PerceptionSnapshot } from '../perception/registry'
import type { MeropePerformanceEventDetail } from '../performanceEvents'

/**
 * Body is the live face the site actually has. Semantic only — no drivers.
 */

export interface BodyCapabilities {
  semantic: readonly string[]
}

export interface BodyIntent {
  runId?: string
  source?: MeropePerformanceEventDetail['source']
  speechText?: string
  messageId?: string
  performance?: PerformanceDirective
}

export interface BodyState {
  expression: string
  posture: string
  acting: string | null
  speaking: boolean
  faceVisible: boolean
  capabilities: readonly string[]
}

export interface BodyAdapter {
  capabilities: () => BodyCapabilities
  state: () => BodyState
  intend: (intent: BodyIntent) => void
}

export interface PerceptionAdapter {
  capture: (input: {
    route: string
    page: PageContent | null
    pageConsent: boolean
    selection?: string
  }) => PerceptionSnapshot[]
}
