import type { TappBridge } from './TappBridge'
import { useEffect, useRef } from 'react'
import { isPageVisible, onVisibility } from '../../hooks/animation'

import {
  getPrimaryColor,
  subscribeToPrimaryColor,
} from '../../utils/colorSubscriber'
import { subscribeToTheme } from '../../utils/themeSubscriber'

/** shouldRun = !paused && isPageVisible()。单一发射器拥有 pause/resume，可见性与最小化不能互相覆盖。 */
export function useSandboxSubscriptions(
  bridgeRef: React.RefObject<TappBridge | null>,
  isReady: boolean,
  paused = false,
): void {
  const pageVisibleRef = useRef(isPageVisible())
  const pausedRef = useRef(paused)
  pausedRef.current = paused

  /** 当前 ready 周期上次发射的 shouldRun；null 表示尚未发射。 */
  const lastShouldRunRef = useRef<boolean | null>(null)

  const emitShouldRun = (shouldRun: boolean, force = false) => {
    const bridge = bridgeRef.current
    if (!bridge) return
    if (!force && lastShouldRunRef.current === shouldRun) return
    lastShouldRunRef.current = shouldRun
    bridge.emit(shouldRun ? 'lifecycle:resume' : 'lifecycle:pause', null)
  }

  const composedShouldRun = () => !pausedRef.current && pageVisibleRef.current

  // iframe 拆除时重置。ready 边沿：shouldRun 为 false 才强制 pause；为 true 只记账不发 resume，以免与 onReady 双启动。
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

  useEffect(() => {
    if (!isReady) return
    emitShouldRun(composedShouldRun())
  }, [paused, isReady, bridgeRef])

  useEffect(() => {
    return onVisibility((visible) => {
      pageVisibleRef.current = visible
      if (isReady && bridgeRef.current) {
        emitShouldRun(composedShouldRun())
      }
    })
  }, [isReady, bridgeRef])

  useEffect(() => {
    if (!isReady) return
    return subscribeToTheme((isDark) => {
      const bridge = bridgeRef.current
      if (bridge) {
        bridge.emit('theme:change', isDark ? 'dark' : 'light')
      }
    })
  }, [isReady, bridgeRef])

  useEffect(() => {
    if (!isReady) return

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
  }, [isReady, bridgeRef])
}
