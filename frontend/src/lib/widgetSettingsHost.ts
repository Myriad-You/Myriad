/** First widget-settings open arms the lazy modal hosts in AppLayout. */

let armed = false
const listeners = new Set<() => void>()

export function armWidgetSettingsHost(): void {
  if (armed) return
  armed = true
  listeners.forEach((listener) => listener())
}

export function isWidgetSettingsHostArmed(): boolean {
  return armed
}

export function subscribeWidgetSettingsHost(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}
