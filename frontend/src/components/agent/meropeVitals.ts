export const ADDRESSEE_UPDATED_EVENT = 'arael-addressee-updated'

export type MoodBand = 'floor' | 'sad' | 'tense' | 'calm' | 'excited'
export type ActivityKey = 'idle' | 'working' | 'thinking' | 'talking'

const DEFAULT_MOOD = 70
const DEFAULT_AROUSAL = 48

export function moodBand(
  mood: number | undefined,
  arousal: number | undefined = DEFAULT_AROUSAL,
): MoodBand {
  const v = typeof mood === 'number' && Number.isFinite(mood) ? mood : DEFAULT_MOOD
  const a =
    typeof arousal === 'number' && Number.isFinite(arousal)
      ? arousal
      : DEFAULT_AROUSAL
  if (v <= 10) return 'floor'
  if (v < 55 && a < 55) return 'sad'
  if (v < 55) return 'tense'
  if (a < 55) return 'calm'
  return 'excited'
}

export function activityKey(raw: string | undefined): ActivityKey {
  if (raw === 'working' || raw === 'thinking' || raw === 'talking') return raw
  return 'idle'
}
