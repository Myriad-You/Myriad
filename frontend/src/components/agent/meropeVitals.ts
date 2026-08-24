/** Per-addressee vitals already returned by GET /api/agent/persona. */

export const ADDRESSEE_UPDATED_EVENT = 'arael-addressee-updated'

export type MoodBand = 'floor' | 'low' | 'normal' | 'high'
export type ActivityKey = 'idle' | 'working' | 'thinking' | 'talking'

export interface MeropeVitalsCopy {
  mood: Record<MoodBand, string>
  moodLine: string
  activity: Record<ActivityKey, string>
}

/** Same bands as `mood_tone_instruction` — UI shows the band, not the number. */
export function moodBand(mood: number | undefined): MoodBand {
  const n = typeof mood === 'number' && Number.isFinite(mood) ? mood : 70
  if (n <= 10) return 'floor'
  if (n < 40) return 'low'
  if (n >= 85) return 'high'
  return 'normal'
}

export function activityKey(raw: string | undefined): ActivityKey {
  if (raw === 'working' || raw === 'thinking' || raw === 'talking') return raw
  return 'idle'
}

export function formatVitalsLine(
  copy: MeropeVitalsCopy,
  mood: number | undefined,
  activity: string | undefined,
): string {
  const band = moodBand(mood)
  return `${copy.moodLine.replace('{band}', copy.mood[band])} · ${copy.activity[activityKey(activity)]}`
}
