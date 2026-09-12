import type { VoiceRunIdentity } from './realtimeChat'
import {
  interruptConvoSession,
  startConvoSession,
  stopConvoSession,
  subscribeConvoRuns,
} from '../../../services/speechApi'
import { faceSpeechGate } from '../faceSpeechArbitration'
import { dispatchMeropeSpeech } from '../speechEvents'
import { markTurnTraceOnce } from '../turnTrace'
import { adoptRealtimeChatRun, realtimeChatSessionId } from './realtimeChat'
import { RtcSpeechAlignment } from './rtcSpeechAlignment'
import { getSpeechPipeline } from './speechPipelineHost'
import { getVoicePresence, patchVoicePresence } from './voicePresence'

const BARGE_OPEN = 0.14
const BARGE_START_FRAMES = 4

type PtsRtcClient = import('agora-rtc-sdk-ng').IAgoraRTCClient & {
  on: (event: 'audio-pts', listener: (pts: number) => void) => void
  off: (event: 'audio-pts', listener: (pts: number) => void) => void
}

interface LiveSession {
  agentId: string
  channel: string
  client: import('agora-rtc-sdk-ng').IAgoraRTCClient
  mic: import('agora-rtc-sdk-ng').ILocalAudioTrack
  rtm: import('agora-rtm').RTMClient
  alignment: RtcSpeechAlignment
  analyser: AnalyserNode | null
  audioContext: AudioContext | null
  energyTimer: number | null
  bargeContext: AudioContext | null
  bargeTimer: number | null
  closeEvents: (() => void) | null
  rtmMessage: ((event: unknown) => void) | null
  identity: VoiceRunIdentity | null
  remoteTrack: import('agora-rtc-sdk-ng').IRemoteAudioTrack | null
  muted: boolean
  speechActive: boolean
  utterance: number
  lastVoiceAt: number
}

let live: LiveSession | null = null
let generation = 0
let starting: Promise<boolean> | null = null
const stoppedListeners = new Set<() => void>()

export function subscribeAgoraStopped(listener: () => void): () => void {
  stoppedListeners.add(listener)
  return () => {
    stoppedListeners.delete(listener)
  }
}

export function agoraConversationActive(): boolean {
  return live != null
}

export function startAgoraConversation(language: string): Promise<boolean> {
  if (starting) return starting
  if (live) return Promise.resolve(true)
  const ticket = ++generation
  const promise = openConversation(language, ticket).finally(() => {
    if (starting === promise) starting = null
  })
  starting = promise
  return promise
}

async function openConversation(
  language: string,
  ticket: number,
): Promise<boolean> {
  const session = await startConvoSession(language, realtimeChatSessionId())
  if (!session.success || !session.token) {
    throw new Error(session.error?.trim() || 'Realtime talk did not start')
  }
  let client: LiveSession['client'] | null = null
  let mic: LiveSession['mic'] | null = null
  let rtm: LiveSession['rtm'] | null = null
  let handle: LiveSession | null = null
  const current = () => ticket === generation
  const checkCurrent = () => {
    if (!current())
      throw new DOMException('Realtime start cancelled', 'AbortError')
  }
  try {
    checkCurrent()
    const [rtcModule, rtmModule] = await Promise.all([
      import('agora-rtc-sdk-ng'),
      import('agora-rtm'),
    ])
    checkCurrent()
    const AgoraRTC = rtcModule.default
    if (typeof AgoraRTC?.createClient !== 'function') {
      throw new TypeError('Agora RTC SDK failed to load')
    }
    const ptsSdk = AgoraRTC as typeof AgoraRTC & {
      setParameter: (key: 'ENABLE_AUDIO_PTS_METADATA', value: boolean) => void
    }
    ptsSdk.setParameter('ENABLE_AUDIO_PTS_METADATA', true)
    const rtc = AgoraRTC.createClient({ mode: 'rtc', codec: 'vp8' })
    client = rtc
    mic = await AgoraRTC.createMicrophoneAudioTrack()
    checkCurrent()

    const rtmClient = new rtmModule.default.RTM(
      session.app_id,
      String(session.uid),
    )
    rtm = rtmClient
    await rtmClient.login({ token: session.token })
    checkCurrent()
    const alignment = new RtcSpeechAlignment(
      String(session.agent_uid),
      session.channel,
    )
    const ptsRtc = rtc as PtsRtcClient
    const audioPtsHandler = (pts: number) => alignment.noteAudioPts(pts)
    ptsRtc.on('audio-pts', audioPtsHandler)

    const owned: LiveSession = {
      agentId: session.agent_id,
      channel: session.channel,
      client: rtc,
      mic,
      rtm: rtmClient,
      alignment,
      analyser: null,
      audioContext: null,
      energyTimer: null,
      bargeContext: null,
      bargeTimer: null,
      closeEvents: null,
      rtmMessage: null,
      identity: null,
      remoteTrack: null,
      muted: true,
      speechActive: false,
      utterance: 0,
      lastVoiceAt: 0,
    }
    handle = owned
    live = owned
    owned.closeEvents = subscribeConvoRuns(
      session.agent_id,
      (notice) => {
        if (!current() || live !== owned) return
        endRemoteSpeech(owned, true)
        getSpeechPipeline().cancel()
        try {
          void owned.alignment.select(notice.providerTurnId, language)
          owned.identity = adoptRealtimeChatRun(notice)
          owned.muted = false
          owned.remoteTrack?.play()
        } catch (error) {
          console.error('[agoraConversation] Chat outlet unavailable:', error)
          void stopAgoraConversation()
        }
      },
      () => {
        if (current() && live === owned) void stopAgoraConversation()
      },
    )

    rtc.on('user-published', (user, mediaType) => {
      if (
        mediaType !== 'audio' ||
        String(user.uid) !== String(session.agent_uid) ||
        !current() ||
        live !== owned
      ) {
        return
      }
      void rtc
        .subscribe(user, 'audio')
        .then(() => {
          const track = user.audioTrack
          if (!track || !current() || live !== owned) return
          if (owned.remoteTrack && owned.remoteTrack !== track) {
            try {
              owned.remoteTrack.stop()
            } catch {
            }
          }
          owned.remoteTrack = track
          if (!owned.muted) track.play()
          attachEnergyTap(owned, track.getMediaStreamTrack())
        })
        .catch((error: unknown) => {
          if (current())
            console.error('[agoraConversation] subscribe failed:', error)
        })
    })

    rtc.on('user-unpublished', (user, mediaType) => {
      if (
        mediaType !== 'audio' ||
        String(user.uid) !== String(session.agent_uid) ||
        live !== owned
      ) {
        return
      }
      try {
        owned.remoteTrack?.stop()
      } catch {
      }
      owned.remoteTrack = null
      if (owned.energyTimer != null) window.clearInterval(owned.energyTimer)
      owned.energyTimer = null
      void owned.audioContext?.close().catch(() => {})
      owned.audioContext = null
      owned.analyser = null
      if (owned.speechActive) endRemoteSpeech(owned, true)
    })

    const onRtmMessage = (event: unknown) => {
      if (!current() || live !== owned) return
      void owned.alignment.update(event).catch((error: unknown) => {
        console.error('[agoraConversation] transcript alignment failed:', error)
      })
    }
    owned.rtmMessage = onRtmMessage
    rtmClient.addEventListener(
      'message',
      onRtmMessage as import('agora-rtm').RTMEvents.RTMClientEventMap['message'],
    )
    await rtmClient.subscribe(session.channel)
    checkCurrent()
    await rtc.join(session.app_id, session.channel, session.token, session.uid)
    checkCurrent()

    await rtc.publish([mic])
    checkCurrent()
    attachLocalBargeIn(owned, mic.getMediaStreamTrack())
    patchVoicePresence({
      listening: true,
      ttsPlaying: false,
      userSpeaking: false,
    })
    return true
  } catch (error) {
    if (handle && live === handle) live = null
    if (handle) {
      await releaseMedia(handle)
    } else {
      if (rtm) await releaseRtm(rtm, session.channel)
      mic?.stop()
      mic?.close()
      await client?.leave().catch(() => {})
    }
    try {
      await stopConvoSession(session.agent_id)
    } catch {
    }
    if (!current()) return false
    patchVoicePresence({
      listening: false,
      ttsPlaying: false,
      userSpeaking: false,
    })
    throw error
  }
}

export async function stopAgoraConversation(): Promise<void> {
  generation += 1
  starting = null
  const session = live
  live = null
  for (const listener of stoppedListeners) listener()
  patchVoicePresence({ listening: false, userSpeaking: false })
  if (!session) return
  endRemoteSpeech(session, true)
  await releaseMedia(session)
  try {
    await stopConvoSession(session.agentId)
  } catch (error) {
    console.error('[agoraConversation] stop failed:', error)
  }
}

async function releaseMedia(session: LiveSession): Promise<void> {
  session.closeEvents?.()
  session.closeEvents = null
  if (session.rtmMessage) {
    try {
      session.rtm.removeEventListener(
        'message',
        session.rtmMessage as import('agora-rtm').RTMEvents.RTMClientEventMap['message'],
      )
    } catch {
    }
    session.rtmMessage = null
  }
  session.remoteTrack?.stop()
  if (session.energyTimer != null) window.clearInterval(session.energyTimer)
  if (session.bargeTimer != null) window.clearInterval(session.bargeTimer)
  session.energyTimer = null
  session.bargeTimer = null
  session.alignment.clear()
  session.client.removeAllListeners()
  try {
    session.mic.stop()
    session.mic.close()
  } catch {
  }
  try {
    await session.client.leave()
  } catch {
  }
  await releaseRtm(session.rtm, session.channel)
  try {
    await session.audioContext?.close()
  } catch {
  }
  try {
    await session.bargeContext?.close()
  } catch {
  }
}

async function releaseRtm(
  rtm: import('agora-rtm').RTMClient,
  channel: string,
): Promise<void> {
  try {
    await rtm.unsubscribe(channel)
  } catch {
  }
  try {
    await rtm.logout()
  } catch {
  }
}

function attachEnergyTap(session: LiveSession, track: MediaStreamTrack): void {
  if (session.energyTimer != null) window.clearInterval(session.energyTimer)
  void session.audioContext?.close().catch(() => {})
  const audioContext = new AudioContext()
  const source = audioContext.createMediaStreamSource(new MediaStream([track]))
  const analyser = audioContext.createAnalyser()
  analyser.fftSize = 256
  source.connect(analyser)
  session.audioContext = audioContext
  session.analyser = analyser
  const bins = new Uint8Array(analyser.frequencyBinCount)
  session.energyTimer = window.setInterval(() => {
    if (live !== session || session.muted || !session.identity) return
    analyser.getByteTimeDomainData(bins)
    let sum = 0
    for (const value of bins) {
      const centered = (value - 128) / 128
      sum += centered * centered
    }
    const rawEnergy = Math.sqrt(sum / bins.length)
    const energy = Math.min(1, rawEnergy * 2.4)
    const now = performance.now()
    const identity = session.identity
    if (rawEnergy > 0.025) {
      session.lastVoiceAt = now
      if (!session.speechActive) {
        session.speechActive = true
        session.utterance += 1
        faceSpeechGate.beginIncoming('chat', identity.messageId)
        dispatchMeropeSpeech({
          phase: 'start',
          messageId: identity.messageId,
          generation: identity.generation,
          source: 'reply',
          utteranceId: remoteUtteranceId(session),
        })
        patchVoicePresence({ ttsPlaying: true })
        markTurnTraceOnce('first_audio')
      }
    }
    if (session.speechActive) {
      const articulation = session.alignment.sample(energy, now)
      dispatchMeropeSpeech({
        phase: articulation ? 'articulation' : 'energy',
        messageId: identity.messageId,
        generation: identity.generation,
        source: 'reply',
        utteranceId: remoteUtteranceId(session),
        ...(articulation ? { articulation } : { energy }),
      })
      if (now - session.lastVoiceAt >= 450) endRemoteSpeech(session, false)
    }
  }, 50)
}

export async function interruptAgoraConversation(): Promise<void> {
  if (!live) return
  const session = live
  session.muted = true
  session.alignment.cancel()
  session.remoteTrack?.stop()
  endRemoteSpeech(session, true)
  try {
    await interruptConvoSession(session.agentId)
  } catch (error) {
    console.error('[agoraConversation] interrupt failed:', error)
  }
}

function remoteUtteranceId(session: LiveSession): string {
  return `rtc-${session.identity?.messageId}-${session.utterance}`
}

function endRemoteSpeech(session: LiveSession, cancelled: boolean): void {
  if (!session.identity) return
  if (session.speechActive) patchVoicePresence({ ttsPlaying: false })
  session.speechActive = false
  dispatchMeropeSpeech({
    phase: cancelled ? 'cancel' : 'end',
    messageId: session.identity.messageId,
    generation: session.identity.generation,
    source: 'reply',
    utteranceId: remoteUtteranceId(session),
  })
  faceSpeechGate.endIncoming('chat', session.identity.messageId)
}

function attachLocalBargeIn(
  session: LiveSession,
  track: MediaStreamTrack,
): void {
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
