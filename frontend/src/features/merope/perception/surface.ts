import { createStore } from '../../../utils/store'

export type ForegroundSurface =
  | 'control_panel'
  | 'notification'
  | 'user_modal'
  | 'none'

const surface = createStore<ForegroundSurface>('none')

export const getForegroundSurface = surface.get
export const subscribeForegroundSurface = surface.subscribe

export function setForegroundSurface(next: ForegroundSurface): void {
  surface.set(next)
}
