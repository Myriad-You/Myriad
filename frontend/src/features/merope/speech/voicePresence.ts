export interface VoicePresenceState {
  listening: boolean
  ttsPlaying: boolean
  userSpeaking: boolean
}

const listeners = new Set<() => void>()
let state: VoicePresenceState = {
  listening: false,
  ttsPlaying: false,
  userSpeaking: false,
}

export function getVoicePresence(): VoicePresenceState {
  return state
}

export function patchVoicePresence(
  patch: Partial<VoicePresenceState>,
): VoicePresenceState {
  state = { ...state, ...patch }
  for (const listener of listeners) listener()
  return state
}

export function subscribeVoicePresence(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function voicePresenceListenerCount(): number {
  return listeners.size
}
