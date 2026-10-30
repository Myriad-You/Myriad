export const ADDRESSEE_UPDATED_EVENT = 'arael-addressee-updated'

export type { MoodBand } from '../../features/merope/performance/mood'
export { moodBand } from '../../features/merope/performance/mood'
export type ActivityKey = 'idle' | 'working' | 'thinking' | 'talking'

export function activityKey(raw: string | undefined): ActivityKey {
  if (raw === 'working' || raw === 'thinking' || raw === 'talking') return raw
  return 'idle'
}
