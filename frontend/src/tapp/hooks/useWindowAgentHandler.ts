import type { FrontendAction, WindowTarget } from '../../services/agent'
import type { WindowRef } from './windowAgentTarget'

import { useEffect } from 'react'
import {
  registerActionHandler,
  unregisterActionHandler,
} from '../../services/agent'
import { resolveCloseWindowIds, resolveWindowTarget } from './windowAgentTarget'

export type { WindowRef }

interface UseWindowAgentHandlerOptions {
  windowsRef: React.RefObject<WindowRef[]>
  activeWindowIdRef: React.RefObject<string | null>
  openTappWindow: (
    tappId: string,
    opts?: {
      size?: { width?: number; height?: number }
      position?: { x?: number; y?: number }
    },
  ) => Promise<void>
  closeWindow: (windowId: string) => void
  focusWindow: (windowId: string) => void
}

export function useWindowAgentHandler({
  windowsRef,
  activeWindowIdRef,
  openTappWindow,
  closeWindow,
  focusWindow,
}: UseWindowAgentHandlerOptions): void {
  useEffect(() => {
    const openWindow = async (action: FrontendAction): Promise<unknown> => {
      const data = action.data as Record<string, unknown> | undefined
      const tappId =
        action.tappId ||
        (data?.tappId as string | undefined) ||
        (data?.tapp_id as string | undefined)
      if (!tappId) return false
      const size =
        action.size ||
        (data?.size as { width?: number; height?: number } | undefined)
      const position =
        action.position ||
        (data?.position as { x?: number; y?: number } | undefined)
      await openTappWindow(tappId, { size, position })
      return true
    }

    const closeWin = async (action: FrontendAction): Promise<unknown> => {
      const target = action.target as WindowTarget | undefined
      if (!target) return false
      const windowIds = resolveCloseWindowIds(
        target,
        windowsRef.current ?? [],
        activeWindowIdRef.current,
      )
      if (windowIds.length === 0) return false
      for (const windowId of windowIds) {
        closeWindow(windowId)
      }
      return true
    }

    const focusWin = async (action: FrontendAction): Promise<unknown> => {
      const target = action.target as WindowTarget | undefined
      if (!target) return false
      const windowId = resolveWindowTarget(
        target,
        windowsRef.current ?? [],
        activeWindowIdRef.current,
      )
      if (!windowId) return false
      focusWindow(windowId)
      return true
    }

    const agentInteraction = async (
      action: FrontendAction,
    ): Promise<unknown> => {
      if (!action.tappId || !action.interactionId) return false
      await openTappWindow(action.tappId)
      return true
    }

    const queryWindows = async (): Promise<unknown> => ({
      available: true,
      windows: windowsRef.current ?? [],
      activeWindowId: activeWindowIdRef.current,
      windowCount: windowsRef.current?.length ?? 0,
    })

    registerActionHandler('open_window', openWindow)
    registerActionHandler('close_window', closeWin)
    registerActionHandler('focus_window', focusWin)
    registerActionHandler('agent_interaction', agentInteraction)
    registerActionHandler('query_windows', queryWindows)

    return () => {
      unregisterActionHandler('open_window')
      unregisterActionHandler('close_window')
      unregisterActionHandler('focus_window')
      unregisterActionHandler('agent_interaction')
      unregisterActionHandler('query_windows')
    }
  }, [windowsRef, activeWindowIdRef, openTappWindow, closeWindow, focusWindow])
}
