import type { TimeTranslations } from '../types'

import { useMemo } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { formatMessage, localeOrFallback } from '../../../i18n'

export function useBrewTimes(): TimeTranslations {
  const { t } = useI18n()
  return useMemo(
    () => ({
      justNow: t.brew.justNow,
      minutesAgo: t.brew.minutesAgo,
      hoursAgo: t.brew.hoursAgo,
      daysAgo: t.brew.daysAgo,
    }),
    [t.brew.justNow, t.brew.minutesAgo, t.brew.hoursAgo, t.brew.daysAgo],
  )
}

export function brewRelativeTime(
  timestamp: number | null | undefined,
  translations: TimeTranslations,
  locale: string,
): string {
  if (!timestamp) return ''
  const date = new Date(timestamp)
  const diff = Date.now() - date.getTime()

  if (diff < 60_000) return translations.justNow
  const loc = localeOrFallback(locale)
  if (diff < 3_600_000) {
    return formatMessage(loc, translations.minutesAgo, {
      minutes: Math.floor(diff / 60_000),
    })
  }
  if (diff < 86_400_000) {
    return formatMessage(loc, translations.hoursAgo, {
      hours: Math.floor(diff / 3_600_000),
    })
  }
  if (diff < 604_800_000) {
    return formatMessage(loc, translations.daysAgo, {
      days: Math.floor(diff / 86_400_000),
    })
  }

  const dateLocale = locale
  return date.toLocaleDateString(dateLocale, {
    month: 'short',
    day: 'numeric',
  })
}
