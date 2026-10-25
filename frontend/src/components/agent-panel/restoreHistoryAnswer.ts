import type { SessionMessage, TaskDetail } from '../../services/agent/types'
import type { ChatMessage } from './engineTypes'
import { readConfirmationDetail } from './confirmationDetail'
import { restoreSessionMessage } from './sessionHistoryMessage'

export interface HistoryAnswerSource {
  sessionId: string
  page: number
  questionId: string
}

/** History is a display page, not execution state. Revalidate before restoring one control. */
export async function restoreHistoryAnswer(
  messageId: string,
  source: HistoryAnswerSource,
  signal: AbortSignal,
  api: {
    getSessionMessages: (session: string, page: number, limit: number, signal: AbortSignal) => Promise<SessionMessage[]>
    getTask: (task: string, signal: AbortSignal) => Promise<TaskDetail>
  },
): Promise<ChatMessage | null> {
  if (signal.aborted || !Number.isSafeInteger(source.page) || source.page < 1) return null
  const rows = await api.getSessionMessages(source.sessionId, source.page, 40, signal)
  if (signal.aborted) return null
  const row = rows.find(row => `loaded_${row.id}` === messageId && row.role === 'assistant')
  if (!row) return null
  const restored = restoreSessionMessage(row, source.sessionId)
  const taskId = restored.taskExecution?.taskId
  const question = restored.pendingQuestion
  if (!taskId || !question || question.questionId !== source.questionId) return null
  const task = await api.getTask(taskId, signal)
  if (signal.aborted || task.taskId !== taskId || task.status !== 'waiting_for_input' ||
    task.pendingQuestion?.questionId !== question.questionId) { return null
}
  return {
    id: messageId, sessionId: source.sessionId, role: 'assistant', createdAt: restored.createdAt,
    content: '',
    pendingQuestion: { ...task.pendingQuestion, detail: readConfirmationDetail(task.pendingQuestion.detail) },
    taskExecution: { taskId, status: 'waiting', progress: task.progress, steps: [] },
  }
}
