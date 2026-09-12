import { getDefaultLocale } from './index'

/** Host UI locale + browser TZ for Myriad API requests. */
export function hostLocaleHeaders(): Record<string, string> {
  const locale = getDefaultLocale()
  let timezone = 'UTC'
  try {
    timezone = new Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'
  } catch {
    // keep UTC
  }
  return {
    'X-Myriad-Locale': locale,
    'X-Myriad-Timezone': timezone,
    'Accept-Language': locale,
  }
}
