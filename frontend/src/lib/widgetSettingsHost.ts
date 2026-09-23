import { createStore } from '../utils/store'

/** First widget-settings open arms the lazy modal hosts in AppLayout. */
const armed = createStore(false)

export function armWidgetSettingsHost(): void {
  armed.set(true)
}

export const isWidgetSettingsHostArmed = armed.get
export const subscribeWidgetSettingsHost = armed.subscribe
