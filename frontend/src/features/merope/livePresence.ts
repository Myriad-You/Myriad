import { getAgentPanelMode } from '../../components/agent-panel/agentPanelMode'
import { liveFaceVisible } from './faceVisible'
import { captureProductionRigStateSummary } from './motion/runtimeHost'
import { getVoicePresence } from './speech/voicePresence'

export function livePresenceFacts(): {
  speaking: boolean
  visibleMode: string
  faceVisible: boolean
  speechInterruptible: boolean
  motionIntent: string | null
  speechIntent: string
} {
  const rig = captureProductionRigStateSummary()
  const voice = getVoicePresence()
  const speaking = rig.speaking || voice.ttsPlaying
  return {
    speaking,
    visibleMode: getAgentPanelMode(),
    faceVisible: liveFaceVisible() && rig.faceVisible,
    speechInterruptible: voice.ttsPlaying || rig.speaking,
    motionIntent: rig.acting.intent,
    speechIntent: speaking ? (voice.ttsPlaying ? 'tts' : 'speech') : 'idle',
  }
}
