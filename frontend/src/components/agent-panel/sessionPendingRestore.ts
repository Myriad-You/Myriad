import type { ChatMessage, PendingQuestion } from './engineTypes'

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined
}

export function pendingQuestionFromMetadata(
  meta: Record<string, unknown> | undefined,
): PendingQuestion | undefined {
  const taskMeta = asRecord(meta?.task)
  const pq =
    asRecord(meta?.pendingQuestion) ?? asRecord(taskMeta?.pendingQuestion)
  if (!pq || typeof pq.question !== 'string') return undefined
  return {
    questionId: String(pq.questionId ?? pq.question_id ?? ''),
    questionType: String(pq.questionType ?? pq.question_type ?? 'free_text'),
    question: pq.question,
    context: typeof pq.context === 'string' ? pq.context : undefined,
    options: pq.options as PendingQuestion['options'],
    required: typeof pq.required === 'boolean' ? pq.required : undefined,
    defaultValue:
      typeof pq.defaultValue === 'string'
        ? pq.defaultValue
        : typeof pq.default_value === 'string'
          ? pq.default_value
          : undefined,
  }
}

export function restoreFollowUpQuestion(
  messages: readonly ChatMessage[],
): string | null {
  const message = messages.findLast((item) => {
    if (item.selectedAnswer) return false
    const question = item.pendingQuestion
    if (!question?.question) return false
    return item.taskExecution?.status === 'waiting'
  })
  return message?.pendingQuestion?.question ?? null
}
