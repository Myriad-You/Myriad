/**
 * 将 Agent 前端操作处理逻辑从 TappWindowManager 解耦
 *
 * 通过 Hook 封装 registerActionHandler / unregisterActionHandler 调用，
 * TappWindowManager 不再直接依赖 agent 服务实现细节。
 */

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
  /** 当前所有窗口的 ref（避免 useEffect 依赖频繁变化） */
  windowsRef: React.RefObject<WindowRef[]>
  /** 当前活跃窗口 ID 的 ref */
  activeWindowIdRef: React.RefObject<string | null>
  /** 打开一个 Tapp 窗口 */
  openTappWindow: (
    tappId: string,
    opts?: {
      size?: { width?: number; height?: number }
      position?: { x?: number; y?: number }
    },
  ) => Promise<void>
  /** 关闭窗口 */
  closeWindow: (windowId: string) => void
  /** 聚焦窗口 */
  focusWindow: (windowId: string) => void
}

/**
 * 注册 Agent 前端操作处理器（typed），在卸载时自动注销。
 * Typed 优先于全局 fallback（App GlobalAgentWindowHandler），
 * 多窗挂载时接管 open_window；卸载后全局 navigate 回退生效。
 */
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
