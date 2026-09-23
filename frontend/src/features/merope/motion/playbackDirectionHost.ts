import type { PlaybackDirectionScope } from './playbackDirection'
import { getAgentPanelMode } from '../../../components/agent-panel/agentPanelMode'
import { API_URL } from '../../../config'
import apiService from '../../../services/api'
import { clearCSRFToken, getCSRFToken } from '../../../utils/csrf'
import { deliverTurnLine } from '../engineFace'
import { liveFaceVisible } from '../faceVisible'
import { sanitizePerformanceDirective } from '../performanceEvents'
import { getSpeechPipeline } from '../speech/speechPipelineHost'
import { MEROPE_SPEECH_EVENT, meropeSpeechEventDetail } from '../speechEvents'
import { markTurnTrace } from '../turnTrace'
import { isLiveMotionGeneration } from './liveGeneration'
import {
  PlaybackDirectionClient,
  sendPlaybackObservation,
} from './playbackDirection'
import {
  captureProductionRigStateSummary,
  getProductionMotionRuntime,
} from './runtimeHost'

let enabled = false
let timer: ReturnType<typeof setInterval> | null = null

function stopChecking(): void {
  if (timer != null) clearInterval(timer)
  timer = null
}

export const playbackDirection = new PlaybackDirectionClient({
  observe: (scope, signal) =>
    sendPlaybackObservation(
      `${API_URL}/api/agent/runs/${encodeURIComponent(scope.runId)}/performance`,
      signal,
      {
        token: getCSRFToken,
        clearToken: clearCSRFToken,
        fetch,
        capture: () => {
          const upcoming = [
            getProductionMotionRuntime().speech.upcomingText({
              ...scope,
              source: 'reply',
            }),
            getSpeechPipeline().pipeline.upcomingText(
              scope.messageId,
              scope.generation,
            ),
          ]
            .filter(Boolean)
            .join('\n')
          return {
            upcomingText: Iterator.from(upcoming).take(900).toArray().join(''),
            rig: captureProductionRigStateSummary(),
          }
        },
      },
    ),
  read: (runId, after, signal) =>
    apiService.get(`/agent/runs/${encodeURIComponent(runId)}/performance`, {
      params: { after },
      signal,
    }),
  close: (runId) => {
    void apiService
      .delete(`/agent/runs/${encodeURIComponent(runId)}/performance`)
      .catch(() => {})
  },
  current: ({ generation }) =>
    enabled &&
    isLiveMotionGeneration(generation) &&
    liveFaceVisible() &&
    document.visibilityState === 'visible' &&
    getAgentPanelMode() === 'chat',
  playing: ({ messageId, generation }) =>
    getSpeechPipeline().isBusyWith(messageId) ||
    getProductionMotionRuntime().speech.hasPlayback({
      messageId,
      generation,
      source: 'reply',
    }),
  sanitize: sanitizePerformanceDirective,
  deliver: (scope, performance) => {
    deliverTurnLine('chat', { ...scope, performance })
  },
  note: (reason, scope) =>
    markTurnTrace('director_delivery', {
      reason,
      runId: scope.runId,
      messageId: scope.messageId,
      generation: scope.generation,
    }),
})

export function retainPlaybackDirection(): () => void {
  enabled = true
  const onSpeech = (event: Event) => {
    const detail = meropeSpeechEventDetail((event as CustomEvent).detail)
    if (detail?.phase === 'cancel' && !detail.utteranceId)
      playbackDirection.cancel(detail.messageId)
  }
  window.addEventListener(MEROPE_SPEECH_EVENT, onSpeech)
  return () => {
    enabled = false
    stopChecking()
    window.removeEventListener(MEROPE_SPEECH_EVENT, onSpeech)
    playbackDirection.stop()
  }
}

export function startPlaybackDirection(scope: PlaybackDirectionScope): void {
  playbackDirection.start(scope)
  if (!playbackDirection.isActive || timer != null) return
  timer = setInterval(() => {
    playbackDirection.check()
    if (!playbackDirection.isActive) stopChecking()
  }, 100)
}
