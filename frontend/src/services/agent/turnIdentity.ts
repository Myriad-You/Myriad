/** Do not invent turnId. generation is in-memory only. */

export type TurnPhase =
  | 'created'
  | 'generating'
  | 'responding'
  | 'speaking'
  | 'completed'
  | 'cancelled'
  | 'superseded'
  | 'failed'

const TERMINAL: readonly TurnPhase[] = [
  'completed',
  'cancelled',
  'superseded',
  'failed',
]

export function isTerminalTurnPhase(phase: TurnPhase): boolean {
  return TERMINAL.includes(phase)
}

export interface TurnIdentity {
  runId: string | null
  generation: number
  messageId: string
}

let messageSequence = 0

/** ASR may commit twice in 1ms; client ids must stay distinct. */
export function nextAgentMessageId(role: 'user' | 'assistant'): string {
  return `msg_${role}_${Date.now()}_${++messageSequence}`
}

/** In-memory only; never persisted. */
export class ChatTurnClock {
  private generation = 0

  next(): number {
    this.generation += 1
    return this.generation
  }

  current(): number {
    return this.generation
  }
}

export function isCurrentChatGeneration(
  eventGeneration: number,
  currentGeneration: number,
): boolean {
  return eventGeneration === currentGeneration
}

export function acceptRunSequence(
  lastByRun: Map<string, number>,
  runId: string,
  sequence: number,
): boolean {
  if (!Number.isFinite(sequence) || sequence <= 0) return false
  const last = lastByRun.get(runId) ?? 0
  if (sequence <= last) return false
  lastByRun.set(runId, sequence)
  return true
}

export const STREAM_SUPERSEDED_MESSAGE = 'Request superseded by a newer request'
export const STREAM_INTERRUPTED_MESSAGE = 'Request interrupted by user'

export function isStreamSupersededError(error: unknown): boolean {
  return error instanceof Error && error.message === STREAM_SUPERSEDED_MESSAGE
}

export function isUserInterruptError(error: unknown): boolean {
  return error instanceof Error && error.message === STREAM_INTERRUPTED_MESSAGE
}
