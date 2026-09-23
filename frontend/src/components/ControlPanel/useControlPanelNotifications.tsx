import type { AppNotification } from '../../services/notificationApi'
import type { DynamicContent } from './islandContentTypes'
import type { PanelTab } from './panelTransition'
import { useCallback, useEffect, useRef, useSyncExternalStore } from 'react'
import { useNotificationCenter } from '../../hooks/useNotificationCenter'
import { useNotificationPreferences } from '../../hooks/useNotificationPreferences'
import {
  notificationSourceFor,
  notificationToastType,
  shouldEmitNotificationToast,
  shouldSurfaceNotification,
} from '../../services/notificationDelivery'
import { emitAppEvent } from '../../utils/appEvents'
import { notificationFacingBody, notificationFacingTitle } from '../../utils/notificationFacing'
import { showToast } from '../../utils/toastManager'
import { isLookingAtAgentPanel, subscribeLookingAtAgentPanel } from '../agent-panel/agentPanelVisible'
import { ADDRESSEE_UPDATED_EVENT } from '../agent/meropeVitals'
import { NotificationSourceIcon, notificationSourceIconAsset } from '../notifications/NotificationIcons'

/** Own notification delivery and its transient island content, independently of panel motion. */
export function useControlPanelNotifications({
  enabled,
  userId,
  panelTab,
  onIslandNotification,
}: {
  enabled: boolean
  userId?: number
  panelTab: PanelTab
  onIslandNotification: (content: DynamicContent | null) => void
}) {
  const lookingAtAgent = useSyncExternalStore(
    subscribeLookingAtAgentPanel,
    isLookingAtAgentPanel,
    () => false,
  )
  const { preferences: notificationPreferences } = useNotificationPreferences(userId)
  const notifCarouselTimerRef = useRef<ReturnType<typeof setTimeout> | null>(
    null,
  )

  useEffect(() => {
    // Account changes invalidate transient delivery too, not only history/SSE.
    onIslandNotification(null)
    return () => {
      if (notifCarouselTimerRef.current !== null) {
        clearTimeout(notifCarouselTimerRef.current)
        notifCarouselTimerRef.current = null
      }
    }
  }, [enabled, userId, onIslandNotification])

  const handleNewNotification = useCallback(
    (n: AppNotification) => {
      const source = notificationSourceFor(n)
      const icon = (
        <NotificationSourceIcon source={source} className="h-4 w-4" />
      )
      const title = notificationFacingTitle(n)
      const body = notificationFacingBody(n)
      const snippet = body.length > 60 ? `${body.slice(0, 60)}…` : body

      if (
        shouldSurfaceNotification(
          notificationPreferences,
          n,
          'island',
          lookingAtAgent,
        )
      ) {
        onIslandNotification({
            type: 'notification',
            icon,
            text: title,
            subtext: snippet,
            showSubtext: true,
        })
        if (notifCarouselTimerRef.current) {
          clearTimeout(notifCarouselTimerRef.current)
        }
        notifCarouselTimerRef.current = setTimeout(() => {
          notifCarouselTimerRef.current = null
          onIslandNotification(null)
        }, 20000)
      }

      // Toast 只改视觉类型，不决定是否展示。
      if (
        shouldEmitNotificationToast(
          notificationPreferences,
          n,
          lookingAtAgent,
        )
      ) {
        const showInPanel = shouldSurfaceNotification(
          notificationPreferences,
          n,
          'panel',
          lookingAtAgent,
        )
        showToast({
          title,
          message: snippet,
          type: notificationToastType(n),
          icon: notificationSourceIconAsset(source),
          duration: 6000,
          showCloseButton: true,
          onClick: showInPanel
            ? () => {
                // 复用打开面板事件带 tab：已展开则只切 tab。
                emitAppEvent('open-control-panel', { tab: 'notifications' })
              }
            : undefined,
        })
      }

      if (
        document.hidden &&
        shouldSurfaceNotification(
          notificationPreferences,
          n,
          'browser',
          lookingAtAgent,
        ) &&
        typeof Notification !== 'undefined' &&
        Notification.permission === 'granted'
      ) {
        try {
          // Notification 构造即展示；同 id 用 tag 去重。
          void new Notification(title, {
            body: body.slice(0, 200),
            tag: n.id,
            icon: notificationSourceIconAsset(source),
          })
        } catch {
        }
      }
    },
    [lookingAtAgent, notificationPreferences, onIslandNotification],
  )

  const includeNotificationInPanel = useCallback(
    (notification: AppNotification) =>
      shouldSurfaceNotification(
        notificationPreferences,
        notification,
        'panel',
        lookingAtAgent,
      ),
    [lookingAtAgent, notificationPreferences],
  )

  const handleLiveSpeech = useCallback(
    (speech: {
      id: string
      event_key: string
      body: string
      performance?: unknown
      merope_state?: unknown
    }) => {
      void Promise.all([
        import('../../features/merope/faceSpeechArbitration'),
        import('../../features/merope/agentFaceChannel'),
      ]).then(([{ deliverProactiveFace, faceSpeechGate }, { agentFace }]) => {
        deliverProactiveFace(agentFace, faceSpeechGate, {
          id: speech.id,
          eventKey: speech.event_key,
          body: speech.body,
          performance: speech.performance,
          meropeState: speech.merope_state,
        })
      })
    },
    [],
  )

  const notifCenter = useNotificationCenter({
    enabled,
    userId,
    onNew: handleNewNotification,
    onLiveSpeech: handleLiveSpeech,
    onLiveSpeechMotion: (id, performance) => {
      void import('../../features/merope/faceSpeechArbitration').then(
        ({ refineProactiveFace, faceSpeechGate }) => {
          refineProactiveFace(faceSpeechGate, id, performance)
        },
      )
    },
    onMeropeState: (state) => {
      void import('../../features/merope/agentFaceChannel').then(({ agentFace }) => {
        agentFace.updateState(state)
      })
    },
    onMeropeResync: () => window.dispatchEvent(new Event(ADDRESSEE_UPDATED_EVENT)),
    includeInPanel: includeNotificationInPanel,
  })
  const { loaded: notifLoaded, loadHistory: loadNotifHistory } = notifCenter

  // hook 预载失败时，打开通知页再试一次。
  useEffect(() => {
    if (panelTab === 'notifications' && !notifLoaded) {
      void loadNotifHistory()
    }
  }, [panelTab, notifLoaded, loadNotifHistory])

  return { notifCenter, notificationPreferences }
}
