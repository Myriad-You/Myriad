import type { WidgetSize } from '../components/widgetGridTypes'
import type { ViewportBand } from '../utils/viewportBands'
import type { WidgetSizeKey } from '../utils/widgetSizeScale'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { VIEWPORT_MQ } from '../utils/viewportBands'
import {
  resolveWidgetContentScale,
  STANDARD_CELL_BY_BAND,
  STANDARD_CELL_SIZE as STANDARD_CELL_SIZE_CONST,
  WIDGET_COMPACT_SCALE,
  WIDGET_MINI_SCALE,
} from '../utils/widgetSizeScale'
import { getCachedSize } from './animation'
import { useHomeResizeObserver } from './animation/pages/home'
import { isReducedAnimation, useAnimationLevel } from './useAnimationLevel'
import { useMediaQuery } from './useSharedEventListener'

export const STANDARD_CELL_SIZE = STANDARD_CELL_SIZE_CONST
export { STANDARD_CELL_BY_BAND, WIDGET_COMPACT_SCALE, WIDGET_MINI_SCALE }

export interface WidgetSizeInfo {
  scale: number
  fontScale: number
  width: number
  height: number
  isCompact: boolean
  isMini: boolean
  viewportBand: ViewportBand
  containerRef: React.RefCallback<HTMLDivElement>
}

function useViewportBand(): ViewportBand {
  const isPhone = useMediaQuery(VIEWPORT_MQ.phone)
  const isDesktop = useMediaQuery(VIEWPORT_MQ.desktop)
  return isPhone ? 'phone' : isDesktop ? 'desktop' : 'tablet'
}

/** 缩放基准与 viewportBands 一致，避免跨 1078 时 isCompact 反向跳变。 */
export function useWidgetSize(
  widgetSize?: WidgetSize,
  forceScale?: number,
): WidgetSizeInfo {
  const [size, setSize] = useState({ width: 0, height: 0 })
  const elementRef = useRef<HTMLDivElement | null>(null)
  const anim = useAnimationLevel()
  const viewportBand = useViewportBand()

  // 低性能 / 低端：仅首次测量，不持续监听。
  const reduceResizeWork = isReducedAnimation(anim)
  const reduceResizeWorkRef = useRef(reduceResizeWork)
  reduceResizeWorkRef.current = reduceResizeWork

  const { observeHomeResize, unobserveHomeResize } = useHomeResizeObserver()

  const handleSizeChange = useCallback((entry: ResizeObserverEntry) => {
    const { width, height } = entry.contentRect
    if (width <= 0) return

    setSize((prev) => {
      // 2px 忽略亚像素；更大的阈值硬切后会滞后换档。
      const THRESHOLD = 2
      if (
        Math.abs(prev.width - width) < THRESHOLD &&
        Math.abs(prev.height - height) < THRESHOLD
      ) {
        return prev
      }
      return { width, height }
    })
  }, [])

  const containerRef = useCallback(
    (node: HTMLDivElement | null) => {
      if (elementRef.current) {
        unobserveHomeResize(elementRef.current)
      }

      elementRef.current = node

      if (node) {
        if (reduceResizeWorkRef.current) {
          const cached = getCachedSize(node)
          if (cached && cached.width > 0) {
            setSize(cached)
          } else {
            requestAnimationFrame(() => {
              if (node.isConnected) {
                const rect = node.getBoundingClientRect()
                if (rect.width > 0) {
                  setSize({ width: rect.width, height: rect.height })
                }
              }
            })
          }
        } else {
          observeHomeResize(node, handleSizeChange)
        }
      }
    },
    [handleSizeChange, observeHomeResize, unobserveHomeResize],
  )

  useEffect(() => {
    return () => {
      if (elementRef.current) {
        unobserveHomeResize(elementRef.current)
      }
    }
  }, [unobserveHomeResize])

  useEffect(() => {
    if (
      reduceResizeWorkRef.current &&
      elementRef.current?.isConnected
    ) {
      requestAnimationFrame(() => {
        if (elementRef.current?.isConnected) {
          const rect = elementRef.current.getBoundingClientRect()
          if (rect.width > 0) {
            setSize({ width: rect.width, height: rect.height })
          }
        }
      })
    }
  }, [widgetSize])

  const scale = useMemo(() => {
    if (forceScale !== undefined) return forceScale
    if (!widgetSize || size.width === 0) return 1
    return resolveWidgetContentScale({
      measuredWidth: size.width,
      measuredHeight: size.height,
      widgetSize: widgetSize as WidgetSizeKey,
      band: viewportBand,
    })
  }, [forceScale, widgetSize, size.width, size.height, viewportBand])

  const fontScale = Math.sqrt(scale)
  const isCompact = scale < WIDGET_COMPACT_SCALE
  const isMini = scale < WIDGET_MINI_SCALE

  return {
    scale,
    fontScale,
    width: size.width,
    height: size.height,
    isCompact,
    isMini,
    viewportBand,
    containerRef,
  }
}
