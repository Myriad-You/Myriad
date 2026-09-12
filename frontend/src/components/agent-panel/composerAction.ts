export type ComposerActionKind = 'voice' | 'send' | 'stop'

export function composerActionKind(input: {
  hasText: boolean
  hasAttachments?: boolean
  busy: boolean
  speechAvailable: boolean
  voiceLocked: boolean
  conversation?: boolean
}): ComposerActionKind | null {
  const hasPayload = input.hasText || !!input.hasAttachments
  if (hasPayload) return 'send'
  if (input.conversation) return 'voice'
  if (input.busy && !hasPayload) return 'stop'
  if (input.voiceLocked || input.speechAvailable) return 'voice'
  return null
}
