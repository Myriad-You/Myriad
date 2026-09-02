import { getRigMotionCoordinator } from './motion/coordinator'
import { speechAudioContextOpen } from './speech/ttsPlayer'
import { voicePresenceListenerCount } from './speech/voicePresence'
import { noteTurnTraceLeaks } from './turnTrace'

/** Snapshot leak-prone live resources into the local trace ring. */
export function sampleTurnTraceLeaks(): void {
  noteTurnTraceLeaks({
    audioContexts: speechAudioContextOpen() ? 1 : 0,
    voiceListeners: voicePresenceListenerCount(),
    leases: getRigMotionCoordinator().leaseCount(),
  })
}
