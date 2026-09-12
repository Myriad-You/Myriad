import { getDefaultLocale } from '../i18n'
import { currentCopy } from '../i18n/localeCopy'

export * from './quote'
export * from './weather'

export type GreetingIconName =
  'sunrise' | 'sun' | 'cloud-sun' | 'sunset' | 'moon'

export interface GreetingData {
  text: string
  icon: GreetingIconName
  time: string
}

export interface GreetingTranslations {
  morning: string
  forenoon: string
  noon: string
  afternoon: string
  dusk: string
  evening: string
  night: string
}

export function getGreeting(
  username?: string,
  translations?: GreetingTranslations,
  locale?: string,
): GreetingData {
  const hour = new Date().getHours()
  const time = new Date().toLocaleTimeString(locale || getDefaultLocale(), {
    hour: '2-digit',
    minute: '2-digit',
  })

  let text = ''
  let icon: GreetingIconName = 'sun'

  const g = currentCopy().greeting
  const t = translations ?? {
    morning: g.morning,
    forenoon: g.forenoon,
    noon: g.noon,
    afternoon: g.afternoon,
    dusk: g.dusk,
    evening: g.evening,
    night: g.night,
  }

  if (hour >= 5 && hour < 8) {
    icon = 'sunrise'
    text = t.morning
  } else if (hour >= 8 && hour < 11) {
    icon = 'sun'
    text = t.forenoon
  } else if (hour >= 11 && hour < 13) {
    icon = 'sun'
    text = t.noon
  } else if (hour >= 13 && hour < 17) {
    icon = 'cloud-sun'
    text = t.afternoon
  } else if (hour >= 17 && hour < 19) {
    icon = 'sunset'
    text = t.dusk
  } else if (hour >= 19 && hour < 22) {
    icon = 'moon'
    text = t.evening
  } else {
    icon = 'moon'
    text = t.night
  }

  if (username) {
    text += locale?.startsWith('zh') ? `，${username}` : `, ${username}`
  }

  return { text, icon, time }
}
