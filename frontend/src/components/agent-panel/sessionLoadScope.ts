import type { AgentPanelMode } from './agentPanelMode'
import { authSubject } from '../../utils/authSubject'

/** Loading another session replaces only that lane's restore/reconnect work. */
export class SessionLoadScope {
  private readonly lanes = new Map<AgentPanelMode, AbortController>()

  begin(mode: AgentPanelMode, subject = authSubject.signal): AbortSignal {
    this.reset(mode)
    const controller = new AbortController()
    this.lanes.set(mode, controller)
    return AbortSignal.any([subject, controller.signal])
  }

  reset(mode?: AgentPanelMode): void {
    for (const lane of mode ? [mode] : ['work', 'chat'] as const) {
      this.lanes.get(lane)?.abort()
      this.lanes.delete(lane)
    }
  }
}
