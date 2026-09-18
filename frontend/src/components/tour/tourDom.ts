export const TOUR_ACTIVE_EVENT = 'myriad-tour-active'
export const TOUR_ACTIVE_ATTR = 'tourActive'

export function isTourDomActive(): boolean {
  if (typeof document === 'undefined') return false
  return document.documentElement.dataset[TOUR_ACTIVE_ATTR] === '1'
}

export function setTourDomActive(active: boolean): void {
  if (typeof document === 'undefined') return
  const root = document.documentElement
  if (active) root.dataset[TOUR_ACTIVE_ATTR] = '1'
  else delete root.dataset[TOUR_ACTIVE_ATTR]
  window.dispatchEvent(new Event(TOUR_ACTIVE_EVENT))
}
