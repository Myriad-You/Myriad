import type { AgentMessage } from './agentMessages'
import type { ChatMessage } from './engineTypes'
import { setAgentMessages, updateAgentMessage } from './agentMessages'
import {
  nonemptyContent,
  peelThoughtFromContent,
  splitThinkContent,
} from './agentThinking'
import { changedMessageIndices } from './useMessageState'

function projectState(message: ChatMessage): AgentMessage['state'] {
  if (message.taskExecution?.status === 'error') return 'error'
  // waiting is not streaming — that would draw a cursor after the question
  if (
    message.taskExecution?.status === 'processing' ||
    message.taskExecution?.status === 'cancelling'
  ) {
    return 'streaming'
  }
  return undefined
}

function projectSteps(message: ChatMessage): AgentMessage['steps'] {
  const live = message.taskExecution?.steps
  if (live?.length) {
    return live.map((step) => ({
      id: step.id,
      name: step.name,
      status:
        step.status === 'completed'
          ? ('done' as const)
          : step.status === 'error'
            ? ('error' as const)
            : step.status === 'running'
              ? ('running' as const)
              : ('pending' as const),
      ...(typeof step.durationMs === 'number'
        ? { durationMs: step.durationMs }
        : {}),
      ...(step.message ? { note: step.message } : {}),
    }))
  }
  const plan = message.taskExecution?.planStepDescriptions
  if (plan?.length) {
    return plan.map((name, index) => ({
      id: `plan-${index}`,
      name,
      status: 'pending' as const,
    }))
  }
  return undefined
}

const TERMINAL_STATUS = /^(完成|The task finished|Processing failed)$/i

export function workOfferFromData(
  data: unknown,
): { input: string } | undefined {
  if (!data || typeof data !== 'object') return undefined
  const offer = (data as Record<string, unknown>).workOffer
  if (!offer || typeof offer !== 'object') return undefined
  const input = (offer as Record<string, unknown>).input
  if (typeof input !== 'string') return undefined
  const trimmed = input.trim()
  return trimmed ? { input: trimmed } : undefined
}

export function projectThought(message: ChatMessage): string | undefined {
  const exec = message.taskExecution
  const content = message.content.trim()
  const reasoning =
    exec?.reasoning?.trim() ||
    exec?.debugTrace?.plannerDecision?.reasoning?.trim()
  if (reasoning && reasoning !== content) return reasoning

  const status = exec?.statusMessage?.trim()
  if (!status || TERMINAL_STATUS.test(status) || status === content) {
    return undefined
  }
  return status
}

export function projectAgentMessage(message: ChatMessage): AgentMessage {
  const steps = projectSteps(message)
  const tagged = splitThinkContent(message.content)
  const fromExec = projectThought({ ...message, content: tagged.content })
  const fromTags = tagged.thought.trim() || undefined
  const thought =
    fromTags && (!fromExec || fromTags.length >= fromExec.length)
      ? fromTags
      : fromExec
  const content = nonemptyContent(
    peelThoughtFromContent(tagged.content, thought ?? ''),
  )
  const workOffer = workOfferFromData(message.data)
  return {
    workPlan: message.taskExecution?.workPlan ?? workPlanFromData(message.data),
    id: message.id,
    role: message.role,
    content,
    body: message.body,
    bodyUnavailable: message.bodyUnavailable,
    thoughtBody: message.taskExecution?.reasoningBody,
    state: projectState(message),
    ...(message.imageUrls?.length ? { imageUrls: message.imageUrls } : {}),
    ...(message.attachments?.length
      ? {
          attachments: message.attachments.map((item) => ({
            id: item.id,
            name: item.name,
            mime: item.mime,
            size: item.size,
            ...(item.previewUrl ? { previewUrl: item.previewUrl } : {}),
          })),
        }
      : {}),
    at: message.createdAt.getTime(),
    ...(message.suggestions?.length
      ? { suggestions: message.suggestions }
      : {}),
    ...(workOffer ? { workOffer } : {}),
    ...(message.pendingQuestion
      ? {
          question: {
            id: message.pendingQuestion.questionId,
            text: message.pendingQuestion.question,
            ...(message.pendingQuestion.context
              ? { context: message.pendingQuestion.context }
              : {}),
            ...(message.pendingQuestion.options?.length
              ? { options: message.pendingQuestion.options }
              : {}),
            ...(message.selectedAnswer
              ? { answered: message.selectedAnswer }
              : {}),
            ...(message.pendingQuestion.detail
              ? { detail: message.pendingQuestion.detail }
              : {}),
          },
        }
      : {}),
    ...(steps ? { steps } : {}),
    ...(thought ? { thought } : {}),
  }
}

export function workPlanFromData(
  data: unknown,
): import('../../services/agent/types').WorkPlanItem[] | undefined {
  if (!data || typeof data !== 'object') return undefined
  const plan = (data as Record<string, unknown>).workPlan
  if (!Array.isArray(plan)) return undefined
  return plan
    .filter(
      (item): item is import('../../services/agent/types').WorkPlanItem =>
        !!item &&
        typeof item.description === 'string' &&
        ['pending', 'in_progress', 'completed'].includes(item.status),
    )
    .slice(0, 12)
}

let previousChats: readonly ChatMessage[] | null = null

export function syncProjectedMessages(chats: readonly ChatMessage[]): void {
  const changed = previousChats && changedMessageIndices(previousChats, chats)
  if (changed) {
    for (const index of changed)
      updateAgentMessage(projectAgentMessage(chats[index]))
  } else {
    setAgentMessages(chats.map(projectAgentMessage))
  }
  previousChats = chats
}
