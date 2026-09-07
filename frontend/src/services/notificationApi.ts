import type { MeropeStateEventDetail } from '../features/merope/performanceEvents'
/**
 * 通知中心 API
 *
 * 对接后端 /api/agent/notifications 系列端点：
 * - SSE 实时流（EventSource，cookie 认证）
 * - 历史列表 / 未读数
 */
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

/** SSE 流事件均由后端按 user_id 过滤，只发给通知 owner。 */
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
  /** 订阅方落后丢消息：应重新 list() 补全 */
  | { event: 'resync'; lagged_by: number }
  /** On-page persona speech. Not a notification-center item. */
  | { event: 'live_speech'; user_id: number; speech: LiveSpeechEvent }
  /** Persisted state update, never a notification-center item or a spoken line. */
  | ({ event: 'merope_state_changed'; user_id: number } & MeropeStateEventDetail)

const BASE = '/agent/notifications'

export interface NotificationSubscribeOptions {
  /**
   * EventSource 断线后浏览器自动重连成功时回调（首次 open 不触发）。
   * 用于 list() 补拉断线窗口内漏掉的通知。
   */
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

  /**
   * 订阅实时通知流。返回关闭函数。
   * EventSource 断线自动重连；认证走 cookie（withCredentials）。
   * 重连成功后触发 onReconnect（若提供），便于补拉历史。
   */
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
      // 浏览器会自动重连；标记后 onopen 触发 onReconnect
      wasError = true
    }

    source.onmessage = (msg) => {
      if (!msg.data) return
      try {
        const event = JSON.parse(msg.data) as NotificationStreamEvent
        onEvent(event)
      } catch {
        // 忽略无法解析的心跳/保活行
      }
    }

    return () => source.close()
  },
}

export default notificationApi
