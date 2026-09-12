import type { RigStateSummary } from '../../services/agent/types'
import { getAgentPanelMode } from '../../components/agent-panel/agentPanelMode'
import { getAgentPanelVisible } from '../../components/agent-panel/agentPanelVisible'
import { liveFaceVisible } from './faceVisible'
import { captureProductionRigStateSummary } from './motion/runtimeHost'
import { getVoicePresence } from './speech/voicePresence'

// Memory-only: duplicated tabs must not inherit a shared sessionStorage ID.
const instanceId = globalThis.crypto?.randomUUID?.()
  ?? `page-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`

export function livePresenceFacts(): {
  instanceId: string
  speaking: boolean
  visibleMode: string
  faceVisible: boolean
  pageVisible: boolean
  panelVisible: boolean
  speechInterruptible: boolean
  motionIntent: string | null
  speechIntent: string
  rigState: RigStateSummary
} {
  const rig = captureProductionRigStateSummary()
  const voice = getVoicePresence()
  const speaking = rig.speaking || voice.ttsPlaying
  return {
    instanceId,
    speaking,
    visibleMode: getAgentPanelMode(),
    faceVisible: liveFaceVisible() && rig.faceVisible,
    pageVisible: typeof document === 'undefined' || !document.hidden,
    panelVisible: getAgentPanelVisible(),
    speechInterruptible: voice.ttsPlaying || rig.speaking,
    motionIntent: rig.acting.intent,
    speechIntent: speaking ? (voice.ttsPlaying ? 'tts' : 'speech') : 'idle',
    rigState: { ...rig, speaking, faceVisible: liveFaceVisible() && rig.faceVisible },
  }
}
