import type { ConfigDomainController, ConfigOperation } from './configDomain'
import type { ShowMessage } from './types'
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react'
import { authSubject } from '../../../utils/authSubject'
import { userFacingError } from '../../../utils/userFacingError'
import {
  executeConfigOperations,
  isPriorityConfigDomain,
  loadConfigDomains,
} from './configDomain'

function subscribeSubject(listener: () => void) {
  return authSubject.subscribe(listener)
}
const subjectRevision = () => authSubject.revision

/** Remount drafts for every identity invalidation, including forced reauthentication. */
export function useConfigSessionKey() {
  return useSyncExternalStore(
    subscribeSubject,
    subjectRevision,
    subjectRevision,
  )
}

interface EditorDomain extends ConfigDomainController {
  ready: boolean
  loading: boolean
  error: unknown
  dirty: boolean
  pendingSync: boolean
  sections?: readonly string[]
  load: (signal?: AbortSignal) => Promise<void>
}
interface EditorMessages {
  loadConfigFailed: string
  configSaved: string
  configSaveFailed: string
  savingConfig: string
  partialSaveWarning: string
  resettingConfig: string
  resetFailed: string
  configReset: string
  resetCurrentPageNone: string
  resetCurrentPageDone: string
}

export function useConfigEditor(
  domains: EditorDomain[],
  showMessage: ShowMessage,
  messages: EditorMessages,
  neededSection = '',
) {
  const latest = useRef({ domains, showMessage, messages, neededSection })
  latest.current = { domains, showMessage, messages, neededSection }
  const busy = useRef(false)
  const session = useRef<AbortSignal | null>(null)
  const [saving, setSaving] = useState(false)
  const isDirty = domains.some((domain) => domain.dirty || domain.pendingSync)

  const load = useCallback(async (section?: string) => {
    const signal = session.current
    if (!signal || signal.aborted) return
    const { domains, showMessage, messages, neededSection } = latest.current
    const selected = section
      ? domains.filter((domain) => domain.sections?.includes(section))
      : domains
    const outcomes = await loadConfigDomains(selected, signal, section ?? neededSection)
    if (signal.aborted) return
    const failed = outcomes.find(
      ({ domain, result }) =>
        result.status === 'rejected' &&
        isPriorityConfigDomain(domain, latest.current.neededSection),
    )
    if (failed?.result.status === 'rejected') {
      showMessage(
        userFacingError(failed.result.reason, messages.loadConfigFailed),
        'error',
        0,
      )
    }
  }, [])
  useEffect(() => {
    const lifetime = new AbortController()
    session.current = AbortSignal.any([lifetime.signal, authSubject.signal])
    void load()
    return () => lifetime.abort()
  }, [load])
  useEffect(() => {
    if (!isDirty && !saving) return
    const beforeUnload = (event: BeforeUnloadEvent) => event.preventDefault()
    window.addEventListener('beforeunload', beforeUnload)
    return () => window.removeEventListener('beforeunload', beforeUnload)
  }, [isDirty, saving])

  const run = useCallback(async (scope?: string) => {
    // Save, reset and external save events share the same synchronous lock.
    const signal = session.current
    if (busy.current || !signal || signal.aborted) return
    busy.current = true
    setSaving(true)
    const { domains, showMessage, messages } = latest.current
    const eventName = scope ? 'config-reset-result' : 'config-save-result'
    try {
      const operations: ConfigOperation[] = domains.flatMap((domain) => {
        const operation = scope
          ? domain.prepareReset(scope)
          : domain.prepareSave()
        return operation ? [operation] : []
      })
      if (
        operations.some((operation) =>
          domains.some((domain) => {
            const state = domain.getSnapshot()
            return domain.id === operation.id && (!state.ready || state.loading)
          }),
        )
      ) {
        throw new Error(messages.loadConfigFailed)
      }
      const result = await executeConfigOperations(domains, operations, signal)
      if (result.cancelled) return
      if (result.errors.length) {
        const partial = result.persisted.length > 0 || result.pendingSync
        const detail = userFacingError(
          result.errors[0],
          scope ? messages.resetFailed : messages.configSaveFailed,
        )
        const message = partial
          ? `${messages.partialSaveWarning}: ${detail}`
          : detail
        showMessage(message, partial ? 'warning' : 'error', 0)
        window.dispatchEvent(
          new CustomEvent(eventName, {
            detail: { success: false, partial, message },
          }),
        )
        return
      }
      const message = scope
        ? operations.length === 0
          ? messages.resetCurrentPageNone
          : scope === 'all'
            ? messages.configReset
            : messages.resetCurrentPageDone
        : messages.configSaved
      showMessage(message, 'success', 3000)
      window.dispatchEvent(
        new CustomEvent(eventName, { detail: { success: true, message } }),
      )
    } catch (error) {
      if (signal.aborted) return
      const message = userFacingError(
        error,
        scope ? messages.resetFailed : messages.configSaveFailed,
      )
      showMessage(message, 'error', 0)
      window.dispatchEvent(
        new CustomEvent(eventName, { detail: { success: false, message } }),
      )
    } finally {
      busy.current = false
      if (!signal.aborted) setSaving(false)
    }
  }, [])
  const save = useCallback(() => run(), [run])
  const reset = useCallback((scope = 'all') => run(scope), [run])
  return { isDirty, saving, load, save, reset }
}
