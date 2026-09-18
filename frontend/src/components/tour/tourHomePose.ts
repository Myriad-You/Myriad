let homeEditSurface = false
const homeEditListeners = new Set<() => void>()

export function setHomeEditSurface(active: boolean): void {
  if (homeEditSurface === active) return
  homeEditSurface = active
  homeEditListeners.forEach((listener) => listener())
}

export function isHomeEditSurface(): boolean {
  return homeEditSurface
}

export function subscribeHomeEditSurface(onStoreChange: () => void): () => void {
  homeEditListeners.add(onStoreChange)
  return () => {
    homeEditListeners.delete(onStoreChange)
  }
}

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
