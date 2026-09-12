import type { VoiceInputTiming } from '../../features/merope/turnTrace'
import { useCallback, useEffect, useRef, useState } from 'react'
import {
  agoraConversationActive,
  startAgoraConversation,
  stopAgoraConversation,
  subscribeAgoraStopped,
} from '../../features/merope/speech/agoraConversation'
import {
  isSubmittableTranscript,
  pcmToWav,
} from '../../features/merope/speech/audioWav'
import { getSpeechPipeline } from '../../features/merope/speech/speechPipelineHost'
import { TranscriptionQueue } from '../../features/merope/speech/transcriptionQueue'
import { UtteranceCapture } from '../../features/merope/speech/utteranceCapture'
import {
  getVoicePresence,
  patchVoicePresence,
} from '../../features/merope/speech/voicePresence'
import {
  dropPendingTurnTrace,
  stageVoiceInputTrace,
} from '../../features/merope/turnTrace'
import { getDefaultLocale } from '../../i18n/locales'
import {
  audioToBase64,
  getSpeechStatus,
  speechToText,
} from '../../services/speechApi'
import {
  getAgentPanelMode,
  setAgentPanelMode,
  subscribeAgentPanelMode,
} from './agentPanelMode'

interface RecorderState {
  audioContext: AudioContext
  stream: MediaStream
  workletNode: AudioWorkletNode
  muteNode: GainNode
  pcmData: Float32Array[]
  capture: UtteranceCapture
  startedAt: number
}

const LOCALE_ENGINE_MAP: Record<string, string> = {
  'zh-CN': '16k_zh',
  'zh-TW': '16k_zh',
  'en-US': '16k_en',
  'ja-JP': '16k_ja',
  'ko-KR': '16k_ko',
  'fr-FR': '16k_en',
  'de-DE': '16k_en',
}

const WORKLET_PROCESSOR_NAME = 'pcm-capture-processor'

const WORKLET_SOURCE = `
class PcmCaptureProcessor extends AudioWorkletProcessor {
  process(inputs) {
    const channel = inputs[0] && inputs[0][0]
    if (channel && channel.length > 0) {
      const copy = new Float32Array(channel.length)
      copy.set(channel)
      this.port.postMessage(copy, [copy.buffer])
    }
    return true
  }
}
registerProcessor('${WORKLET_PROCESSOR_NAME}', PcmCaptureProcessor)
`

async function createPcmCaptureNode(
  audioContext: AudioContext,
): Promise<AudioWorkletNode> {
  if (!audioContext.audioWorklet) {
    throw new Error('AudioWorklet is not supported in this browser')
  }
  const blob = new Blob([WORKLET_SOURCE], { type: 'application/javascript' })
  const url = URL.createObjectURL(blob)
  try {
    await audioContext.audioWorklet.addModule(url)
  } finally {
    URL.revokeObjectURL(url)
  }
  return new AudioWorkletNode(audioContext, WORKLET_PROCESSOR_NAME)
}

function cleanupRecorder(recorder: RecorderState) {
  try {
    recorder.workletNode.port.onmessage = null
    recorder.workletNode.disconnect()
  } catch {
    /* already disconnected */
  }
  try {
    recorder.muteNode.disconnect()
  } catch {
    /* already disconnected */
  }
  recorder.stream.getTracks().forEach((track) => track.stop())
  void recorder.audioContext.close()
}

export function useVoiceRecording(
  onResult: (text: string) => void,
  locale: string = getDefaultLocale(),
) {
  const [speechAvailable, setSpeechAvailable] = useState(false)
  const convoRtcRef = useRef(false)
  const [isRecording, setIsRecording] = useState(false)
  const [isProcessingVoice, setIsProcessingVoice] = useState(false)
  const [conversation, setConversation] = useState(false)
  const recorderRef = useRef<RecorderState | null>(null)
  const isRecordingRef = useRef(false)
  const conversationRef = useRef(false)
  const lifecycleRef = useRef(0)
  const openingRef = useRef(false)
  const mountedRef = useRef(true)
  const utteranceStartedRef = useRef(0)
  const onResultRef = useRef(onResult)
  onResultRef.current = onResult
  const localeRef = useRef(locale)
  localeRef.current = locale
  const transcriptionRef = useRef<TranscriptionQueue<{
    text: string
    timing: VoiceInputTiming
  }> | null>(null)
  if (!transcriptionRef.current) {
    transcriptionRef.current = new TranscriptionQueue({
      onResult: ({ text, timing }) => {
        if (!mountedRef.current || !isSubmittableTranscript(text)) return
        stageVoiceInputTrace(timing)
        onResultRef.current(text)
      },
      onError: (error) =>
        console.error('[useVoiceRecording] 语音识别出错:', error),
      onBusy: (busy) => {
        if (mountedRef.current) setIsProcessingVoice(busy)
      },
    })
  }

  useEffect(() => {
    let disposed = false
    getSpeechStatus()
      .then((s) => {
        if (disposed) return
        const convo = !!s.convo_enabled
        convoRtcRef.current = convo
        setSpeechAvailable(Boolean(s.available && (s.asr_enabled || convo)))
      })
      .catch(() => {})
    return () => {
      disposed = true
    }
  }, [])

  const transcribe = useCallback(
    (pcmData: Float32Array[], sampleRate: number, startedAt: number) => {
      if (pcmData.length === 0) return
      const inputEnded = performance.now()
      const engine = LOCALE_ENGINE_MAP[localeRef.current] || '16k_zh'
      transcriptionRef.current!.enqueue(async (signal) => {
        signal.throwIfAborted()
        const wavBlob = pcmToWav(pcmData, sampleRate)
        const base64Audio = await audioToBase64(wavBlob)
        signal.throwIfAborted()
        const asrStarted = performance.now()
        const result = await speechToText(
          {
            audio_data: base64Audio,
            format: 'wav',
            engine,
          },
          undefined,
          signal,
        )
        const text = result.success ? (result.text?.trim() ?? '') : ''
        return {
          text,
          timing: {
            input_started: startedAt,
            input_ended: inputEnded,
            asr_started: asrStarted,
            asr_completed: performance.now(),
          },
        }
      })
    },
    [],
  )

  const onPcm = useCallback(
    (frame: Float32Array) => {
      const recorder = recorderRef.current
      if (!recorder || !isRecordingRef.current) return
      if (!conversationRef.current) {
        recorder.pcmData.push(frame)
        return
      }

      const event = recorder.capture.push(frame, getVoicePresence().ttsPlaying)
      if (event.started) {
        utteranceStartedRef.current = performance.now()
        patchVoicePresence({ userSpeaking: true })
        getSpeechPipeline().cancel()
      }
      if (event.ended) patchVoicePresence({ userSpeaking: false })
      if (event.utterance) {
        transcribe(
          event.utterance.pcm,
          event.utterance.sampleRate,
          utteranceStartedRef.current,
        )
      }
    },
    [transcribe],
  )

  const startRecording = useCallback(async () => {
    if (isRecordingRef.current || recorderRef.current || openingRef.current)
      return
    openingRef.current = true
    const ticket = ++lifecycleRef.current
    transcriptionRef.current!.reset()
    try {
      void import('../../utils/analyticsEvents').then(
        ({ trackProductEvent, AnalyticsEvents }) => {
          trackProductEvent(AnalyticsEvents.AGENT_VOICE, { throttleMs: 5000 })
        },
      )
      const status = await getSpeechStatus()
      if (!mountedRef.current || ticket !== lifecycleRef.current) return
      if (!status.available || !status.asr_enabled) return

      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          sampleRate: 16000,
          channelCount: 1,
          echoCancellation: true,
          noiseSuppression: true,
        },
      })
      if (!mountedRef.current || ticket !== lifecycleRef.current) {
        stream.getTracks().forEach((track) => track.stop())
        return
      }

      const audioContext = new AudioContext({ sampleRate: 16000 })
      const pcmData: Float32Array[] = []

      let workletNode: AudioWorkletNode
      try {
        workletNode = await createPcmCaptureNode(audioContext)
        if (audioContext.state === 'suspended') await audioContext.resume()
      } catch (err) {
        stream.getTracks().forEach((track) => track.stop())
        await audioContext.close()
        throw err
      }
      if (!mountedRef.current || ticket !== lifecycleRef.current) {
        workletNode.disconnect()
        stream.getTracks().forEach((track) => track.stop())
        await audioContext.close()
        return
      }

      workletNode.port.onmessage = (event: MessageEvent<Float32Array>) => {
        onPcm(event.data)
      }

      const muteNode = audioContext.createGain()
      muteNode.gain.value = 0
      const source = audioContext.createMediaStreamSource(stream)
      source.connect(workletNode)
      workletNode.connect(muteNode)
      muteNode.connect(audioContext.destination)

      recorderRef.current = {
        audioContext,
        stream,
        workletNode,
        muteNode,
        pcmData,
        capture: new UtteranceCapture(audioContext.sampleRate),
        startedAt: performance.now(),
      }
      isRecordingRef.current = true
      setIsRecording(true)
      patchVoicePresence({ listening: conversationRef.current })
    } catch (err) {
      console.error('[useVoiceRecording] 无法访问麦克风:', err)
    } finally {
      if (ticket === lifecycleRef.current) {
        openingRef.current = false
        if (!isRecordingRef.current && conversationRef.current) {
          conversationRef.current = false
          if (mountedRef.current) setConversation(false)
        }
      }
    }
  }, [onPcm])

  const stopRecording = useCallback(async () => {
    lifecycleRef.current += 1
    openingRef.current = false
    const fromConversation = conversationRef.current
    isRecordingRef.current = false
    setIsRecording(false)
    conversationRef.current = false
    setConversation(false)
    transcriptionRef.current!.reset()
    patchVoicePresence({ listening: false, userSpeaking: false })
    if (convoRtcRef.current && fromConversation) {
      isRecordingRef.current = false
      setIsRecording(false)
      conversationRef.current = false
      setConversation(false)
      await stopAgoraConversation()
      return
    }
    const recorder = recorderRef.current
    if (!recorder) return

    const sampleRate = recorder.audioContext.sampleRate || 16000
    const pcmData = recorder.pcmData
    recorder.capture.reset()
    cleanupRecorder(recorder)
    recorderRef.current = null
    // conversation exit drops unfinished speech; PTT submits the clip
    if (!fromConversation) transcribe(pcmData, sampleRate, recorder.startedAt)
  }, [transcribe])

  const toggleRecording = useCallback(() => {
    if (isRecordingRef.current || openingRef.current) void stopRecording()
    else void startRecording()
  }, [startRecording, stopRecording])

  const enterConversation = useCallback(async () => {
    if (conversationRef.current) return
    conversationRef.current = true
    setConversation(true)
    if (convoRtcRef.current) {
      setAgentPanelMode('chat')
      const recorder = recorderRef.current
      if (recorder) {
        cleanupRecorder(recorder)
        recorderRef.current = null
        isRecordingRef.current = false
        setIsRecording(false)
      }
      transcriptionRef.current!.reset()
      const ticket = ++lifecycleRef.current
      openingRef.current = true
      try {
        const ok = await startAgoraConversation(localeRef.current)
        if (!mountedRef.current || ticket !== lifecycleRef.current) return
        if (!ok) throw new Error('convo start returned false')
        isRecordingRef.current = true
        setIsRecording(true)
        patchVoicePresence({ listening: true })
        return
      } catch (err) {
        if (ticket !== lifecycleRef.current || !mountedRef.current) return
        console.error('[useVoiceRecording] 实时对话启动失败:', err)
        conversationRef.current = false
        setConversation(false)
        return
      } finally {
        if (ticket === lifecycleRef.current) openingRef.current = false
      }
    }
    if (isRecordingRef.current) {
      const recorder = recorderRef.current
      if (recorder) recorder.pcmData.length = 0
      patchVoicePresence({ listening: true })
      return
    }
    await startRecording()
  }, [startRecording])

  const stopConversation = useCallback(() => {
    if (!conversationRef.current) return
    void stopRecording()
  }, [stopRecording])

  useEffect(() => subscribeAgoraStopped(() => {
    if (!convoRtcRef.current || !conversationRef.current) return
    lifecycleRef.current += 1
    openingRef.current = false
    conversationRef.current = false
    isRecordingRef.current = false
    if (mountedRef.current) { setConversation(false); setIsRecording(false) }
  }), [])

  useEffect(
    () =>
      subscribeAgentPanelMode(() => {
        if (
          convoRtcRef.current &&
          conversationRef.current &&
          getAgentPanelMode() !== 'chat'
        ) {
          void stopRecording()
}
      }),
    [stopRecording],
  )

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
      lifecycleRef.current += 1
      openingRef.current = false
      transcriptionRef.current!.reset()
      if (
        agoraConversationActive() ||
        (convoRtcRef.current && conversationRef.current)
      ) {
        void stopAgoraConversation()
      }
      const recorder = recorderRef.current
      if (recorder) {
        isRecordingRef.current = false
        conversationRef.current = false
        cleanupRecorder(recorder)
        recorderRef.current = null
      }
      dropPendingTurnTrace()
      patchVoicePresence({ listening: false, userSpeaking: false })
    }
  }, [])

  return {
    speechAvailable,
    isRecording,
    isProcessingVoice,
    conversation,
    startRecording,
    stopRecording,
    toggleRecording,
    enterConversation,
    stopConversation,
  }
}
