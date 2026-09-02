/**
 * 小组件响应式尺寸适配 Hook
 *
 * 缩放基准与 `utils/viewportBands` 一致：
 * - phone / tablet / desktop 各有「该档位下格子看起来正常」的 cell 设计尺寸
 * - 避免桌面 16 列（格小）与平板 8 列（格大）共用 80px 基准导致
 *   跨 1078 时 isCompact 反向跳变
 */

import type { WidgetSize } from '../components/WidgetGrid'
import type { ViewportBand } from '../utils/viewportBands'
import type { WidgetSizeKey } from '../utils/widgetSizeScale'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { VIEWPORT_MQ } from '../utils/viewportBands'
import {
  getStandardWidgetDimensions as getStandardWidgetDimensionsPure,
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
export {
  getStandardWidgetDimensionsForBand,
  resolveWidgetContentScale,
  standardCellSizeForBand,
  WIDGET_SCALE_MAX,
  WIDGET_SCALE_MIN,
} from '../utils/widgetSizeScale'

export function getStandardWidgetDimensions(widgetSize: WidgetSize): {
  width: number
  height: number
} {
  return getStandardWidgetDimensionsPure(widgetSize as WidgetSizeKey)
}

/**
 * Library strip only: shrink the already-standard-sized preview so many
 * widgets fit. Content still renders at STANDARD_CELL_SIZE (forceScale=1).
 */
export const LIBRARY_PREVIEW_DISPLAY_SCALE = 0.65

export interface WidgetSizeInfo {
  /** 缩放比例（相对当前 viewport 档设计尺寸） */
  scale: number
  /** 字体缩放比例 (比几何缩放更平缓) */
  fontScale: number
  /** 实际宽度(px) */
  width: number
  /** 实际高度(px) */
  height: number
  /** 是否为紧凑模式（相对该档设计明显偏小） */
  isCompact: boolean
  /** 是否为迷你模式 */
  isMini: boolean
  /** 当前 viewport 档（与主页网格断点一致） */
  viewportBand: ViewportBand
  /** 容器ref,必须绑定到组件根元素 */
  containerRef: React.RefCallback<HTMLDivElement>
}

function useViewportBand(): ViewportBand {
  const isPhone = useMediaQuery(VIEWPORT_MQ.phone)
  const isDesktop = useMediaQuery(VIEWPORT_MQ.desktop)
  return isPhone ? 'phone' : isDesktop ? 'desktop' : 'tablet'
}

export function useWidgetSize(
  widgetSize?: WidgetSize,
  forceScale?: number,
): WidgetSizeInfo {
  const [size, setSize] = useState({ width: 0, height: 0 })
  const elementRef = useRef<HTMLDivElement | null>(null)
  const anim = useAnimationLevel()
  const viewportBand = useViewportBand()
  // 低性能模式与硬件低端：仅首次测量，不持续监听
  const reduceResizeWork = isReducedAnimation(anim)
  const reduceResizeWorkRef = useRef(reduceResizeWork)
  reduceResizeWorkRef.current = reduceResizeWork

  const { observeHomeResize, unobserveHomeResize } = useHomeResizeObserver()

  const handleSizeChange = useCallback((entry: ResizeObserverEntry) => {
    const { width, height } = entry.contentRect
    if (width <= 0) return

    setSize((prev) => {
      // 2px: ignore subpixel noise; was 8px and could lag mode after hard-cut.
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
      elementRef.current &&
      elementRef.current.isConnected
    ) {
      requestAnimationFrame(() => {
        if (elementRef.current && elementRef.current.isConnected) {
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
