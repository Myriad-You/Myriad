import type { NotificationCenterState } from '../hooks/useNotificationCenter'
import type { AppNotification } from '../services/notificationApi'
import type { NotificationSourceKey } from '../services/notificationPreferencesApi'
/**
 * 通知列表面板（智能岛「通知」tab 的内容区）
 *
 * iOS 通知中心模型：无已读概念，通知堆积直到被清除。
 * 页头为问候语 + 日期，右侧清理按钮先展示 X 图标，
 * 点击后变为文本二次确认（3 秒未确认自动还原）。
 * 点击通知直接跳转对应内容（任务类 → Agent 会话），无落点时展开详情。
 * 联邦邀请类通知可从 metadata.actions 一键 Accept / Reject。
 */
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../contexts/I18nContext'
import { federationApi } from '../services/federationApi'
import { notificationSourceFor } from '../services/notificationDelivery'
import { getGreeting } from '../utils/dynamicContent'
import {
  notificationFacingBody,
  notificationFacingTitle,
} from '../utils/notificationFacing'
import { userFacingError } from '../utils/userFacingError'
import { NotificationSourceIcon } from './notifications/NotificationIcons'

/** metadata.actions 单项（后端 notify 写入） */
interface NotifAction {
  id: string
  label?: string
  api?: string
}

function parseNotifActions(n: AppNotification): NotifAction[] {
  const raw = n.metadata?.actions
  if (!Array.isArray(raw)) return []
  const out: NotifAction[] = []
  for (const a of raw) {
    if (!a || typeof a !== 'object') continue
    const o = a as Record<string, unknown>
    const id = typeof o.id === 'string' ? o.id : ''
    if (!id) continue
    out.push({
      id,
      label: typeof o.label === 'string' ? o.label : undefined,
      api: typeof o.api === 'string' ? o.api : undefined,
    })
  }
  return out
}

/** 根据 kind + action id 调用联邦 API */
async function runFederationInviteAction(
  n: AppNotification,
  actionId: string,
): Promise<void> {
  const kind = n.metadata?.kind
  const roomId =
    typeof n.metadata?.room_id === 'string' ? n.metadata.room_id : ''
  const channelId =
    typeof n.metadata?.channel_id === 'string' ? n.metadata.channel_id : ''

  if (kind === 'room_invite' || (roomId && !channelId)) {
    if (!roomId) throw new Error('Missing room_id')
    if (actionId === 'accept') {
      await federationApi.acceptRoomInvite(roomId)
      return
    }
    if (actionId === 'reject') {
      await federationApi.rejectRoomInvite(roomId)
      return
    }
  }
  if (kind === 'channel_invite' || channelId) {
    if (!channelId) throw new Error('Missing channel_id')
    if (actionId === 'accept') {
      await federationApi.acceptChannel(channelId)
      return
    }
    if (actionId === 'reject') {
      await federationApi.closeChannel(channelId)
      return
    }
  }
  throw new Error(`Unsupported invite action: ${actionId}`)
}

/** Apple 风格胶囊按钮基础样式 */
const PILL_BTN =
  'rounded-full px-3 py-1 text-xs font-medium transition-colors ' +
  'bg-black/5 text-gray-600 hover:bg-black/9 ' +
  'dark:bg-white/10 dark:text-gray-300 dark:hover:bg-white/15'

/** 通知的跳转目标 */
type NotifTarget =
  | {
      kind: 'session'
      sessionId: string
      runId?: string
      taskId?: string
    }
  | { kind: 'route'; path: string }
  | { kind: 'arael_manage'; tab?: 'heartbeat' | 'skills' | 'memory' }
  | null

/** 解析点击落点：任务类通知带 session_id 时跳回对应 Agent 会话（可带 run/task 以 reattach） */
function resolveTarget(n: AppNotification): NotifTarget {
  if (n.metadata?.action === 'open_arael') {
    const sid =
      typeof n.metadata?.session_id === 'string' ? n.metadata.session_id : ''
    const runId =
      typeof n.metadata?.run_id === 'string' ? n.metadata.run_id : undefined
    const taskId =
      typeof n.metadata?.task_id === 'string' ? n.metadata.task_id : undefined
    return { kind: 'session', sessionId: sid, runId, taskId }
  }
  if (
    n.notification_type === 'task_progress' ||
    n.notification_type === 'task_completed' ||
    n.notification_type === 'task_failed' ||
    n.notification_type === 'task_cancelled' ||
    n.notification_type === 'agent_clarification'
  ) {
    const sid = n.metadata?.session_id
    if (typeof sid === 'string' && sid) {
      const runId =
        typeof n.metadata?.run_id === 'string' ? n.metadata.run_id : undefined
      const taskId =
        typeof n.metadata?.task_id === 'string' ? n.metadata.task_id : undefined
      return { kind: 'session', sessionId: sid, runId, taskId }
    }
  }
  const route = n.metadata?.route
  if (typeof route === 'string' && route.startsWith('/')) {
    return { kind: 'route', path: route }
  }
  if (n.metadata?.action === 'open_arael_manage') {
    const rawTab = n.metadata?.tab
    const tab =
      rawTab === 'skills' || rawTab === 'memory' || rawTab === 'heartbeat'
        ? rawTab
        : undefined
    return { kind: 'arael_manage', tab }
  }
  return null
}

interface Props {
  center: NotificationCenterState
  /** 填满父容器高度（覆盖层模式：继承控制面板高度，列表内部滚动） */
  fill?: boolean
  /** 打开 Agent 会话（由 GlobalControlPanel 注入：收起面板 + 派发打开事件） */
  onOpenSession?: (
    sessionId: string,
    opts?: { runId?: string; taskId?: string },
  ) => void
  /** 打开普通应用路由（如 Brew 新内容） */
  onNavigate?: (path: string) => void
  /** 打开 Agent 管理面板（Heartbeat / Skills 通知；可选初始 tab） */
  onOpenAraelManage?: (tab?: 'heartbeat' | 'skills' | 'memory') => void
  /** 当前用户是否允许浏览器系统通知。 */
  browserNotificationsEnabled?: boolean
}

function NotificationPanelList({
  center,
  fill,
  onOpenSession,
  onNavigate,
  onOpenAraelManage,
  browserNotificationsEnabled = true,
}: Props) {
  const { t, format, locale } = useI18n()
  const { items, removeItem, clearAll } = center

  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [confirmClear, setConfirmClear] = useState(false)
  const [actionBusyId, setActionBusyId] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const [notifPermission, setNotifPermission] = useState<string>(
    typeof Notification !== 'undefined' ? Notification.permission : 'denied',
  )
  const confirmTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // tab 切换 / 面板收起会卸载本组件，确认倒计时须随之清理
  useEffect(
    () => () => {
      if (confirmTimerRef.current) clearTimeout(confirmTimerRef.current)
    },
    [],
  )

  // 问候语 + 本地化日期（组件随 tab 打开重挂载，时点足够新鲜）
  const { greetingText, dateText } = useMemo(() => {
    const greeting = getGreeting(
      undefined,
      {
        morning: t.greeting?.morning ?? 'Good morning',
        forenoon: t.greeting?.forenoon ?? t.greeting?.morning ?? 'Good morning',
        noon: t.greeting?.noon ?? 'Good afternoon',
        afternoon: t.greeting?.afternoon ?? 'Good afternoon',
        dusk: t.greeting?.dusk ?? t.greeting?.evening ?? 'Good evening',
        evening: t.greeting?.evening ?? 'Good evening',
        night: t.greeting?.night ?? 'Good night',
      },
      locale,
    )
    const date = new Intl.DateTimeFormat(locale, {
      month: 'long',
      day: 'numeric',
      weekday: 'long',
    }).format(new Date())
    return { greetingText: greeting.text, dateText: date }
  }, [t, locale])

  const handleClearAll = useCallback(() => {
    if (!confirmClear) {
      setConfirmClear(true)
      if (confirmTimerRef.current) clearTimeout(confirmTimerRef.current)
      confirmTimerRef.current = setTimeout(setConfirmClear, 3000, false)
      return
    }
    if (confirmTimerRef.current) clearTimeout(confirmTimerRef.current)
    setConfirmClear(false)
    void clearAll()
  }, [confirmClear, clearAll])

  const requestSystemNotif = useCallback(async () => {
    if (typeof Notification === 'undefined') return
    const perm = await Notification.requestPermission()
    setNotifPermission(perm)
  }, [])

  const handleItemClick = useCallback(
    (n: AppNotification) => {
      const target = resolveTarget(n)
      if (target?.kind === 'session' && onOpenSession) {
        onOpenSession(target.sessionId, {
          runId: target.runId,
          taskId: target.taskId,
        })
        return
      }
      if (target?.kind === 'route' && onNavigate) {
        onNavigate(target.path)
        return
      }
      if (target?.kind === 'arael_manage' && onOpenAraelManage) {
        onOpenAraelManage(target.tab)
        return
      }
      // 无落点：展开/收起详情
      setExpandedId((prev) => (prev === n.id ? null : n.id))
    },
    [onOpenSession, onNavigate, onOpenAraelManage],
  )

  const handleInviteAction = useCallback(
    async (n: AppNotification, actionId: string) => {
      if (actionBusyId) return
      setActionBusyId(`${n.id}:${actionId}`)
      setActionError(null)
      try {
        await runFederationInviteAction(n, actionId)
        void removeItem(n)
        // 接受后若有 route，顺带打开对应会话
        if (actionId === 'accept') {
          const route = n.metadata?.route
          if (typeof route === 'string' && route.startsWith('/') && onNavigate) {
            onNavigate(route)
          }
        }
      } catch (err) {
        setActionError(
          userFacingError(err, t.errors.notificationActionFailed),
        )
      } finally {
        setActionBusyId(null)
      }
    },
    [actionBusyId, removeItem, onNavigate, t],
  )

  const relativeTime = useCallback(
    (iso: string): string => {
      const diffMs = Date.now() - new Date(iso).getTime()
      const minutes = Math.floor(diffMs / 60000)
      if (minutes < 1) return t.notificationCenter.justNow
      if (minutes < 60)
        return format(t.notificationCenter.minutesAgo, { n: minutes })
      const hours = Math.floor(minutes / 60)
      if (hours < 24) return format(t.notificationCenter.hoursAgo, { n: hours })
      return format(t.notificationCenter.daysAgo, {
        n: Math.floor(hours / 24),
      })
    },
    [t, format],
  )

  /** 发信源名称（iOS 通知头行的 App 名位置） */
  const sourceLabels = useMemo<Record<NotificationSourceKey, string>>(
    () => ({
      agent: t.notificationCenter.sourceAgent,
      heartbeat: t.notificationCenter.sourceHeartbeat,
      mcp: t.notificationCenter.sourceMcp,
      brew: 'Brew',
      tapp: 'Tapp',
      updater: t.notificationCenter.sourceSystem,
      federation: t.notificationCenter.sourceAro,
      system: t.notificationCenter.sourceSystem,
    }),
    [t],
  )

  return (
    <div className={`flex flex-col min-h-0 ${fill ? 'h-full' : ''}`}>
      {/* 页头：问候语 + 日期，右侧为系统通知开关与清理按钮 */}
      <div className="flex items-start justify-between gap-2 px-0.5 pb-2.5">
        <div className="min-w-0 leading-tight">
          <div className="text-xs font-medium text-gray-400 dark:text-gray-500">
            {dateText}
          </div>
          <div className="truncate text-lg font-bold text-gray-800 dark:text-gray-100">
            {greetingText}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {browserNotificationsEnabled && notifPermission === 'default' && (
            <button
              type="button"
              onClick={() => void requestSystemNotif()}
              className={PILL_BTN}
            >
              {t.notificationCenter.enableSystemNotif}
            </button>
          )}
          {items.length > 0 &&
            (confirmClear ? (
              // 二次确认态：与图标态同高（h-7），仅内容由图标换为文本，
              // 保持中性配色（不变红）——语气克制，符合 Apple 的清除确认调性
              <button
                type="button"
                onClick={handleClearAll}
                className="flex h-7 items-center rounded-full bg-black/5 px-3
                  text-xs font-medium text-gray-600 transition-colors hover:bg-black/9
                  dark:bg-white/10 dark:text-gray-300 dark:hover:bg-white/15"
              >
                {t.notificationCenter.clearConfirm}
              </button>
            ) : (
              <button
                type="button"
                onClick={handleClearAll}
                aria-label={t.notificationCenter.clearAll}
                className="flex h-7 w-7 items-center justify-center rounded-full
                  bg-black/5 text-gray-500 transition-colors hover:bg-black/9
                  dark:bg-white/10 dark:text-gray-400 dark:hover:bg-white/15"
              >
                {/* 清空全部：用簸箕/垃圾桶图标，与单条删除的 X 区分语义 */}
                <svg
                  className="h-3.5 w-3.5"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth={1.8}
                  viewBox="0 0 24 24"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0"
                  />
                </svg>
              </button>
            ))}
        </div>
      </div>

      {/* 列表：覆盖层模式填满剩余空间内部滚动，否则回退到视口上限 */}
      <div
        className={`flex-1 overflow-y-auto overscroll-contain ${
          fill ? 'min-h-0' : 'max-h-[50vh]'
        }`}
      >
        {items.length === 0 ? (
          <div className="py-10 text-center text-sm text-gray-400 dark:text-gray-500">
            {t.notificationCenter.empty}
          </div>
        ) : (
          items.map((n) => {
            const source = notificationSourceFor(n)
            const inviteActions = parseNotifActions(n)
            return (
              <div
                key={n.id}
                role="button"
                tabIndex={0}
                onClick={() => handleItemClick(n)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault()
                    handleItemClick(n)
                  }
                }}
                className="group relative mb-1.5 flex w-full cursor-pointer items-start gap-2.5
                rounded-xl px-2.5 py-2.5 text-left transition-colors
                bg-black/3 hover:bg-black/6
                dark:bg-white/4 dark:hover:bg-white/8"
              >
                {/* 直接展示透明来源图标，不额外叠加背景容器 */}
                <NotificationSourceIcon
                  source={source}
                  className="h-10 w-10 shrink-0 object-contain"
                />

                <div className="min-w-0 flex-1">
                  {/* 头行：发信源名 + 时间 */}
                  <div className="flex items-center gap-1.5">
                    <span className="truncate text-[10px] font-medium tracking-wide text-gray-400 dark:text-gray-500">
                      {sourceLabels[source]}
                    </span>
                    <span className="ml-auto shrink-0 text-[10px] text-gray-400 dark:text-gray-500">
                      {relativeTime(n.created_at)}
                    </span>
                  </div>

                  <div className="mt-0.5 truncate text-sm font-semibold text-gray-800 dark:text-gray-100">
                    {notificationFacingTitle(n)}
                  </div>
                  <p
                    className={`mt-0.5 text-xs text-gray-500 dark:text-gray-400 whitespace-pre-wrap break-words ${
                      expandedId === n.id ? '' : 'line-clamp-2'
                    }`}
                  >
                    {notificationFacingBody(n)}
                  </p>
                  {typeof n.metadata?.progress === 'number' &&
                    (n.notification_type === 'task_progress' ||
                      n.notification_type === 'agent_clarification') && (
                      <div className="mt-2 h-1 overflow-hidden rounded-full bg-black/8 dark:bg-white/10">
                        <div
                          className="h-full rounded-full bg-blue-500 transition-[width] duration-300 dark:bg-blue-400"
                          style={{
                            width: `${Math.max(0, Math.min(100, n.metadata.progress))}%`,
                          }}
                        />
                      </div>
                    )}
                  {inviteActions.length > 0 && (
                    <div
                      className="mt-2 flex flex-wrap gap-1.5"
                      onClick={(e) => e.stopPropagation()}
                      onKeyDown={(e) => e.stopPropagation()}
                    >
                      {inviteActions.map((act) => {
                        const busy =
                          actionBusyId === `${n.id}:${act.id}`
                        const isAccept = act.id === 'accept'
                        const isReject = act.id === 'reject'
                        return (
                          <button
                            key={act.id}
                            type="button"
                            disabled={!!actionBusyId}
                            onClick={() => void handleInviteAction(n, act.id)}
                            className={
                              `rounded-full px-3 py-1 text-xs font-medium transition-colors disabled:opacity-50 ${
                              isAccept
                                ? 'bg-emerald-500/15 text-emerald-700 hover:bg-emerald-500/25 dark:text-emerald-300'
                                : isReject
                                  ? 'bg-red-500/10 text-red-600 hover:bg-red-500/20 dark:text-red-300'
                                  : 'bg-black/5 text-gray-600 hover:bg-black/10 dark:bg-white/10 dark:text-gray-300'}`
                            }
                          >
                            {busy
                              ? '…'
                              : isAccept
                                ? t.common?.confirm || 'Accept'
                                : isReject
                                  ? t.common?.cancel || 'Decline'
                                  : act.label || act.id}
                          </button>
                        )
                      })}
                    </div>
                  )}
                  {actionError && expandedId === n.id && (
                    <p className="mt-1 text-[11px] text-red-500">{actionError}</p>
                  )}
                </div>

                <button
                  type="button"
                  aria-label={t.common.delete}
                  onClick={(e) => {
                    e.stopPropagation()
                    void removeItem(n)
                  }}
                  className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full
                  text-gray-300 dark:text-gray-600 hover:text-red-500 dark:hover:text-red-400
                  hover:bg-red-50 dark:hover:bg-red-950/40
                  opacity-0 group-hover:opacity-100 focus:opacity-100 max-sm:opacity-60
                  transition-all"
                >
                  <svg
                    className="w-3.5 h-3.5"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth={2}
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      d="M6 18L18 6M6 6l12 12"
                    />
                  </svg>
                </button>
              </div>
            )
          })
        )}
      </div>
    </div>
  )
}

// memo：面板展开且音乐播放时父组件（GlobalControlPanel）每秒重渲染，
// center 已由 hook 端 useMemo 稳定，通知无变化时整个列表跳过重渲染
export default memo(NotificationPanelList)
