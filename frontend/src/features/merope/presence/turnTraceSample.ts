import { noteTurnTraceLeaks } from '../events/turnTrace'
import { getProductionCoordinator } from '../motion/productionCoordinator'
import { speechAudioContextOpen } from '../speech/ttsPlayer'
import { voicePresenceListenerCount } from '../speech/voicePresence'

export function sampleTurnTraceLeaks(): void {
  noteTurnTraceLeaks({
    audioContexts: speechAudioContextOpen() ? 1 : 0,
    voiceListeners: voicePresenceListenerCount(),
    leases: getProductionCoordinator().leaseCount(),
  })
}
