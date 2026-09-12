import type { ActivityKey, MoodBand } from '../../../components/agent/meropeVitals'
import { activityKey, moodBand } from '../../../components/agent/meropeVitals'

export interface TappPersonaCard {
  enabled: boolean
  name: string
  moodBand: MoodBand
  activity: ActivityKey
  portraitUrl: string | null
}

/** 头像用同源路径。远端 https 需要 network:fetch，故丢弃不改写。 */
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
