/**
 * 沙箱共享订阅 Hook
 *
 * 提取 TappPageSandbox 和 TappWidgetSandbox 之间的重复订阅逻辑：
 * - 主题变化监听
 * - 主色调变化监听
 * - 生命周期暂停/恢复（document 可见性 + host minimize/paused 合成）
 */

import type { TappBridge } from './TappBridge'
import { useEffect, useRef } from 'react'
import { isPageVisible, onVisibility } from '../../hooks/animation'

import {
  getPrimaryColor,
  subscribeToPrimaryColor,
} from '../../utils/colorSubscriber'
import { subscribeToTheme } from '../../utils/themeSubscriber'

/**
 * 管理沙箱公共事件订阅（主题、主色调、可见性/暂停）
 *
 * Host "should run" is composed as `!paused && isPageVisible()`. A single
 * emitter owns lifecycle:pause / lifecycle:resume so visibility and minimize
 * cannot override each other.
 *
 * @param bridgeRef - TappBridge 引用
 * @param isReady - 沙箱是否就绪
 * @param paused - Host has hidden this surface (e.g. multi-window minimize)
 * @param canSubscribeTheme - granted `ui:theme:subscribe`; theme/primary-color
 * change forwarding is gated on it so subscription requires its own granted
 * permission (one-shot reads are gated separately via `ui:theme:read`).
 */
export function useSandboxSubscriptions(
  bridgeRef: React.RefObject<TappBridge | null>,
  isReady: boolean,
  paused = false,
  canSubscribeTheme = false,
): void {
  const pageVisibleRef = useRef(isPageVisible())
  const pausedRef = useRef(paused)
  pausedRef.current = paused

  /**
   * Last emitted "should run" for the current ready cycle.
   * `null` means nothing has been emitted yet for this bridge/iframe
   * (after remount we always re-emit current state).
   */
  const lastShouldRunRef = useRef<boolean | null>(null)

  const emitShouldRun = (shouldRun: boolean, force = false) => {
    const bridge = bridgeRef.current
    if (!bridge) return
    if (!force && lastShouldRunRef.current === shouldRun) return
    lastShouldRunRef.current = shouldRun
    bridge.emit(shouldRun ? 'lifecycle:resume' : 'lifecycle:pause', null)
  }

  const composedShouldRun = () => !pausedRef.current && pageVisibleRef.current

  // Reset tracking when the bridge/iframe is torn down.
  // Ready edge: only force-emit pause when shouldRun is false (remount while
  // minimized/hidden). When shouldRun is true, mark last without emitting
  // resume — running is the default until pause, and force-resume here would
  // double-start with onReady init.
  useEffect(() => {
    if (!isReady) {
      lastShouldRunRef.current = null
      return
    }
    const shouldRun = composedShouldRun()
    if (!shouldRun) {
      emitShouldRun(false, true)
    } else {
      lastShouldRunRef.current = true
    }
  }, [isReady, bridgeRef])

  // Host minimize / hide
  useEffect(() => {
    if (!isReady) return
    emitShouldRun(composedShouldRun())
  }, [paused, isReady, bridgeRef])

  // Document visibility
  useEffect(() => {
    return onVisibility((visible) => {
      pageVisibleRef.current = visible
      if (isReady && bridgeRef.current) {
        emitShouldRun(composedShouldRun())
      }
    })
  }, [isReady, bridgeRef])

  // 主题变化监听（仅当已授予 ui:theme:subscribe 时转发，见上）
  useEffect(() => {
    if (!isReady || !canSubscribeTheme) return
    return subscribeToTheme((isDark) => {
      const bridge = bridgeRef.current
      if (bridge) {
        bridge.emit('theme:change', isDark ? 'dark' : 'light')
      }
    })
  }, [isReady, canSubscribeTheme, bridgeRef])

  // 主色调变化监听（isReady 时立即发送当前颜色 + 订阅后续变化）
  useEffect(() => {
    if (!isReady || !canSubscribeTheme) return

    const bridge = bridgeRef.current
    const currentColor = getPrimaryColor()
    if (bridge && currentColor) {
      bridge.emit('primaryColor:change', currentColor)
    }

    return subscribeToPrimaryColor((color) => {
      const bridge = bridgeRef.current
      if (bridge && color) {
        bridge.emit('primaryColor:change', color)
      }
    })
  }, [isReady, canSubscribeTheme, bridgeRef])
}
