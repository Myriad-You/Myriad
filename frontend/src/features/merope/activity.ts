import type { MeropeActivity } from './types'

/**
 * The rig follows the Agent's live work state. Speech owns its own mouth and
 * co-speech channels, so a persisted `talking` value must not keep the body
 * talking after the utterance has ended.
 */
export function agentStatusActivity(status: string): MeropeActivity {
  return status === 'thinking' || status === 'working' ? 'thinking' : 'idle'
}
