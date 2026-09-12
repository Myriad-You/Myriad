import type { MeropeStateEventDetail } from '../features/merope/performanceEvents'
import { API_URL } from '../config'
import apiService from './api'

export type NotificationType =
  | 'task_progress'
  | 'task_completed'
  | 'task_failed'
  | 'task_cancelled'
  | 'heartbeat_result'
  | 'mcp_server_status'
  | 'brew_new_items'
  | 'brew_source_error'
  | 'tapp_notification'
  | 'updater_status'
  | 'system_info'
  | 'agent_clarification'
  | 'federation_message'
  | 'federation_follow'
  | 'federation_invite'

export type NotificationPriority = 'low' | 'normal' | 'high' | 'urgent'

export interface AppNotification {
  id: string
  notification_type: NotificationType
  priority: NotificationPriority
  title: string
  body: string
  user_id: number
  metadata?: Record<string, unknown> | null
  created_at: string
  read: boolean
}

export interface NotificationListResponse {
  notifications: AppNotification[]
  unread_count: number
  total: number
}

export interface TappNotificationRequest {
  tapp_id: string
  title?: string
  message: string
  notification_type?: 'success' | 'info' | 'warning' | 'error'
}

export interface LiveSpeechEvent {
  id: string
  body: string
  event_key: string
  performance?: unknown
  merope_state?: unknown
  intention_id?: string
}

export type NotificationStreamEvent =
  | { event: 'init'; unread_count: number }
  | { event: 'new_notification'; notification: AppNotification }
  | { event: 'notification_read'; id: string; user_id: number }
  | { event: 'notification_deleted'; id: string; user_id: number }
  | { event: 'notifications_cleared'; user_id: number }
  | { event: 'resync'; lagged_by: number }
  | { event: 'live_speech'; user_id: number; speech: LiveSpeechEvent }
  | {
      event: 'live_speech_motion'
      user_id: number
      id: string
      performance: unknown
    }
  | ({
      event: 'merope_state_changed'
      user_id: number
    } & MeropeStateEventDetail)

const BASE = '/agent/notifications'

export interface NotificationSubscribeOptions {
  onReconnect?: () => void
}

export const notificationApi = {
  async list(limit = 50): Promise<NotificationListResponse> {
    return apiService.get<NotificationListResponse>(`${BASE}?limit=${limit}`)
  },

  async publishTapp(request: TappNotificationRequest): Promise<string> {
    const response = await apiService.post<{
      success: boolean
      notification_id: string
    }>('/tapp/notifications', request)
    return response.notification_id
  },

  async remove(id: string): Promise<{ success: boolean }> {
    return apiService.delete(`${BASE}/${encodeURIComponent(id)}`)
  },

  async clearAll(): Promise<{ success: boolean; deleted: number }> {
    return apiService.post(`${BASE}/clear`)
  },

  /** EventSource: withCredentials. */
  subscribe(
    onEvent: (event: NotificationStreamEvent) => void,
    options?: NotificationSubscribeOptions,
  ): () => void {
    const source = new EventSource(`${API_URL}/api${BASE}/stream`, {
      withCredentials: true,
    })

    let hasOpenedOnce = false
    let wasError = false

    source.onopen = () => {
      if (hasOpenedOnce && wasError) {
        options?.onReconnect?.()
      }
      hasOpenedOnce = true
      wasError = false
    }

    source.onerror = () => {
      wasError = true
    }

    source.onmessage = (msg) => {
      if (!msg.data) return
      try {
        const event = JSON.parse(msg.data) as NotificationStreamEvent
        onEvent(event)
      } catch {
      }
    }

    return () => source.close()
  },
}

export default notificationApi
