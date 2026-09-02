export type ForegroundSurface =
  | 'control_panel'
  | 'notification'
  | 'user_modal'
  | 'none'

const listeners = new Set<() => void>()
let surface: ForegroundSurface = 'none'

export function getForegroundSurface(): ForegroundSurface {
  return surface
}

export function setForegroundSurface(next: ForegroundSurface): void {
  if (surface === next) return
  surface = next
  for (const listener of listeners) listener()
}

export function subscribeForegroundSurface(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}
