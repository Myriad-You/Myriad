import type { ConfigDomainController, ConfigOperation } from './configDomain'
import type { ShowMessage } from './types'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useUnsavedChangesGuard } from '../../../hooks/useUnsavedChangesGuard'
import { userFacingError } from '../../../utils/userFacingError'
import { executeConfigOperations } from './configDomain'

interface EditorDomain extends ConfigDomainController {
  ready: boolean
  loading: boolean
  error: unknown
  dirty: boolean
  pendingSync: boolean
  load: () => Promise<void>
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
  unsavedChangesPrompt: string
}

export function useConfigEditor(
  domains: EditorDomain[],
  showMessage: ShowMessage,
  messages: EditorMessages,
) {
  const latest = useRef({ domains, showMessage, messages })
  latest.current = { domains, showMessage, messages }
  const busy = useRef(false)
  const [saving, setSaving] = useState(false)
  const isDirty = domains.some((domain) => domain.dirty || domain.pendingSync)
  useUnsavedChangesGuard(isDirty, messages.unsavedChangesPrompt)

  const load = useCallback(async () => {
    const { domains, showMessage, messages } = latest.current
    const results = await Promise.allSettled(
      domains.map((domain) => domain.load()),
    )
    const failed = results.find((result) => result.status === 'rejected')
    if (failed?.status === 'rejected') {
      showMessage(
        userFacingError(failed.reason, messages.loadConfigFailed),
        'error',
        0,
      )
    }
  }, [])
  useEffect(() => {
    void load()
  }, [load])

  useEffect(() => {
    window.dispatchEvent(
      new CustomEvent('config-dirty-state', { detail: { dirty: isDirty } }),
    )
  }, [isDirty])
  useEffect(
    () => () => {
      window.dispatchEvent(
        new CustomEvent('config-dirty-state', { detail: { dirty: false } }),
      )
    },
    [],
  )

  const run = useCallback(async (scope?: string) => {
    // Save, reset and external save events share the same synchronous lock.
    if (busy.current) return
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
          domains.some(
            (domain) =>
              domain.id === operation.id && (!domain.ready || domain.loading),
          ),
        )
      ) {
        throw new Error(messages.loadConfigFailed)
      }
      showMessage(
        scope ? messages.resettingConfig : messages.savingConfig,
        'info',
        0,
      )
      const result = await executeConfigOperations(domains, operations)
      if (result.errors.length) {
        const partial =
          result.persisted.length > 0 ||
          domains.some((domain) => domain.pendingSync)
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
      setSaving(false)
    }
  }, [])
  const save = useCallback(() => run(), [run])
  const reset = useCallback((scope = 'all') => run(scope), [run])
  return { isDirty, saving, load, save, reset }
}
