/**
 * Restore confirmation cards and follow-up questions from persisted session
 * metadata. Live SSE already has the overlay; opening a session must rebuild
 * it from what was written on the assistant message.
 */

import type { ConfirmationStep } from '../../services/agent'
import type { AgentPendingAction } from './agentAction'
import type { ChatMessage, PendingQuestion } from './engineTypes'
import { buildAgentPendingAction } from './agentAction'

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined
}

function pendingStepsFrom(value: unknown): ConfirmationStep[] | undefined {
  if (!Array.isArray(value)) return undefined
  const steps: ConfirmationStep[] = []
  for (const item of value) {
    const row = asRecord(item)
    if (!row) continue
    steps.push({
      stepId: String(row.stepId ?? row.step_id ?? ''),
      capabilityName: String(row.capabilityName ?? row.capability_name ?? ''),
      message: String(row.message ?? ''),
      impact: Array.isArray(row.impact)
        ? row.impact.filter(
            (entry): entry is string => typeof entry === 'string',
          )
        : [],
    })
  }
  return steps.length > 0 ? steps : undefined
}

export function pendingQuestionFromMetadata(
  meta: Record<string, unknown> | undefined,
  receivedAtMs?: number,
): PendingQuestion | undefined {
  const taskMeta = asRecord(meta?.task)
  const pq =
    asRecord(meta?.pendingQuestion) || asRecord(taskMeta?.pendingQuestion)
  if (!pq || typeof pq.question !== 'string') return undefined
  const pendingSteps = pendingStepsFrom(pq.pendingSteps ?? pq.pending_steps)
  const confirmationId =
    typeof pq.confirmationId === 'string'
      ? pq.confirmationId
      : typeof pq.confirmation_id === 'string'
        ? pq.confirmation_id
        : undefined
  return {
    questionId: String(pq.questionId ?? pq.question_id ?? ''),
    confirmationId,
    questionType: String(pq.questionType ?? pq.question_type ?? 'free_text'),
    question: pq.question,
    context: typeof pq.context === 'string' ? pq.context : undefined,
    options: pq.options as PendingQuestion['options'],
    required: typeof pq.required === 'boolean' ? pq.required : undefined,
    riskLevel:
      typeof pq.riskLevel === 'string'
        ? pq.riskLevel
        : typeof pq.risk_level === 'string'
          ? pq.risk_level
          : undefined,
    expiresInSeconds:
      typeof pq.expiresInSeconds === 'number'
        ? pq.expiresInSeconds
        : typeof pq.expires_in_seconds === 'number'
          ? pq.expires_in_seconds
          : undefined,
    defaultValue:
      typeof pq.defaultValue === 'string'
        ? pq.defaultValue
        : typeof pq.default_value === 'string'
          ? pq.default_value
          : undefined,
    receivedAtMs,
    pendingSteps,
  }
}

export function restorePendingActionFromMessages(
  messages: readonly ChatMessage[],
  nowMs: number,
): AgentPendingAction | null {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index]
    if (message.role !== 'assistant' || message.selectedAnswer) continue
    const question = message.pendingQuestion
    if (!question?.confirmationId || !question.question) continue
    if (
      message.taskExecution?.status &&
      message.taskExecution.status !== 'waiting'
    ) {
      continue
    }
    return buildAgentPendingAction({
      confirmation: {
        confirmationId: question.confirmationId,
        riskLevel: question.riskLevel ?? 'critical',
        expiresInSeconds: question.expiresInSeconds ?? 0,
        pendingSteps: question.pendingSteps ?? [],
      },
      prompt: question.question,
      nowMs: question.receivedAtMs ?? nowMs,
    })
  }
  return null
}

export function restoreFollowUpQuestion(
  messages: readonly ChatMessage[],
): string | null {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index]
    if (message.selectedAnswer) continue
    const question = message.pendingQuestion
    if (!question?.question || question.confirmationId) continue
    if (message.taskExecution?.status === 'waiting') return question.question
  }
  return null
}
