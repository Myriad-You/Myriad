/**
 * 面板现在人能不能看见。Toast 用它决定要不要再弹一层。
 */

let visible = false
const listeners = new Set<() => void>()

export function getAgentPanelVisible(): boolean {
  return visible
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
