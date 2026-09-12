import { useEffect, useRef } from 'react'
import { isLocale } from '../i18n'
import {
  putAccountLocale,
  setLocalePersistHandler,
} from '../i18n/localeAccount'
import { useAuth } from './AuthContext'
import { useI18n } from './I18nContext'

/** Apply account language after login; persist control-panel switches. */
export function LocaleAccountSync() {
  const { locale, setLocale } = useI18n()
  const { isAuthenticated, user, hasChecked } = useAuth()
  const appliedUserId = useRef<number | null>(null)

  useEffect(() => {
    if (!isAuthenticated) {
      appliedUserId.current = null
      setLocalePersistHandler(null)
      return
    }
    setLocalePersistHandler((next) => {
      void putAccountLocale(next).catch((err) => {
        console.error('[I18n] Failed to persist locale:', err)
      })
    })
    return () => setLocalePersistHandler(null)
  }, [isAuthenticated])

  useEffect(() => {
    if (!hasChecked || !isAuthenticated || !user) return
    if (appliedUserId.current === user.id) return
    appliedUserId.current = user.id
    if (isLocale(user.locale) && user.locale !== locale) {
      setLocale(user.locale, { persist: false })
    }
  }, [hasChecked, isAuthenticated, user, locale, setLocale])

  return null
}
