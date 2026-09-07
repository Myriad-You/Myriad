import type { ToastType } from '../components/Toast'
import type { AppNotification } from './notificationApi'
import type {
  NotificationEventKey,
  NotificationPreferences,
  NotificationSourceKey,
} from './notificationPreferencesApi'
import { NOTIFICATION_EVENT_KEYS } from './notificationPreferencesApi'

export type NotificationLocation = 'panel' | 'toast' | 'island' | 'browser'

const EVENT_KEYS = new Set<string>(NOTIFICATION_EVENT_KEYS)

export function notificationSourceFor(
  notification: AppNotification,
): NotificationSourceKey {
  const eventKey = notification.metadata?.event_key
  if (typeof eventKey === 'string') {
    const source = eventKey.split('.')[0]
    // skill.* events are Agent skill lifecycle (pruned/improved/changed) —
    // no dedicated source icon; fold into agent.
    if (source === 'skill') return 'agent'
    if (
      source === 'agent' ||
      source === 'heartbeat' ||
      source === 'mcp' ||
      source === 'brew' ||
      source === 'tapp' ||
      source === 'updater' ||
      source === 'federation' ||
      source === 'system'
    ) {
      return source
    }
  }
  if (notification.notification_type.startsWith('task_')) return 'agent'
  if (notification.notification_type === 'agent_clarification') return 'agent'
  if (notification.notification_type === 'heartbeat_result') return 'heartbeat'
  if (notification.notification_type === 'mcp_server_status') return 'mcp'
  if (notification.notification_type.startsWith('brew_')) return 'brew'
  if (notification.notification_type === 'tapp_notification') return 'tapp'
  if (notification.notification_type === 'updater_status') return 'updater'
  if (notification.notification_type.startsWith('federation_')) {
    return 'federation'
  }
  return 'system'
}

/**
 * 所有通知展示位置共用的唯一投递判断。
 * 后端会按总开关、来源和事件过滤创建；这里再次应用相同偏好，并叠加展示位置，
 * 从而让实时通知和刷新后的面板历史使用完全一致的策略。
 */
export function shouldDeliverNotification(
  preferences: NotificationPreferences,
  notification: AppNotification,
  location: NotificationLocation,
): boolean {
  if (!preferences.enabled) return false

  const source = notificationSourceFor(notification)
  if (!preferences.sources[source]) return false

  const eventKey = notification.metadata?.event_key
  if (
    typeof eventKey === 'string' &&
    EVENT_KEYS.has(eventKey) &&
    !preferences.events[eventKey as NotificationEventKey]
  ) {
    return false
  }

  if (!preferences.locations[source]?.[location]) return false
  if (location === 'panel') return true
  return preferences.delivery[location]
}

/**
 * Looking at the Agent panel: Agent notifications stay in the conversation.
 * No list, island, toast, or browser push.
 */
export function shouldSurfaceNotification(
  preferences: NotificationPreferences,
  notification: AppNotification,
  location: NotificationLocation,
  lookingAtAgentPanel: boolean,
): boolean {
  if (lookingAtAgentPanel && notificationSourceFor(notification) === 'agent') {
    return false
  }
  return shouldDeliverNotification(preferences, notification, location)
}

/**
 * Toast 只在人看不见 Agent 面板时弹。面板开着，通知已经在对话里，不必再盖一层。
 */
export function shouldEmitNotificationToast(
  preferences: NotificationPreferences,
  notification: AppNotification,
  lookingAtAgentPanel: boolean,
): boolean {
  if (lookingAtAgentPanel) return false
  return shouldDeliverNotification(preferences, notification, 'toast')
}

export function notificationToastType(
  notification: AppNotification,
): ToastType {
  const status = notification.metadata?.status
  const tappType = notification.metadata?.tapp_notification_type
  const failed =
    notification.notification_type === 'task_failed' ||
    notification.notification_type === 'brew_source_error' ||
    status === 'failed' ||
    tappType === 'error' ||
    tappType === 'danger'
  if (failed || notification.priority === 'urgent') return 'error'

  const succeeded =
    notification.notification_type === 'task_completed' ||
    status === 'completed' ||
    status === 'succeeded' ||
    tappType === 'success'
  if (succeeded) return 'success'
  if (tappType === 'warning') return 'warning'
  if (notification.priority === 'high') return 'warning'
  return 'info'
}
