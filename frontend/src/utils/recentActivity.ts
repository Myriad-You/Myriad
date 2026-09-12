export const RECENT_ACTIVITY_UPDATED_EVENT = 'recent-activity-updated'

export function notifyRecentActivityUpdated(): void {
  window.dispatchEvent(new Event(RECENT_ACTIVITY_UPDATED_EVENT))
}
