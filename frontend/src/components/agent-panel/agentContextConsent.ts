const STORAGE_KEY = 'myriad.agentPanel.contextConsent'

let consent = true
let loaded = false
const listeners = new Set<() => void>()

function read(): boolean {
  try {
    return window.localStorage.getItem(STORAGE_KEY) !== 'off'
  } catch {
    return true
  }
}

function ensureLoaded(): void {
  if (loaded) return
  loaded = true
  if (typeof window !== 'undefined') consent = read()
}

export function getAgentContextConsent(): boolean {
  ensureLoaded()
  return consent
}

export function setAgentContextConsent(next: boolean): void {
  ensureLoaded()
  if (consent === next) return
  consent = next
  try {
    window.localStorage.setItem(STORAGE_KEY, next ? 'on' : 'off')
  } catch {
    /* session-only */
  }
  for (const listener of listeners) listener()
}

export function subscribeAgentContextConsent(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function getServerAgentContextConsent(): boolean {
  return true
}
