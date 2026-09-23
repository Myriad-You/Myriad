import type { WidgetConfig } from '../widgetGridTypes'
import { API_URL } from '../../config'
import { currentCopy } from '../../i18n/localeCopy'
import { apiService } from '../../services/api'
import { authSubject } from '../../utils/authSubject'
import { DebouncedLatestWriter } from '../../utils/debouncedLatestWriter'
import { formatUserFacingError } from '../../utils/formatUserFacingError'
import { clearDedupCache } from '../../utils/requestDedup'
import { showError } from '../../utils/toastManager'

/** Both fields are already serialized, so a queued snapshot cannot be mutated later. */
interface ControlPanelPatch {
  control_panel_layout: string
  control_panel_rows: number
}

let writer: { signal: AbortSignal, queue: DebouncedLatestWriter<ControlPanelPatch> } | null = null

/** Saving belongs to the account, and survives attention moving away from the panel. */
export function saveControlPanelLayout(layout: WidgetConfig[], rows: number): void {
  const signal = authSubject.signal
  if (!writer || writer.signal !== signal) {
    writer = { signal, queue: new DebouncedLatestWriter({
      signal,
      delay: 500,
      write: async (patch: ControlPanelPatch, owner: AbortSignal) => {
        owner.throwIfAborted()
        await apiService.post('/config/control-panel', patch, { signal: owner })
        owner.throwIfAborted()
        clearDedupCache(`${API_URL}/api/config/ui`)
      },
      onError: async (error, owner) => {
        const message = await formatUserFacingError(error, currentCopy().errors.controlPanelSaveFailed)
        if (!owner.aborted) showError(message)
      },
    }) }
  }
  // Freeze the snapshot now; later edits must not mutate the queued request.
  writer.queue.enqueue({ control_panel_layout: JSON.stringify(layout), control_panel_rows: rows })
}
