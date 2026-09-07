import type { MeropeStateEventDetail } from '../features/merope/performanceEvents'
import type {
  AppNotification,
  LiveSpeechEvent,
  NotificationStreamEvent,
} from '../services/notificationApi'
/**
 * 通知中心状态 hook
 *
 * 封装 SSE 订阅、历史加载与删除/清空操作。
 * 无已读概念（iOS 通知中心模型）：通知堆积直到被清除，计数即列表长度。
 * 由 GlobalControlPanel（智能岛）独占消费——保持单一 SSE 连接。
 */
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { currentCopy } from '../i18n/localeCopy'
import notificationApi from '../services/notificationApi'
import { showError } from '../utils/toastManager'
import { userFacingError } from '../utils/userFacingError'

/** 列表长度上限：历史加载 50 条，SSE 增量在此封顶，防止长会话无限增长 */
const MAX_ITEMS = 100

export interface UseNotificationCenterOptions {
  /** 是否启用（未登录时应为 false） */
  enabled: boolean
  /** 当前用户 ID；账号切换时用于重建 SSE 并丢弃旧用户的迟到响应。 */
  userId?: number
  /** 新通知到达回调（用于轮播展示 / 系统通知 / toast） */
  onNew?: (notification: AppNotification) => void
  /** On-page persona speech. Must not enter the notification list. */
  onLiveSpeech?: (speech: LiveSpeechEvent) => void
  onMeropeState?: (state: MeropeStateEventDetail) => void
  onMeropeResync?: () => void
  /** 通知面板过滤器；实时与历史使用同一份显示位置策略。 */
  includeInPanel?: (notification: AppNotification) => boolean
}

export function useNotificationCenter({
  enabled,
  userId,
  onNew,
  onLiveSpeech,
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
  const onMeropeStateRef = useRef(onMeropeState)
  onMeropeStateRef.current = onMeropeState
  const onMeropeResyncRef = useRef(onMeropeResync)
  onMeropeResyncRef.current = onMeropeResync
  const includeInPanelRef = useRef(includeInPanel)
  includeInPanelRef.current = includeInPanel
  // enabled 镜像：丢弃登出后才到达的历史响应，避免污染下一个用户的状态
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
        userFacingError(e, currentCopy().notificationCenter.loadFailed),
      )
      setLoaded(true)
    }
  }, [userId])

  // 用 ref 暴露最新 loadHistory，避免 SSE 回调闭包过期
  const loadHistoryRef = useRef(loadHistory)
  loadHistoryRef.current = loadHistory

  // SSE 订阅（断线自动重连）
  useEffect(() => {
    if (!enabled) {
      // 登出即清空，避免下一个登录用户短暂看到上一用户的通知
      setItems([])
      setLoaded(false)
      return
    }
    // 即使两个账号之间 enabled 都是 true，也必须先清掉上一用户的数据并重建连接。
    setItems([])
    setLoaded(false)
    let active = true
    const close = notificationApi.subscribe(
      (event: NotificationStreamEvent) => {
        if (!active || !enabledRef.current || userId !== userIdRef.current) return
        if (event.event === 'new_notification') {
          const n = event.notification
          // 账号切换与旧 EventSource cleanup 之间可能有一个极短窗口；再按 payload
          // owner 校验一次，避免旧连接的最后一条事件进入新账号状态。
          if (n.user_id !== userIdRef.current) return
          // 运行中任务使用稳定通知 ID；新进度要替换旧快照并移到顶部。
          setItems((prev) =>
            [n, ...prev.filter((p) => p.id !== n.id)].slice(0, MAX_ITEMS),
          )
          // 每个后端实时通知事件都进入同一投递器。稳定 ID 只用于替换面板快照，
          // 不能再成为 Toast/智能岛/系统通知的隐式过滤条件。
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
        } else if (event.event === 'merope_state_changed') {
          if (event.user_id !== userIdRef.current) return
          onMeropeStateRef.current?.(event)
        } else if (event.event === 'resync') {
          // broadcast 丢事件后后端发 resync；补拉历史避免漏通知
          void loadHistoryRef.current()
          onMeropeResyncRef.current?.()
        }
        // init / notification_read：已读动作已从本仓客户端拿掉，忽略。
      },
      {
        // EventSource 闪断重连后补拉，覆盖 resync 之外的丢包窗口
        onReconnect: () => {
          if (!active || !enabledRef.current || userId !== userIdRef.current) return
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

  // 启用即拉取历史：此前列表要等打开通知页才加载，
  // 页面刷新后的历史通知完全无感知
  useEffect(() => {
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
          userFacingError(e, currentCopy().errors.notificationDeleteFailed),
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
        userFacingError(e, currentCopy().errors.notificationClearFailed),
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

  // 操作引用仅在账号（loadHistory）变化时更新；普通通知增量不会让下游操作失稳。
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
