let visible = false
const listeners = new Set<() => void>()

export function getAgentPanelVisible(): boolean {
  return visible
}

/** Hidden tab with the panel mounted is not looking. */
export function isLookingAtAgentPanel(): boolean {
  return visible && (typeof document === 'undefined' || !document.hidden)
}

export function setAgentPanelVisible(next: boolean): void {
  if (visible === next) return
  visible = next
  for (const listener of listeners) listener()
}

export function subscribeAgentPanelVisible(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function subscribeLookingAtAgentPanel(listener: () => void): () => void {
  const stop = subscribeAgentPanelVisible(listener)
  if (typeof document === 'undefined') return stop
  document.addEventListener('visibilitychange', listener)
  return () => {
    stop()
    document.removeEventListener('visibilitychange', listener)
  }
}
