/**
 * 小组件响应式尺寸适配 Hook
 *
 * 基于组件实际尺寸自动调整内容显示,支持:
 * - 标准尺寸 (100%)
 * - 紧凑尺寸 (66% - 2/3)
 * - 迷你尺寸 (50% - 1/2)
 *
 * 性能优化：
 * - 使用共享 ResizeObserver（通过 AnimationCoordinator）
 * - 自动节流和尺寸变化阈值过滤
 * - 页面不可见时暂停监测
 * - 低端设备仅首次测量
 *
 * @example
 * const { scale, isCompact, isMini, containerRef } = useWidgetSize();
 *
 * // 使用scale动态调整字体/间距
 * <div ref={containerRef} style={{ fontSize: `${14 * scale}px` }}>
 *
 * // 或使用布尔值条件渲染
 * {!isCompact && <DetailedContent />}
 * {isCompact && <CompactContent />}
 */

import type { WidgetSize } from '../components/WidgetGrid'
import { useCallback, useEffect, useRef, useState } from 'react'
import { getCachedSize } from './animation'
import { useHomeResizeObserver } from './animation/pages/home'
import { usePerformanceProfile } from './usePerformanceProfile'

// 标准尺寸映射 (像素值基于假设的标准单元格大小)
// 调整基准：从 120px 降至 80px，以适应 1366px/1440px 等主流笔记本屏幕
// 在 1920px 屏幕上，单元格约 110px，scale 会被限制在 1
// 在 1366px 屏幕上，单元格约 75px，scale 约 0.93，接近 1
const STANDARD_DIMENSIONS: Record<WidgetSize, { width: number, height: number }> = {
  '1x1': { width: 80, height: 80 },
  '2x1': { width: 160, height: 80 },
  '4x1': { width: 320, height: 80 },
  '1x2': { width: 80, height: 160 },
  '2x2': { width: 160, height: 160 },
  '2x4': { width: 160, height: 320 },
  '4x2': { width: 320, height: 160 },
  '4x4': { width: 320, height: 320 },
}

export interface WidgetSizeInfo {
  /** 缩放比例 0-1, 1为标准尺寸 */
  scale: number
  /** 字体缩放比例 (比几何缩放更平缓) */
  fontScale: number
  /** 实际宽度(px) */
  width: number
  /** 实际高度(px) */
  height: number
  /** 是否为紧凑模式 (60%-85%) */
  isCompact: boolean
  /** 是否为迷你模式 (<60%) */
  isMini: boolean
  /** 容器ref,必须绑定到组件根元素 */
  containerRef: React.RefCallback<HTMLDivElement>
}

export function useWidgetSize(widgetSize?: WidgetSize, forceScale?: number): WidgetSizeInfo {
  const [size, setSize] = useState({ width: 0, height: 0 })
  const elementRef = useRef<HTMLDivElement | null>(null)
  const perf = usePerformanceProfile()
  const lowEndDeviceRef = useRef(perf.lowEndDevice)
  lowEndDeviceRef.current = perf.lowEndDevice

  // 🆕 使用首页原子化 ResizeObserver
  const { observeHomeResize, unobserveHomeResize } = useHomeResizeObserver()

  // 尺寸更新处理
  const handleSizeChange = useCallback((entry: ResizeObserverEntry) => {
    const { width, height } = entry.contentRect

    // 宽度为0时不更新（可能是隐藏或未渲染）
    if (width <= 0)
      return

    setSize((prev) => {
      // 使用较大的阈值避免微小变化触发重渲染
      const THRESHOLD = 8
      if (Math.abs(prev.width - width) < THRESHOLD && Math.abs(prev.height - height) < THRESHOLD) {
        return prev
      }
      return { width, height }
    })
  }, [])

  // Ref callback - 连接到首页原子化 ResizeObserver
  const containerRef = useCallback((node: HTMLDivElement | null) => {
    // 清理旧观察
    if (elementRef.current) {
      unobserveHomeResize(elementRef.current)
    }

    elementRef.current = node

    if (node) {
      // 低端设备：仅首次测量，不持续监听
      if (lowEndDeviceRef.current) {
        // 尝试获取缓存尺寸
        const cached = getCachedSize(node)
        if (cached && cached.width > 0) {
          setSize(cached)
        }
        else {
          // 延迟测量一次
          requestAnimationFrame(() => {
            if (node.isConnected) {
              const rect = node.getBoundingClientRect()
              if (rect.width > 0) {
                setSize({ width: rect.width, height: rect.height })
              }
            }
          })
        }
      }
      else {
        // 正常设备：使用首页原子化 ResizeObserver 持续监听
        observeHomeResize(node, handleSizeChange)
      }
    }
  }, [handleSizeChange, observeHomeResize, unobserveHomeResize])

  // 组件卸载时清理
  useEffect(() => {
    return () => {
      if (elementRef.current) {
        unobserveHomeResize(elementRef.current)
      }
    }
  }, [unobserveHomeResize])

  // widgetSize 变化时重新测量（针对低端设备）
  useEffect(() => {
    if (lowEndDeviceRef.current && elementRef.current && elementRef.current.isConnected) {
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

  // 计算缩放比例
  const scale = (() => {
    if (forceScale !== undefined)
      return forceScale
    if (!widgetSize || size.width === 0)
      return 1

    const standard = STANDARD_DIMENSIONS[widgetSize]
    if (!standard)
      return 1

    // 基于宽度计算缩放比例
    const calculatedScale = size.width / standard.width

    // 限制在 0.5 - 1.1 之间，避免过大或过小
    return Math.max(0.5, Math.min(1.1, calculatedScale))
  })()

  // 字体缩放比例：使用平方根使缩放更平缓
  // 例如：scale = 0.64 -> fontScale = 0.8
  const fontScale = Math.sqrt(scale)

  // 判断模式
  const isCompact = scale < 0.85
  const isMini = scale < 0.65

  return {
    scale,
    fontScale,
    width: size.width,
    height: size.height,
    isCompact,
    isMini,
    containerRef,
  }
}

/**
 * 简化版 - 只返回缩放比例
 */
export function useWidgetScale(widgetSize?: WidgetSize): number {
  const { scale } = useWidgetSize(widgetSize)
  return scale
}

/**
 * 布尔版 - 只判断是否为紧凑/迷你模式
 */
export function useWidgetMode(widgetSize?: WidgetSize): { isCompact: boolean, isMini: boolean } {
  const { isCompact, isMini } = useWidgetSize(widgetSize)
  return { isCompact, isMini }
}
