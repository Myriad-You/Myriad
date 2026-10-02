import type { SessionMessage } from '../../services/agent/types'
import type { ChatMessage, TaskExecution } from './engineTypes'
import { imageUrlsFromAgentPayload } from '../../services/agent/taskEnvelope'
import { executionStepsFromHistory } from './engineTypes'
import { pendingQuestionFromMetadata } from './sessionPendingRestore'

export function restoreSessionMessage(
  m: SessionMessage,
  sessionId: string,
): ChatMessage {
  const meta = m.metadata as Record<string, unknown> | undefined
  const data = meta?.data
  const stepHistory = (meta?.task as Record<string, unknown> | undefined)
    ?.stepHistory as Array<Record<string, unknown>> | undefined
  const imageUrls = imageUrlsFromAgentPayload(data, stepHistory)

  const metaTaskId =
    (typeof meta?.taskId === 'string' && meta.taskId) ||
    (typeof meta?.task_id === 'string' && meta.task_id) ||
    m.taskId ||
    undefined
  const metaRunId =
    (typeof meta?.runId === 'string' && meta.runId) ||
    (typeof meta?.run_id === 'string' && meta.run_id) ||
    undefined
  const taskMeta = meta?.task as Record<string, unknown> | undefined
  const statusFromMeta =
    typeof taskMeta?.status === 'string' ? taskMeta.status : undefined
  const historySteps = executionStepsFromHistory(stepHistory)

  let taskExecution: TaskExecution | undefined
  if (metaTaskId || metaRunId || historySteps.length) {
    const waiting =
      statusFromMeta === 'waiting_for_input' || !!taskMeta?.pendingQuestion
    taskExecution = {
      taskId: metaTaskId || '',
      runId: metaRunId,
      status: waiting ? 'waiting' : 'completed',
      progress:
        typeof taskMeta?.progress === 'number'
          ? (taskMeta.progress as number)
          : waiting
            ? 50
            : 100,
      steps: historySteps,
    }
  }

  const pendingQuestion = pendingQuestionFromMetadata(meta)
  if (pendingQuestion && taskExecution) taskExecution.status = 'waiting'

  return {
    id: `loaded_${m.id}`,
    sessionId,
    role: m.role as ChatMessage['role'],
    content: m.content,
    createdAt: new Date(m.createdAt),
    suggestions: meta?.suggestions as string[] | undefined,
    data: data ?? undefined,
    imageUrls: imageUrls.length > 0 ? imageUrls : undefined,
    taskExecution,
    pendingQuestion,
  }
}
