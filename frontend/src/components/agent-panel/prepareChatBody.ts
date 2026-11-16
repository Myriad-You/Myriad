import type { ChatMessage } from './engineTypes'
import { safeRandomUUID } from '../../utils/uuid'
import { BODY_PAGE_CHARS, BodyWriter, prepareMessageBody } from './messageBody'
import { SummaryTextStream } from './summaryTextStream'

/** Ingress-only normalization; neither rendering nor token updates parse full text. */
export async function prepareAssistantBody(content: string, signal: AbortSignal) {
  const reply = new BodyWriter(safeRandomUUID(), signal)
  const thought = new BodyWriter(safeRandomUUID(), signal)
  const parser = new SummaryTextStream()
  try {
    for (let at = 0; at < content.length; at += BODY_PAGE_CHARS) {
      const part = parser.push(content.slice(at, at + BODY_PAGE_CHARS), at + BODY_PAGE_CHARS >= content.length)
      if (part.content) await reply.append(part.content)
      if (part.thought) await thought.append(part.thought)
    }
    signal.throwIfAborted()
    return { ...reply.snapshot(), thought: thought.snapshot() }
  } catch (error) {
    await Promise.all([reply.discardStored(), thought.discardStored()])
    throw error
  } finally { reply.finish(); thought.finish() }
}
export async function prepareChatBody(message: ChatMessage, signal: AbortSignal): Promise<ChatMessage> {
  if (message.role !== 'assistant') return { ...message, ...await prepareMessageBody(message.content, signal) }
  const { thought, ...reply } = await prepareAssistantBody(message.content, signal)
  if (!thought.content && !thought.body) return { ...message, ...reply }
  return { ...message, ...reply, taskExecution: {
    ...(message.taskExecution ?? { taskId: '', status: 'completed', progress: 100, steps: [] }),
    reasoning: thought.content, reasoningBody: thought.body,
  } }
}
