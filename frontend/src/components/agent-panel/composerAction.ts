export type ComposerActionKind = 'voice' | 'send' | 'stop'

/**
 * 右侧那一枚动作的身份：有字或附件是发送；连续对话中保持语音；忙着且没字是
 * 终止（思考中也不回语音）；空着且语音可用才是语音。没配置语音服务时返回
 * null，按钮整枚不画。
 */
export function composerActionKind(input: {
  hasText: boolean
  /** 框里没字但挂了附件，也该是发送 */
  hasAttachments?: boolean
  busy: boolean
  speechAvailable: boolean
  voiceLocked: boolean
  /** 连续对话要把麦克风留在场上，不能被终止顶掉 */
  conversation?: boolean
}): ComposerActionKind | null {
  const hasPayload = input.hasText || !!input.hasAttachments
  if (hasPayload) return 'send'
  if (input.conversation) return 'voice'
  if (input.busy && !hasPayload) return 'stop'
  if (input.voiceLocked || input.speechAvailable) return 'voice'
  return null
}
