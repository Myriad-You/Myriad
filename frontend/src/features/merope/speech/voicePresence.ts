import { createStore, patchStore } from '../../../utils/store'

export interface VoicePresenceState {
  listening: boolean
  ttsPlaying: boolean
  userSpeaking: boolean
}

let listenerCount = 0
const state = createStore<VoicePresenceState>({
  listening: false,
  ttsPlaying: false,
  userSpeaking: false,
})

export const getVoicePresence = state.get

export function patchVoicePresence(
  patch: Partial<VoicePresenceState>,
): VoicePresenceState {
  return patchStore(state, patch)
}

export function subscribeVoicePresence(listener: () => void): () => void {
  const stop = state.subscribe(listener)
  listenerCount++
  let active = true
  return () => {
    if (!active) return
    active = false
    listenerCount--
    stop()
  }
}

export function voicePresenceListenerCount(): number {
  return listenerCount
}
