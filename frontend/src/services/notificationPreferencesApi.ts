import { currentCopy } from '../i18n/localeCopy'
import { formatUserFacingError } from '../utils/formatUserFacingError'
import apiService from './api'

export const NOTIFICATION_SOURCE_KEYS = [
  'agent',
  'heartbeat',
  'mcp',
  'phantasi',
  'tapp',
  'updater',
  'federation',
  'system',
] as const

export type NotificationSourceKey = (typeof NOTIFICATION_SOURCE_KEYS)[number]

export const NOTIFICATION_EVENT_KEYS = [
  'agent.task_progress',
  'agent.task_completed',
  'agent.task_failed',
  'agent.task_cancelled',
  'agent.clarification',
  'agent.merope.platform_activity',
  'heartbeat.succeeded',
  'heartbeat.failed',
  'heartbeat.seo_review',
  'mcp.connected',
  'mcp.disconnected',
  'phantasi.new_items',
  'phantasi.source_error',
  'platform.sync.failed',
  'tapp.message',
  'tapp.warning',
  'tapp.error',
  'updater.submitted',
  'updater.running',
  'updater.succeeded',
  'updater.failed',
  'updater.needs_manual',
  'updater.unknown',
  'federation.channel_message',
  'federation.room_message',
  'federation.new_follower',
  'federation.follow_accepted',
  'federation.channel_invite',
  'federation.room_invite',
  'federation.channel_accepted',
  'federation.room_invite_accepted',
  'system.info',
  'skill.pruned',
  'skill.improved',
  'skill.changed',
] as const

export type NotificationEventKey = (typeof NOTIFICATION_EVENT_KEYS)[number]

export interface NotificationDeliveryPreferences {
  island: boolean
  toast: boolean
  browser: boolean
}

export interface NotificationLocationPreferences {
  panel: boolean
  toast: boolean
  island: boolean
  browser: boolean
}

export interface NotificationPreferences {
  enabled: boolean
  sources: Record<NotificationSourceKey, boolean>
  events: Record<NotificationEventKey, boolean>
  delivery: NotificationDeliveryPreferences
  locations: Record<NotificationSourceKey, NotificationLocationPreferences>
}

export interface NotificationEventDefinition {
  key: NotificationEventKey
  source: NotificationSourceKey
}

export interface NotificationPreferencesResponse {
  success: boolean
  preferences: NotificationPreferences
  catalog: {
    sources: NotificationSourceKey[]
    events: NotificationEventDefinition[]
  }
}

export const DEFAULT_NOTIFICATION_PREFERENCES: NotificationPreferences = {
  enabled: true,
  sources: Object.fromEntries(
    NOTIFICATION_SOURCE_KEYS.map((key) => [key, true]),
  ) as Record<NotificationSourceKey, boolean>,
  events: Object.fromEntries(
    NOTIFICATION_EVENT_KEYS.map((key) => [key, true]),
  ) as Record<NotificationEventKey, boolean>,
  delivery: {
    island: true,
    toast: true,
    browser: true,
  },
  locations: Object.fromEntries(
    NOTIFICATION_SOURCE_KEYS.map((key) => [
      key,
      { panel: true, toast: true, island: true, browser: true },
    ]),
  ) as Record<NotificationSourceKey, NotificationLocationPreferences>,
}

export const DEFAULT_NOTIFICATION_CATALOG = {
  sources: Iterator.from(NOTIFICATION_SOURCE_KEYS).toArray(),
  events: NOTIFICATION_EVENT_KEYS.map((key) => ({
    key,
    source: key.split('.')[0] as NotificationSourceKey,
  })),
}

export const NOTIFICATION_PREFERENCES_UPDATED_EVENT =
  'notification-preferences-updated'

export function cloneNotificationPreferences(
  preferences: NotificationPreferences,
): NotificationPreferences {
  return {
    ...preferences,
    sources: { ...preferences.sources },
    events: { ...preferences.events },
    delivery: { ...preferences.delivery },
    locations: Object.fromEntries(
      Object.entries(preferences.locations).map(([source, locations]) => [
        source,
        { ...locations },
      ]),
    ) as NotificationPreferences['locations'],
  }
}

export function areNotificationPreferencesEqual(
  left: NotificationPreferences,
  right: NotificationPreferences,
): boolean {
  return JSON.stringify(left) === JSON.stringify(right)
}

const BASE = '/agent/notifications/preferences'

export const notificationPreferencesApi = {
  get(): Promise<NotificationPreferencesResponse> {
    return apiService.get(BASE)
  },

  async update(
    preferences: NotificationPreferences,
    userId?: number,
  ): Promise<NotificationPreferences> {
    const response = await apiService.put<{
      success: boolean
      message?: string
      preferences: NotificationPreferences
    }>(BASE, preferences)
    if (!response.success || !response.preferences) {
      throw new Error(
        await formatUserFacingError(
          response.message,
          currentCopy().errors.notificationPrefsSaveFailed,
        ),
      )
    }
    window.dispatchEvent(
      new CustomEvent(NOTIFICATION_PREFERENCES_UPDATED_EVENT, {
        detail: { preferences: response.preferences, userId },
      }),
    )
    return response.preferences
  },
}

export default notificationPreferencesApi
