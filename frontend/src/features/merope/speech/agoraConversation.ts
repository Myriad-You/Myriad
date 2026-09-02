import {
  interruptConvoSession,
  startConvoSession,
  stopConvoSession,
} from '../../../services/speechApi'
import { liveMotionGeneration } from '../motion/liveGeneration'
import { dispatchMeropeSpeech } from '../speechEvents'
import { getVoicePresence, patchVoicePresence } from './voicePresence'

const BARGE_OPEN = 0.14
const BARGE_START_FRAMES = 4

interface LiveSession {
  agentId: string
  client: import('agora-rtc-sdk-ng').IAgoraRTCClient
  mic: import('agora-rtc-sdk-ng').ILocalAudioTrack
  analyser: AnalyserNode | null
  audioContext: AudioContext | null
  energyTimer: number | null
  bargeContext: AudioContext | null
  bargeTimer: number | null
}

let live: LiveSession | null = null

export function agoraConversationActive(): boolean {
  return live != null
}

export async function startAgoraConversation(language: string): Promise<boolean> {
  if (live) return true
  const session = await startConvoSession(language)
  if (!session.success || !session.token) {
    throw new Error(session.error?.trim() || 'Realtime talk did not start')
  }
  try {
    const mod = await import('agora-rtc-sdk-ng')
    const AgoraRTC = mod.default
    if (typeof AgoraRTC?.createClient !== 'function') {
      throw new TypeError('Agora RTC SDK failed to load')
    }
    const client = AgoraRTC.createClient({ mode: 'rtc', codec: 'vp8' })
    const mic = await AgoraRTC.createMicrophoneAudioTrack()
    await client.join(session.app_id, session.channel, session.token, session.uid)

    const handle: LiveSession = {
      agentId: session.agent_id,
      client,
      mic,
      analyser: null,
      audioContext: null,
      energyTimer: null,
      bargeContext: null,
      bargeTimer: null,
    }
    live = handle

    client.on('user-published', (user, mediaType) => {
      if (mediaType !== 'audio' || !live || live.agentId !== session.agent_id) return
      void client.subscribe(user, 'audio').then(() => {
        const track = user.audioTrack
        if (!track || !live) return
        track.play()
        attachEnergyTap(live, track.getMediaStreamTrack(), session.agent_id)
      })
    })

    await client.publish([mic])
    attachLocalBargeIn(handle, mic.getMediaStreamTrack())
    patchVoicePresence({ listening: true, ttsPlaying: false, userSpeaking: false })
    return true
  } catch (error) {
    live = null
    try {
      await stopConvoSession(session.agent_id)
    } catch {
      // already stopped
    }
    throw error
  }
}

export async function stopAgoraConversation(): Promise<void> {
  const session = live
  live = null
  if (!session) return
  if (session.energyTimer != null) window.clearInterval(session.energyTimer)
  if (session.bargeTimer != null) window.clearInterval(session.bargeTimer)
  try {
    session.mic.stop()
    session.mic.close()
  } catch {
    // already closed
  }
  try {
    await session.client.leave()
  } catch {
    // already left
  }
  try {
    await session.audioContext?.close()
  } catch {
    // already closed
  }
  try {
    await session.bargeContext?.close()
  } catch {
    // already closed
  }
  patchVoicePresence({ listening: false, ttsPlaying: false, userSpeaking: false })
  try {
    await stopConvoSession(session.agentId)
  } catch (error) {
    console.error('[agoraConversation] stop failed:', error)
  }
}

function attachEnergyTap(
  session: LiveSession,
  track: MediaStreamTrack,
  agentId: string,
): void {
  const audioContext = new AudioContext()
  const source = audioContext.createMediaStreamSource(new MediaStream([track]))
  const analyser = audioContext.createAnalyser()
  analyser.fftSize = 256
  source.connect(analyser)
  session.audioContext = audioContext
  session.analyser = analyser
  const bins = new Uint8Array(analyser.frequencyBinCount)
  session.energyTimer = window.setInterval(() => {
    if (!live || live.agentId !== agentId) return
    analyser.getByteTimeDomainData(bins)
    let sum = 0
    for (const value of bins) {
      const centered = (value - 128) / 128
      sum += centered * centered
    }
    const energy = Math.sqrt(sum / bins.length)
    const speaking = energy > 0.04
    patchVoicePresence({ ttsPlaying: speaking })
    if (speaking) {
      dispatchMeropeSpeech({
        phase: 'energy',
        messageId: `convo-${agentId}`,
        source: 'reply',
        utteranceId: `convo-${agentId}`,
        generation: liveMotionGeneration(),
        energy,
      })
    }
  }, 50)
}

async function interruptAgoraConversation(): Promise<void> {
  if (!live) return
  try {
    await interruptConvoSession(live.agentId)
  } catch (error) {
    console.error('[agoraConversation] interrupt failed:', error)
  }
}

function attachLocalBargeIn(session: LiveSession, track: MediaStreamTrack): void {
  const audioContext = new AudioContext()
  const source = audioContext.createMediaStreamSource(new MediaStream([track]))
  const analyser = audioContext.createAnalyser()
  analyser.fftSize = 256
  source.connect(analyser)
  session.bargeContext = audioContext
  const bins = new Uint8Array(analyser.frequencyBinCount)
  let openFrames = 0
  let sent = false
  session.bargeTimer = window.setInterval(() => {
    if (!live || live.agentId !== session.agentId) return
    analyser.getByteTimeDomainData(bins)
    let sum = 0
    for (const value of bins) {
      const centered = (value - 128) / 128
      sum += centered * centered
    }
    const energy = Math.sqrt(sum / bins.length)
    if (energy >= BARGE_OPEN) {
      openFrames += 1
      if (
        openFrames >= BARGE_START_FRAMES &&
        !sent &&
        getVoicePresence().ttsPlaying
      ) {
        sent = true
        void interruptAgoraConversation()
      }
    } else {
      openFrames = 0
      sent = false
    }
  }, 50)
}
