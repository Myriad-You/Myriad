/**
 * Only the mounted Agent engine may adopt voice runs. The transport never
 * creates its own Chat store, response handler, or director.
 */
export interface VoiceRunNotice {
  runId: string
  sessionId: string
  input: string
  providerTurnId: number
  sequence: number
}

export interface VoiceRunIdentity {
  messageId: string
  generation: number
}

interface ChatOutlet {
  sessionId: () => string | null
  adopt: (notice: VoiceRunNotice) => VoiceRunIdentity
}

let outlet: ChatOutlet | null = null

export function bindRealtimeChat(next: ChatOutlet): () => void {
  outlet = next
  return () => {
    if (outlet === next) outlet = null
  }
}

export function realtimeChatSessionId(): string | null {
  if (!outlet) throw new Error('Chat is not mounted')
  return outlet.sessionId()
}

export function adoptRealtimeChatRun(notice: VoiceRunNotice): VoiceRunIdentity {
  if (!outlet) throw new Error('Chat is not mounted')
  return outlet.adopt(notice)
}

export function parseVoiceRunNotice(value: unknown): VoiceRunNotice | null {
  if (!value || typeof value !== 'object') return null
  const v = value as Record<string, unknown>
  if (
    typeof v.runId !== 'string' ||
    !v.runId ||
    v.runId.length > 160 ||
    typeof v.sessionId !== 'string' ||
    !v.sessionId ||
    v.sessionId.length > 160 ||
    typeof v.input !== 'string' ||
    !v.input.trim() ||
    v.input.length > 32_000 ||
    typeof v.providerTurnId !== 'number' ||
    !Number.isSafeInteger(v.providerTurnId) ||
    v.providerTurnId < 0 ||
    typeof v.sequence !== 'number' ||
    !Number.isSafeInteger(v.sequence) ||
    v.sequence <= 0
  ) {
    return null
  }
  return {
    runId: v.runId,
    sessionId: v.sessionId,
    input: v.input,
    providerTurnId: v.providerTurnId,
    sequence: v.sequence,
  }
}
