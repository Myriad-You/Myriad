import type { Store } from '../../utils/store'
import { createStore } from '../../utils/store'

const STORAGE_KEY = 'myriad.agentPanel.contextConsent'

function read(): boolean {
  try {
    return window.localStorage.getItem(STORAGE_KEY) !== 'off'
  } catch {
    return true
  }
}

let consent: Store<boolean> | null = null

/** Created on first use from storage, so the initial read publishes nothing. */
function store(): Store<boolean> {
  consent ??= createStore(typeof window !== 'undefined' ? read() : true)
  return consent
}

export function getAgentContextConsent(): boolean {
  return store().get()
}

export function setAgentContextConsent(next: boolean): void {
  if (store().get() === next) return
  store().set(next)
  try {
    window.localStorage.setItem(STORAGE_KEY, next ? 'on' : 'off')
  } catch {
    /* session-only */
  }
}

export function subscribeAgentContextConsent(listener: () => void): () => void {
  return store().subscribe(listener)
}

export function getServerAgentContextConsent(): boolean {
  return true
}
