/**
 * 待确认的操作 —— Action UI 的数据形状。
 *
 * 后端已经把最难的部分做完了：哪些步骤敏感、各自什么风险、影响是什么、这次确认
 * 什么时候过期，全在响应里。这里只做两件事：把风险等级归一，再决定该用哪一档
 * 界面去问。
 *
 * 分档按「问得多重」而不是按风险名字：
 * - light   可逆操作，一句话一个按钮，问过就算
 * - preview 会动数据，先把要做的事摊开再问
 * - explicit 不可逆或系统级，影响逐条列出，倒计时摆在明处，确认键不做默认项
 */

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
  /** 后端的 confirmationId，回话时原样带回去 */
  id: string
  risk: AgentActionRisk
  tier: AgentActionTier
  /** 助手自己那句话，问的就是它 */
  prompt: string
  steps: AgentActionStep[]
  /**
   * 绝对时刻。存剩余秒数就得有人不停地减，还会在标签页被挂起时停摆 ——
   * 存到期时刻，谁要显示谁自己减。
   */
  expiresAtMs: number | null
}

const KNOWN_RISKS: ReadonlySet<string> = new Set([
  'none',
  'low',
  'medium',
  'high',
  'critical',
])

/** 认不出来的风险按最重的算 —— 猜轻了要出事，猜重了只是多问一句。 */
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
    // 后端没给有效期就不显示倒计时，别自己编一个
    expiresAtMs:
      typeof ttl === 'number' && ttl > 0 ? input.nowMs + ttl * 1000 : null,
  }
}

/** 还剩几秒。没有有效期返回 null；已经过期返回 0。 */
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

/**
 * 影响条目去重后的总表。同一次确认里几个步骤常常报同一条影响，
 * 列三遍只会让人不想读。
 */
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
