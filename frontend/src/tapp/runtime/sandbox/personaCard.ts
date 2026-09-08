import type { ActivityKey, MoodBand } from '../../../components/agent/meropeVitals'
import { activityKey, moodBand } from '../../../components/agent/meropeVitals'

/** Public TAPP projection of Agent 人设. Numbers and soul stay off this card. */
export interface TappPersonaCard {
  enabled: boolean
  name: string
  moodBand: MoodBand
  activity: ActivityKey
  portraitUrl: string | null
}

/**
 * Same-origin portrait path for `<img src>`. CSP already allows the host origin;
 * remote https would need `network:fetch`, so it is dropped rather than rewritten.
 */
export function sameOriginPortraitUrl(
  raw: string | null | undefined,
): string | null {
  if (typeof raw !== 'string') return null
  const value = raw.trim()
  if (
    !value.startsWith('/') ||
    value.startsWith('//') ||
    value.length > 512 ||
    value.includes(':') ||
    value.includes('..') ||
    /[\s\u0000-\u001F]/.test(value)
  ) {
    return null
  }
  return value
}

export function projectPersonaCard(input: {
  enabled: boolean
  name: string
  mood?: number
  arousal?: number
  activity?: string
  portraitUrl?: string | null
}): TappPersonaCard {
  return {
    enabled: input.enabled === true,
    name: input.name,
    moodBand: moodBand(input.mood, input.arousal),
    activity: activityKey(input.activity),
    portraitUrl: sameOriginPortraitUrl(input.portraitUrl),
  }
}
