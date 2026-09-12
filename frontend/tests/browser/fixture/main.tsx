import { StrictMode, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { useVoiceRecording } from '../../../src/components/agent-panel/useVoiceRecording'
import { RigMotionCoordinator } from '../../../src/features/merope/motion/coordinator'
import { setLiveMotionGeneration } from '../../../src/features/merope/motion/liveGeneration'
import { SpeechMotionSource } from '../../../src/features/merope/motion/speechSource'
import { bindRealtimeChat } from '../../../src/features/merope/speech/realtimeChat'
import { getSpeechPipeline } from '../../../src/features/merope/speech/speechPipelineHost'
import {
  getVoicePresence,
  subscribeVoicePresence,
} from '../../../src/features/merope/speech/voicePresence'
import {
  MEROPE_SPEECH_EVENT,
  meropeSpeechEventDetail,
} from '../../../src/features/merope/speechEvents'

declare global {
  interface Window {
    __emitVoiceRun: (notice: unknown) => void
    __closeVoiceRuns: () => void
    __fakeAgoraVoice: (active: boolean) => void
    __fakeAgoraTranscript: (event: unknown) => void
  }
}

class FixtureEventSource {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 2
  readonly url: string
  readonly withCredentials: boolean
  readyState = FixtureEventSource.CONNECTING
  onmessage: ((event: MessageEvent) => void) | null = null
  onerror: (() => void) | null = null
  private readonly listeners = new Map<string, Set<() => void>>()

  constructor(url: string | URL, init?: EventSourceInit) {
    this.url = String(url)
    this.withCredentials = init?.withCredentials ?? false
    fixtureSources.push(this)
    queueMicrotask(() => {
      if (this.readyState !== FixtureEventSource.CLOSED)
        this.readyState = FixtureEventSource.OPEN
    })
  }

  addEventListener(type: string, listener: EventListenerOrEventListenerObject) {
    const call =
      typeof listener === 'function'
        ? () => listener(new Event(type))
        : () => listener.handleEvent(new Event(type))
    const listeners = this.listeners.get(type) ?? new Set()
    listeners.add(call)
    this.listeners.set(type, listeners)
  }

  close() {
    this.readyState = FixtureEventSource.CLOSED
  }

  emit(notice: unknown) {
    this.onmessage?.(
      new MessageEvent('message', { data: JSON.stringify(notice) }),
    )
  }

  finish() {
    for (const listener of this.listeners.get('closed') ?? []) listener()
  }
}

const fixtureSources: FixtureEventSource[] = []
globalThis.EventSource = FixtureEventSource as unknown as typeof EventSource
window.__emitVoiceRun = (notice) => fixtureSources.at(-1)?.emit(notice)
window.__closeVoiceRuns = () => fixtureSources.at(-1)?.finish()

// Synthetic input uses a real MediaStream and AudioWorklet, never a hardware
// microphone. The UI below controls the signal so tests need no audio files.
let input: GainNode | null = null
const tracks: MediaStreamTrack[] = []
const grants: (() => void)[] = []
navigator.mediaDevices.getUserMedia = async () => {
  if (location.search.includes('late-permission')) {
    await new Promise<void>((resolve) => {
      grants.push(resolve)
    })
  }
  const context = new AudioContext({ sampleRate: 16000 })
  const destination = context.createMediaStreamDestination()
  const oscillator = context.createOscillator()
  oscillator.frequency.value = 180
  input = context.createGain()
  input.gain.value = 0
  oscillator.connect(input).connect(destination)
  oscillator.start()
  await context.resume()
  for (const track of destination.stream.getTracks()) {
    const stop = track.stop.bind(track)
    track.stop = () => {
      stop()
      oscillator.stop()
      void context.close()
    }
    tracks.push(track)
  }
  return destination.stream
}

function Recorder() {
  const [results, setResults] = useState<string[]>([])
  const voice = useVoiceRecording(
    (text) => setResults((rows) => [...rows, text]),
    'en-US',
  )
  return (
    <section>
      <button onClick={() => void voice.enterConversation()}>Listen</button>
      <button onClick={() => void voice.startRecording()}>Record</button>
      <button onClick={() => void voice.stopRecording()}>Stop</button>
      <output data-testid="recording">{String(voice.isRecording)}</output>
      <output data-testid="conversation">{String(voice.conversation)}</output>
      <output data-testid="processing">
        {String(voice.isProcessingVoice)}
      </output>
      <ol data-testid="transcripts">
        {results.map((text, index) => (
          <li key={index}>{text}</li>
        ))}
      </ol>
    </section>
  )
}

function Fixture() {
  const [mounted, setMounted] = useState(true)
  const [events, setEvents] = useState<string[]>([])
  const [trackStates, setTrackStates] = useState('')
  const [mouth, setMouth] = useState(false)
  const [inputActive, setInputActive] = useState(false)
  const [ttsPlaying, setTtsPlaying] = useState(false)
  const [behaviors, setBehaviors] = useState(0)
  const [articulation, setArticulation] = useState('')
  useEffect(() => {
    const unbind = bindRealtimeChat({
      sessionId: () => 'fixture-chat',
      adopt: (notice) => {
        setLiveMotionGeneration(notice.sequence)
        return {
          messageId: `msg_rtc_${notice.runId}`,
          generation: notice.sequence,
        }
      },
    })
    const unsubscribe = subscribeVoicePresence(() => {
      const presence = getVoicePresence()
      setInputActive(presence.userSpeaking)
      setTtsPlaying(presence.ttsPlaying)
    })
    const source = new SpeechMotionSource(
      new RigMotionCoordinator(),
      (intent) => {
        setMouth(intent.active)
        setBehaviors(intent.behaviorPlan?.behaviors.length ?? 0)
      },
    )
    source.start()
    const listener = (event: Event) => {
      const detail = meropeSpeechEventDetail((event as CustomEvent).detail)
      if (detail?.phase === 'articulation') {
        setArticulation(detail.articulation.viseme)
      }
      if (detail && !['energy', 'articulation'].includes(detail.phase)) {
        setEvents((rows) => [...rows, `${detail.messageId}:${detail.phase}`])
      }
    }
    window.addEventListener(MEROPE_SPEECH_EVENT, listener)
    const timer = setInterval(
      () => setTrackStates(tracks.map((track) => track.readyState).join(',')),
      50,
    )
    return () => {
      unbind()
      unsubscribe()
      source.stop()
      clearInterval(timer)
      window.removeEventListener(MEROPE_SPEECH_EVENT, listener)
    }
  }, [])
  const reply = (id: string) =>
    getSpeechPipeline().speakLine({
      messageId: id,
      text: `This is the ${id} reply.`,
    })
  return (
    <main>
      {mounted && <Recorder />}
      <button onClick={() => setMounted(false)}>Unmount</button>
      <button
        onClick={() => {
          grants.shift()?.()
        }}
      >
        Grant microphone
      </button>
      <button
        onClick={() => {
          if (input) input.gain.value = 0.18
        }}
      >
        Voice on
      </button>
      <button
        onClick={() => {
          if (input) input.gain.value = 0
        }}
      >
        Voice off
      </button>
      <button onClick={() => reply('old')}>Old reply</button>
      <button onClick={() => reply('new')}>New reply</button>
      <button
        onClick={() =>
          getSpeechPipeline().applyStatus({
            available: true,
            tts_enabled: true,
            persona_speech_enabled: false,
          })
        }
      >
        Disable speech
      </button>
      <button onClick={() => getSpeechPipeline().cancel()}>Cancel reply</button>
      <button onClick={() => getSpeechPipeline().cancel('old')}>
        Cancel old reply
      </button>
      <output data-testid="tracks">{trackStates}</output>
      <output data-testid="mouth">{String(mouth)}</output>
      <output data-testid="input-active">{String(inputActive)}</output>
      <output data-testid="tts-playing">{String(ttsPlaying)}</output>
      <output data-testid="behaviors">{behaviors}</output>
      <output data-testid="articulation">{articulation}</output>
      <ol data-testid="speech-events">
        {events.map((event, index) => (
          <li key={index}>{event}</li>
        ))}
      </ol>
    </main>
  )
}

void getSpeechPipeline().probe()
createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Fixture />
  </StrictMode>,
)
