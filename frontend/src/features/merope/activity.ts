import type { MeropeActivity } from './types'

/** Speech owns mouth; persisted `talking` must not keep the body talking. */
export function agentStatusActivity(status: string): MeropeActivity {
  return status === 'thinking' || status === 'working' ? 'thinking' : 'idle'
}
