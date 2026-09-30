import { noteTurnTraceLeaks } from '../events/turnTrace'
import { getRigMotionCoordinator } from '../motion/coordinator'
import { speechAudioContextOpen } from '../speech/ttsPlayer'
import { voicePresenceListenerCount } from '../speech/voicePresence'

export function sampleTurnTraceLeaks(): void {
  noteTurnTraceLeaks({
    audioContexts: speechAudioContextOpen() ? 1 : 0,
    voiceListeners: voicePresenceListenerCount(),
    leases: getRigMotionCoordinator().leaseCount(),
  })
}
