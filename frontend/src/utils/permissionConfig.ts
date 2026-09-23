import { useEffect, useSyncExternalStore } from 'react'

interface PermissionResponse {
  success?: boolean
  config?: { user?: { ai_chat?: boolean }; guest?: { ai_chat?: boolean } }
}
interface Snapshot {
  loaded: boolean
  elevatedAiChat?: { user: boolean; guest: boolean }
}

export function createPermissionConfigStore(fetchConfig: () => Promise<PermissionResponse>) {
  let snapshot: Snapshot = { loaded: false }
  let generation = 0
  let pending: Promise<void> | undefined
  let failed = false
  const listeners = new Set<() => void>()
  const publish = (next: Snapshot) => { snapshot = next; listeners.forEach(listener => listener()) }
  const load = (): Promise<void> => {
    if (pending) return pending
    if (snapshot.loaded && !failed) return Promise.resolve()
    const owner = generation
    const request = Promise.resolve().then(fetchConfig).then(response => {
      if (!response.success || !response.config) throw new Error('Permission configuration unavailable')
      if (owner !== generation) return
      failed = false
      publish({ loaded: true, elevatedAiChat: { user: !!response.config.user?.ai_chat, guest: !!response.config.guest?.ai_chat } })
    }).catch(() => {
      if (owner !== generation) return
      failed = true
      publish({ loaded: true })
    }).finally(() => { if (pending === request) pending = undefined })
    pending = request
    return request
  }
  return {
    getSnapshot: () => snapshot,
    subscribe: (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener) } },
    load,
    invalidate: () => {
      generation++
      pending = undefined
      failed = false
      publish({ loaded: false })
      if (listeners.size) void load()
    },
  }
}

const store = createPermissionConfigStore(async () => {
  const { fetchPermissionsConfig } = await import('../services/configApi')
  return fetchPermissionsConfig()
})
export const invalidatePermissionConfig = store.invalidate

export function usePermissionConfig() {
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot)
  useEffect(() => {
    const retry = () => { void store.load() }
    retry()
    window.addEventListener('online', retry)
    window.addEventListener('focus', retry)
    return () => {
      window.removeEventListener('online', retry)
      window.removeEventListener('focus', retry)
    }
  }, [])
  return snapshot
}
