export type ComposerActionKind = 'voice' | 'send' | 'stop'

/**
 * How long a send may wait for its run to show up before the button lets go
 * of stop. Sends that never start a run (guest hint, answering a question)
 * fall back to the idle button after this.
 */
export const COMPOSER_SEND_SETTLE_MS = 1200

export function composerActionKind(input: {
  hasText: boolean
  hasAttachments?: boolean
  busy: boolean
  /**
   * Just sent, run not yet reported. The engine marks the lane loading only
   * after the body is prepared, so without this the button would flash
   * voice (or vanish) for a frame between send and stop.
   */
  sending?: boolean
  speechAvailable: boolean
  voiceLocked: boolean
  conversation?: boolean
}): ComposerActionKind | null {
  const hasPayload = input.hasText || !!input.hasAttachments
  if (hasPayload) return 'send'
  if (input.conversation) return 'voice'
  if (input.busy || input.sending) return 'stop'
  if (input.voiceLocked || input.speechAvailable) return 'voice'
  return null
}
