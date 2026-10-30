import { getRigMotionCoordinator } from '@myriad/figure/motion/coordinator'
import { noteTurnTraceLeaks } from '../events/turnTrace'
import { speechAudioContextOpen } from '../speech/ttsPlayer'
import { voicePresenceListenerCount } from '../speech/voicePresence'

export function sampleTurnTraceLeaks(): void {
  noteTurnTraceLeaks({
    audioContexts: speechAudioContextOpen() ? 1 : 0,
    voiceListeners: voicePresenceListenerCount(),
    leases: getRigMotionCoordinator().leaseCount(),
  })
}
