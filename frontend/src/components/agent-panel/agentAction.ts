import type { ConfirmationInfo, ConfirmationStep } from '../../services/agent'

export type AgentActionRisk = 'none' | 'low' | 'medium' | 'high' | 'critical'

export type AgentActionTier = 'light' | 'preview' | 'explicit'

export interface AgentActionStep {
  id: string
  name: string
  message: string
  impact: string[]
}

export interface AgentPendingAction {
  id: string
  risk: AgentActionRisk
  tier: AgentActionTier
  prompt: string
  steps: AgentActionStep[]
  /** Absolute expiry; remaining seconds freeze when the tab is suspended. */
  expiresAtMs: number | null
}

const KNOWN_RISKS: ReadonlySet<string> = new Set([
  'none',
  'low',
  'medium',
  'high',
  'critical',
])

/** Unknown risk is critical. */
export function normalizeActionRisk(raw: string | undefined): AgentActionRisk {
  const value = raw?.trim().toLowerCase()
  if (!value) return 'critical'
  return KNOWN_RISKS.has(value) ? (value as AgentActionRisk) : 'critical'
}

export function agentActionTier(risk: AgentActionRisk): AgentActionTier {
  switch (risk) {
    case 'none':
    case 'low':
      return 'light'
    case 'medium':
      return 'preview'
    default:
      return 'explicit'
  }
}

function toStep(step: ConfirmationStep, index: number): AgentActionStep {
  return {
    id: step.stepId || `step-${index}`,
    name: step.capabilityName,
    message: step.message,
    impact: Array.isArray(step.impact) ? step.impact.filter(Boolean) : [],
  }
}

export function buildAgentPendingAction(input: {
  confirmation: ConfirmationInfo
  prompt: string
  nowMs: number
}): AgentPendingAction {
  const risk = normalizeActionRisk(input.confirmation.riskLevel)
  const ttl = input.confirmation.expiresInSeconds
  return {
    id: input.confirmation.confirmationId,
    risk,
    tier: agentActionTier(risk),
    prompt: input.prompt,
    steps: (input.confirmation.pendingSteps ?? []).map(toStep),
    expiresAtMs:
      typeof ttl === 'number' && ttl > 0 ? input.nowMs + ttl * 1000 : null,
  }
}

export function agentActionRemainingSeconds(
  action: AgentPendingAction,
  nowMs: number,
): number | null {
  if (action.expiresAtMs === null) return null
  return Math.max(0, Math.ceil((action.expiresAtMs - nowMs) / 1000))
}

export function agentActionExpired(
  action: AgentPendingAction,
  nowMs: number,
): boolean {
  return action.expiresAtMs !== null && nowMs >= action.expiresAtMs
}

export function agentActionImpacts(action: AgentPendingAction): string[] {
  const seen = new Set<string>()
  const impacts: string[] = []
  for (const step of action.steps) {
    for (const item of step.impact) {
      const key = item.trim()
      if (!key || seen.has(key)) continue
      seen.add(key)
      impacts.push(key)
    }
  }
  return impacts
}
