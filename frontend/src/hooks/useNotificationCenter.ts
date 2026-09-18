import type { MeropeStateEventDetail } from '../features/merope/performanceEvents'
import type {
  AppNotification,
  LiveSpeechEvent,
  NotificationStreamEvent,
} from '../services/notificationApi'

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { currentCopy } from '../i18n/localeCopy'
import notificationApi from '../services/notificationApi'
import { formatUserFacingError } from '../utils/formatUserFacingError'
import { showError } from '../utils/toastManager'

/** 历史/SSE 增量封顶，防止长会话无限增长。 */
const MAX_ITEMS = 100

export interface UseNotificationCenterOptions {
  enabled: boolean
  userId?: number
  onNew?: (notification: AppNotification) => void
  /** On-page persona speech. Must not enter the notification list. */
  onLiveSpeech?: (speech: LiveSpeechEvent) => void
  onLiveSpeechMotion?: (id: string, performance: unknown) => void
  onMeropeState?: (state: MeropeStateEventDetail) => void
  onMeropeResync?: () => void
  includeInPanel?: (notification: AppNotification) => boolean
}

export function useNotificationCenter({
  enabled,
  userId,
  onNew,
  onLiveSpeech,
  onLiveSpeechMotion,
  onMeropeState,
  onMeropeResync,
  includeInPanel,
}: UseNotificationCenterOptions) {
  const [items, setItems] = useState<AppNotification[]>([])
  const [loaded, setLoaded] = useState(false)
  const onNewRef = useRef(onNew)
  onNewRef.current = onNew
  const onLiveSpeechRef = useRef(onLiveSpeech)
  onLiveSpeechRef.current = onLiveSpeech
  const onLiveSpeechMotionRef = useRef(onLiveSpeechMotion)
  onLiveSpeechMotionRef.current = onLiveSpeechMotion
  const onMeropeStateRef = useRef(onMeropeState)
  onMeropeStateRef.current = onMeropeState
  const onMeropeResyncRef = useRef(onMeropeResync)
  onMeropeResyncRef.current = onMeropeResync
  const includeInPanelRef = useRef(includeInPanel)
  includeInPanelRef.current = includeInPanel

  // 丢弃登出后才到达的历史响应，避免污染下一个用户。
  const enabledRef = useRef(enabled)
  enabledRef.current = enabled
  const userIdRef = useRef(userId)
  userIdRef.current = userId

  const loadHistory = useCallback(async () => {
    const requestedUserId = userId
    try {
      const res = await notificationApi.list(50)
      if (!enabledRef.current || userIdRef.current !== requestedUserId) return
      setItems(res.notifications)
      setLoaded(true)
    } catch (e) {
      console.warn('[NotificationCenter] Failed to load history:', e)
      if (!enabledRef.current || userIdRef.current !== requestedUserId) return
      showError(
        await formatUserFacingError(
          e,
          currentCopy().notificationCenter.loadFailed,
        ),
      )
      setLoaded(true)
    }
  }, [userId])

  const loadHistoryRef = useRef(loadHistory)
  loadHistoryRef.current = loadHistory

  useEffect(() => {
    // 登出即清空，避免下一用户短暂看到上一用户的通知。
    if (!enabled) {
      setItems([])
      setLoaded(false)
      return
    }

    // 即使 enabled 都是 true，账号切换也必须清数据并重建连接。
    setItems([])
    setLoaded(false)
    let active = true
    const close = notificationApi.subscribe(
      (event: NotificationStreamEvent) => {
        if (!active || !enabledRef.current || userId !== userIdRef.current)
          return
        if (event.event === 'new_notification') {
          const n = event.notification

          // 旧 EventSource cleanup 窗口内再按 payload owner 校验一次。
          if (n.user_id !== userIdRef.current) return

          setItems((prev) =>
            // 运行中任务用稳定通知 ID；新进度替换旧快照并移到顶部。
            [n, ...prev.filter((p) => p.id !== n.id)].slice(0, MAX_ITEMS),
          )

          // 稳定 ID 只用于替换面板快照，不能再当 Toast/岛/系统通知的隐式过滤。
          onNewRef.current?.(n)
        } else if (event.event === 'notification_deleted') {
          if (event.user_id !== userIdRef.current) return
          setItems((prev) => prev.filter((p) => p.id !== event.id))
        } else if (event.event === 'notifications_cleared') {
          if (event.user_id !== userIdRef.current) return
          setItems([])
        } else if (event.event === 'live_speech') {
          if (event.user_id !== userIdRef.current) return
          onLiveSpeechRef.current?.(event.speech)
        } else if (event.event === 'live_speech_motion') {
          if (event.user_id !== userIdRef.current) return
          onLiveSpeechMotionRef.current?.(event.id, event.performance)
        } else if (event.event === 'merope_state_changed') {
          if (event.user_id !== userIdRef.current) return
          onMeropeStateRef.current?.(event)
        // broadcast 丢事件后后端发 resync；补拉历史。
        } else if (event.event === 'resync') {
          void loadHistoryRef.current()
          onMeropeResyncRef.current?.()
        }
      },
      {

        onReconnect: () => {
          if (!active || !enabledRef.current || userId !== userIdRef.current)
            return
          void loadHistoryRef.current()
          onMeropeResyncRef.current?.()
        },
      },
    )
    return () => {
      active = false
      close()
    }
  }, [enabled, userId])

  useEffect(() => {
    // 启用即拉历史：刷新后不能等打开通知页才加载。
    if (enabled) void loadHistory()
  }, [enabled, loadHistory])

  const removeItem = useCallback(
    async (n: AppNotification) => {
      setItems((prev) => prev.filter((p) => p.id !== n.id))
      try {
        await notificationApi.remove(n.id)
      } catch (e) {
        console.warn('[NotificationCenter] delete failed:', e)
        showError(
          await formatUserFacingError(
            e,
            currentCopy().errors.notificationDeleteFailed,
          ),
        )
        void loadHistory()
      }
    },
    [loadHistory],
  )

  const clearAll = useCallback(async () => {
    setItems([])
    try {
      await notificationApi.clearAll()
    } catch (e) {
      console.warn('[NotificationCenter] clear all failed:', e)
      showError(
        await formatUserFacingError(
          e,
          currentCopy().errors.notificationClearFailed,
        ),
      )
      void loadHistory()
    }
  }, [loadHistory])

  const panelItems = useMemo(
    () =>
      includeInPanelRef.current
        ? items.filter(includeInPanelRef.current)
        : items,
    [items, includeInPanel],
  )

  return useMemo(
    () => ({
      items: panelItems,
      loaded,
      loadHistory,
      removeItem,
      clearAll,
    }),
    [panelItems, loaded, loadHistory, removeItem, clearAll],
  )
}

export type NotificationCenterState = ReturnType<typeof useNotificationCenter>
