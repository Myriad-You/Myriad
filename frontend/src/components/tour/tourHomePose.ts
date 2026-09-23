import { createStore } from '../../utils/store'

const homeEditSurface = createStore(false)

export function setHomeEditSurface(active: boolean): void {
  homeEditSurface.set(active)
}

export const isHomeEditSurface = homeEditSurface.get
export const subscribeHomeEditSurface = homeEditSurface.subscribe

export type HomeEditTourDockPose = 'parked' | 'restored'
export type HomeBrowseTourPanelPose = 'collapsed' | 'expanded'

export function homeEditTourDockPose(
  tourId: string | null,
  stepId: string | null,
): HomeEditTourDockPose | undefined {
  if (tourId !== 'home-edit-owner') return undefined
  if (stepId === 'home-widget-library') return 'restored'
  return 'parked'
}

export function homeBrowseTourPanelPose(
  tourId: string | null,
  stepId: string | null,
): HomeBrowseTourPanelPose | undefined {
  if (tourId !== 'home-visitor' && tourId !== 'home-owner') return undefined
  if (stepId === 'control-panel' || stepId === 'control-panel-owner') {
    return 'expanded'
  }
  return stepId ? 'collapsed' : undefined
}
