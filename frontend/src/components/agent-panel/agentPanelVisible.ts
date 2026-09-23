import { createStore } from '../../utils/store'

const visible = createStore(false)

export const getAgentPanelVisible = visible.get
export const subscribeAgentPanelVisible = visible.subscribe

/** Hidden tab with the panel mounted is not looking. */
export function isLookingAtAgentPanel(): boolean {
  return visible.get() && (typeof document === 'undefined' || !document.hidden)
}

export function setAgentPanelVisible(next: boolean): void {
  visible.set(next)
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
