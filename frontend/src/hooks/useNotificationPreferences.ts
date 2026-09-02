import type {
  NotificationEventDefinition,
  NotificationPreferences,
  NotificationSourceKey,
} from '../services/notificationPreferencesApi'
import { useCallback, useEffect, useRef, useState } from 'react'
import { currentCopy } from '../i18n/localeCopy'
import notificationPreferencesApi, {
  DEFAULT_NOTIFICATION_CATALOG,
  DEFAULT_NOTIFICATION_PREFERENCES,
  NOTIFICATION_PREFERENCES_UPDATED_EVENT,
} from '../services/notificationPreferencesApi'
import { showError } from '../utils/toastManager'
import { userFacingError } from '../utils/userFacingError'

export function useNotificationPreferences(userId?: number) {
  const [preferences, setPreferences] = useState<NotificationPreferences>(
    DEFAULT_NOTIFICATION_PREFERENCES,
  )
  const [sources, setSources] = useState<NotificationSourceKey[]>(
    DEFAULT_NOTIFICATION_CATALOG.sources,
  )
  const [events, setEvents] = useState<NotificationEventDefinition[]>(
    DEFAULT_NOTIFICATION_CATALOG.events,
  )
  const [loading, setLoading] = useState(Boolean(userId))
  const userIdRef = useRef(userId)
  userIdRef.current = userId

  const reload = useCallback(async () => {
    if (!userId) {
      setPreferences(DEFAULT_NOTIFICATION_PREFERENCES)
      setLoading(false)
      return
    }
    setLoading(true)
    const requestedUserId = userId
    try {
      const response = await notificationPreferencesApi.get()
      if (userIdRef.current !== requestedUserId) return
      setPreferences(response.preferences)
      setSources(response.catalog.sources)
      setEvents(response.catalog.events)
    } catch (error) {
      console.warn('[Notifications] Failed to load preferences:', error)
      if (userIdRef.current !== requestedUserId) return
      showError(
        userFacingError(
          error,
          currentCopy().errors.notificationPrefsLoadFailed,
        ),
      )
    } finally {
      if (userIdRef.current === requestedUserId) setLoading(false)
    }
  }, [userId])

  useEffect(() => {
    setPreferences(DEFAULT_NOTIFICATION_PREFERENCES)
    setSources(DEFAULT_NOTIFICATION_CATALOG.sources)
    setEvents(DEFAULT_NOTIFICATION_CATALOG.events)
    void reload()
  }, [reload])

  useEffect(() => {
    const handleUpdated = (event: Event) => {
      const detail = (
        event as CustomEvent<{
          preferences: NotificationPreferences
          userId?: number
        }>
      ).detail
      if (detail?.preferences && detail.userId === userIdRef.current) {
        setPreferences(detail.preferences)
      }
    }
    window.addEventListener(
      NOTIFICATION_PREFERENCES_UPDATED_EVENT,
      handleUpdated,
    )
    return () =>
      window.removeEventListener(
        NOTIFICATION_PREFERENCES_UPDATED_EVENT,
        handleUpdated,
      )
  }, [])

  return { preferences, setPreferences, sources, events, loading, reload }
}
