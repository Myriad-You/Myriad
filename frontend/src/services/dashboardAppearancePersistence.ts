import type { DashboardConfigPatch } from './dashboardConfigApi'
import { currentCopy } from '../i18n/localeCopy'
import { authSubject } from '../utils/authSubject'
import { DebouncedLatestWriter } from '../utils/debouncedLatestWriter'
import { formatUserFacingError } from '../utils/formatUserFacingError'
import { showError } from '../utils/toastManager'
import { saveDashboardConfig } from './dashboardConfigApi'

export type DashboardAppearancePatch = Pick<
  DashboardConfigPatch,
  'title_font' | 'title_font_size' | 'title_color' | 'widget_theme'
>

type SaveErrorKey = 'titleStyleSaveFailed' | 'widgetThemeSaveFailed'

function createWriter(signal: AbortSignal) {
  let pending: DashboardAppearancePatch = {}
  let activeErrorKey: SaveErrorKey = 'titleStyleSaveFailed'
  const queue = new DebouncedLatestWriter<{ settings: DashboardAppearancePatch, errorKey: SaveErrorKey }>({
    signal,
    delay: 500,
    write: async ({ settings, errorKey }, owner) => {
      pending = {}
      activeErrorKey = errorKey
      owner.throwIfAborted()
      await saveDashboardConfig(settings, { signal: owner })
      owner.throwIfAborted()
    },
    onError: async (error, owner) => {
      const message = await formatUserFacingError(error, currentCopy().errors[activeErrorKey])
      if (!owner.aborted) showError(message)
    },
  })
  return {
    signal,
    enqueue(settings: DashboardAppearancePatch, errorKey: SaveErrorKey) {
      pending = { ...pending, ...settings }
      queue.enqueue({ settings: pending, errorKey })
    },
  }
}

let writer: ReturnType<typeof createWriter> | null = null

/** Account-owned writes survive panel closure; a subject change cancels pending work. */
export function saveDashboardAppearance(settings: DashboardAppearancePatch, errorKey: SaveErrorKey, owner = authSubject.signal) {
  if (owner.aborted || owner !== authSubject.signal) return
  if (!writer || writer.signal !== owner) writer = createWriter(owner)
  writer.enqueue(settings, errorKey)
}
