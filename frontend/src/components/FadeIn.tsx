/**
 * FadeIn 组件 - 受页面动画调度的淡入动画
 *
 * 使用方式：
 * 1. 包裹需要淡入效果的组件
 * 2. 组件会在页面动画完成后自动淡入显示
 * 3. 可以通过 delay 属性设置额外延迟
 *
 * 特点：
 * - 自动等待页面级动画完成后再执行
 * - 支持交错动画（通过 staggerIndex）
 * - 使用 CSS 过渡实现平滑效果
 * - 🔧 性能优化：动画完成后释放资源
 * - 🔧 避免 DOM 节点跳跃：使用 visibility 配合 opacity
 */

import type { CSSProperties, ReactNode } from 'react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useElementAnimation } from '../hooks/animation'

interface FadeInProps {
  /** 子内容 */
  children: ReactNode
  /** 淡入延迟（毫秒） */
  delay?: number
  /** 动画持续时间（毫秒） */
  duration?: number
  /** 自定义类名 */
  className?: string
  /** 自定义样式 */
  style?: CSSProperties
  /** 是否禁用动画（直接显示） */
  disabled?: boolean
  /** 淡入方向：fade-仅透明度，up-从下往上，down-从上往下 */
  direction?: 'fade' | 'up' | 'down'
  /** 交错索引 - 用于计算交错延迟 */
  staggerIndex?: number
  /** 交错基础延迟（毫秒） */
  staggerDelay?: number
  /** 是否等待页面动画完成 */
  waitForPage?: boolean
  /** 分组ID */
  groupId?: string
  /** 🔧 新增：动画完成后是否保留占位（避免布局跳跃） */
  keepPlaceholder?: boolean
  /** 🔧 新增：预估高度（用于动画前占位，避免布局跳跃） */
  estimatedHeight?: number | string
}

/** 动画阶段枚举 - 用于精确控制资源分配 */
enum AnimationPhase {
  /** 等待中 - 尚未进入动画 */
  WAITING = 'waiting',
  /** 进行中 - 正在播放动画 */
  ANIMATING = 'animating',
  /** 已完成 - 动画结束，释放资源 */
  COMPLETED = 'completed',
}

/**
 * 单组件淡入包装器
 * 组件挂载后自动平滑淡入，遵循协调器调度
 *
 * 🔧 性能优化：
 * - 动画完成后移除 willChange 和 transition，释放 GPU 层
 * - 使用 visibility: hidden 替代 opacity: 0 避免空间占用问题
 * - 支持预估高度，避免布局跳跃
 */
export default function FadeIn({
  children,
  delay = 0,
  duration = 350,
  className = '',
  style,
  disabled = false,
  direction = 'fade',
  staggerIndex = 0,
  staggerDelay = 50,
  waitForPage = true,
  groupId = 'fadein',
  keepPlaceholder = true,
  estimatedHeight,
}: FadeInProps) {
  // 使用新的动画协调系统
  const { canAnimate, onComplete } = useElementAnimation({
    groupId,
    index: staggerIndex,
    staggerDelay,
    waitForPage: !disabled && waitForPage,
  })

  // 🔧 优化：使用动画阶段替代简单的 isVisible 状态
  const [phase, setPhase] = useState<AnimationPhase>(
    disabled ? AnimationPhase.COMPLETED : AnimationPhase.WAITING,
  )

  const ref = useRef<HTMLDivElement>(null)
  const mountedRef = useRef(true)
  const hasCompletedRef = useRef(false)
  const animationStartTimeRef = useRef<number>(0)

  // 🔧 计算是否可见（用于样式计算）
  const isVisible = phase !== AnimationPhase.WAITING
  const isAnimationComplete = phase === AnimationPhase.COMPLETED

  // 协调器通知可以开始动画后，再应用额外延迟
  useEffect(() => {
    mountedRef.current = true

    if (disabled) {
      setPhase(AnimationPhase.COMPLETED)
      return
    }

    if (!canAnimate)
      return

    // 只应用额外的 delay（交错延迟已由协调器处理）
    if (delay > 0) {
      const timeoutId = window.setTimeout(() => {
        if (mountedRef.current) {
          animationStartTimeRef.current = performance.now()
          setPhase(AnimationPhase.ANIMATING)
        }
      }, delay)

      return () => {
        clearTimeout(timeoutId)
      }
    }
    else {
      animationStartTimeRef.current = performance.now()
      setPhase(AnimationPhase.ANIMATING)
    }

    return () => {
      mountedRef.current = false
    }
  }, [delay, disabled, canAnimate])

  // 🔧 优化：使用 transitionend 事件检测动画完成
  const handleTransitionEnd = useCallback((e: React.TransitionEvent) => {
    // 只响应 opacity 过渡（避免多次触发）
    if (e.propertyName !== 'opacity')
      return
    if (hasCompletedRef.current)
      return

    hasCompletedRef.current = true
    setPhase(AnimationPhase.COMPLETED)
    onComplete()
  }, [onComplete])

  // 🔧 备用：定时器检测动画完成（防止 transitionend 不触发）
  useEffect(() => {
    if (hasCompletedRef.current)
      return

    if (disabled) {
      hasCompletedRef.current = true
      onComplete()
      return
    }

    if (phase !== AnimationPhase.ANIMATING) {
      return
    }

    // 🔧 优化：计算剩余时间，避免固定等待
    const elapsed = performance.now() - animationStartTimeRef.current
    const remaining = Math.max(0, duration - elapsed + 50) // 额外 50ms 缓冲

    const timer = window.setTimeout(() => {
      if (!hasCompletedRef.current && mountedRef.current) {
        hasCompletedRef.current = true
        setPhase(AnimationPhase.COMPLETED)
        onComplete()
      }
    }, remaining)

    return () => {
      window.clearTimeout(timer)
    }
  }, [phase, duration, disabled, onComplete])

  // 🔧 优化：使用 useMemo 缓存 transform 计算
  const transform = useMemo(() => {
    if (disabled || isAnimationComplete)
      return 'none'

    switch (direction) {
      case 'up':
        return isVisible ? 'translateY(0)' : 'translateY(12px)'
      case 'down':
        return isVisible ? 'translateY(0)' : 'translateY(-12px)'
      default:
        return 'none'
    }
  }, [disabled, isAnimationComplete, direction, isVisible])

  // 🔧 优化：计算样式，动画完成后释放资源
  const computedStyle = useMemo((): CSSProperties => {
    const baseStyle: CSSProperties = {}

    // 动画完成后，完全移除动画相关属性
    if (isAnimationComplete) {
      return {
        opacity: 1,
        // 🔧 不设置 transform、transition、willChange，让浏览器回收 GPU 层
        ...style,
      }
    }

    // 等待阶段：使用 visibility 配合 opacity 避免布局问题
    if (phase === AnimationPhase.WAITING) {
      baseStyle.opacity = 0
      baseStyle.visibility = keepPlaceholder ? 'visible' : 'hidden'
      baseStyle.transform = direction === 'up'
        ? 'translateY(12px)'
        : direction === 'down'
          ? 'translateY(-12px)'
          : 'none'

      // 🔧 如果提供了预估高度，设置最小高度避免布局跳跃
      if (estimatedHeight !== undefined) {
        baseStyle.minHeight = typeof estimatedHeight === 'number'
          ? `${estimatedHeight}px`
          : estimatedHeight
      }

      // 等待阶段不设置 willChange，节省资源
      return { ...baseStyle, ...style }
    }

    // 动画进行中
    baseStyle.opacity = 1
    baseStyle.visibility = 'visible'
    baseStyle.transform = transform
    baseStyle.transition = `opacity ${duration}ms cubic-bezier(0.22, 1, 0.36, 1), transform ${duration}ms cubic-bezier(0.22, 1, 0.36, 1)`
    baseStyle.willChange = 'opacity, transform'

    return { ...baseStyle, ...style }
  }, [phase, isAnimationComplete, transform, duration, direction, keepPlaceholder, estimatedHeight, style])

  return (
    <div
      ref={ref}
      className={className}
      style={computedStyle}
      onTransitionEnd={phase === AnimationPhase.ANIMATING ? handleTransitionEnd : undefined}
    >
      {children}
    </div>
  )
}

// 向后兼容导出
export { resetPageAnimationState } from '../hooks/usePageReady'

/** 动画状态类型（用于 FadeInWithSkeleton） */
enum SkeletonPhase {
  /** 显示骨架屏 */
  SKELETON = 'skeleton',
  /** 过渡到内容 */
  TRANSITIONING = 'transitioning',
  /** 显示内容，动画进行中 */
  CONTENT_ANIMATING = 'content-animating',
  /** 完成，释放资源 */
  COMPLETED = 'completed',
}

/**
 * 带骨架屏的 FadeIn
 * 在内容加载前显示骨架屏，加载后平滑切换
 *
 * 🔧 性能优化：
 * - 动画完成后彻底移除骨架屏 DOM
 * - 释放 GPU 层和过渡资源
 * - 使用 CSS contain 属性优化渲染
 */
interface FadeInWithSkeletonProps extends Omit<FadeInProps, 'disabled'> {
  /** 是否正在加载 */
  loading: boolean
  /** 骨架屏内容 */
  skeleton: ReactNode
  /** 最小骨架屏显示时间（避免闪烁） */
  minSkeletonTime?: number
}

export function FadeInWithSkeleton({
  children,
  loading,
  skeleton,
  delay = 0,
  duration = 350,
  minSkeletonTime = 200,
  className = '',
  style,
  direction = 'fade',
  staggerIndex = 0,
  staggerDelay = 50,
  waitForPage = true,
  groupId = 'fadein-skeleton',
  keepPlaceholder = true,
  estimatedHeight,
}: FadeInWithSkeletonProps & { groupId?: string, keepPlaceholder?: boolean, estimatedHeight?: number | string }) {
  // 使用新的动画协调系统
  const { canAnimate, onComplete } = useElementAnimation({
    groupId,
    index: staggerIndex,
    staggerDelay,
    waitForPage,
  })

  // 🔧 优化：使用阶段状态替代多个布尔值
  const [phase, setPhase] = useState<SkeletonPhase>(
    loading ? SkeletonPhase.SKELETON : SkeletonPhase.COMPLETED,
  )

  const loadStartRef = useRef(Date.now())
  const mountedRef = useRef(true)
  const hasCompletedRef = useRef(false)
  const contentRef = useRef<HTMLDivElement>(null)

  // 计算派生状态
  const showSkeleton = phase === SkeletonPhase.SKELETON || phase === SkeletonPhase.TRANSITIONING
  const showContent = phase !== SkeletonPhase.SKELETON
  const isContentVisible = phase === SkeletonPhase.CONTENT_ANIMATING || phase === SkeletonPhase.COMPLETED
  const isComplete = phase === SkeletonPhase.COMPLETED

  useEffect(() => {
    mountedRef.current = true

    if (loading) {
      loadStartRef.current = Date.now()
      setPhase(SkeletonPhase.SKELETON)
      hasCompletedRef.current = false
    }
    else if (canAnimate) {
      // 确保骨架屏至少显示 minSkeletonTime
      const elapsed = Date.now() - loadStartRef.current
      const remaining = Math.max(0, minSkeletonTime - elapsed)

      const timer1 = window.setTimeout(() => {
        if (!mountedRef.current)
          return

        // 开始过渡
        setPhase(SkeletonPhase.TRANSITIONING)

        // 等待一帧后显示内容
        requestAnimationFrame(() => {
          const timer2 = window.setTimeout(() => {
            if (mountedRef.current) {
              setPhase(SkeletonPhase.CONTENT_ANIMATING)
            }
          }, delay)
          return () => clearTimeout(timer2)
        })
      }, remaining)

      return () => {
        clearTimeout(timer1)
      }
    }

    return () => {
      mountedRef.current = false
    }
  }, [loading, delay, minSkeletonTime, canAnimate])

  // 🔧 优化：使用 transitionend 检测内容动画完成
  const handleContentTransitionEnd = useCallback((e: React.TransitionEvent) => {
    if (e.propertyName !== 'opacity')
      return
    if (hasCompletedRef.current)
      return
    if (phase !== SkeletonPhase.CONTENT_ANIMATING)
      return

    hasCompletedRef.current = true
    setPhase(SkeletonPhase.COMPLETED)
    onComplete()
  }, [phase, onComplete])

  // 备用定时器
  useEffect(() => {
    if (hasCompletedRef.current)
      return
    if (phase !== SkeletonPhase.CONTENT_ANIMATING)
      return

    const timer = window.setTimeout(() => {
      if (!hasCompletedRef.current && mountedRef.current) {
        hasCompletedRef.current = true
        setPhase(SkeletonPhase.COMPLETED)
        onComplete()
      }
    }, duration + 50)

    return () => {
      window.clearTimeout(timer)
    }
  }, [phase, duration, onComplete])

  // 🔧 优化：缓存 transform 计算
  const contentTransform = useMemo(() => {
    if (isComplete)
      return 'none'

    switch (direction) {
      case 'up':
        return isContentVisible ? 'translateY(0)' : 'translateY(12px)'
      case 'down':
        return isContentVisible ? 'translateY(0)' : 'translateY(-12px)'
      default:
        return 'none'
    }
  }, [isComplete, direction, isContentVisible])

  // 🔧 优化：计算容器样式
  const containerStyle = useMemo((): CSSProperties => {
    const base: CSSProperties = {
      position: 'relative',
      // 🔧 使用 contain 优化渲染性能
      contain: isComplete ? 'none' : 'layout',
    }

    if (estimatedHeight !== undefined && !isComplete) {
      base.minHeight = typeof estimatedHeight === 'number'
        ? `${estimatedHeight}px`
        : estimatedHeight
    }

    return { ...base, ...style }
  }, [isComplete, estimatedHeight, style])

  // 🔧 优化：骨架屏样式
  const skeletonStyle = useMemo((): CSSProperties => {
    if (isComplete)
      return {} // 完成后不渲染骨架屏

    return {
      position: showContent ? 'absolute' : 'relative',
      inset: 0,
      opacity: showContent ? 0 : 1,
      transition: `opacity ${duration}ms cubic-bezier(0.22, 1, 0.36, 1)`,
      pointerEvents: showContent ? 'none' : 'auto',
      // 🔧 过渡期间使用 willChange
      willChange: phase === SkeletonPhase.TRANSITIONING ? 'opacity' : 'auto',
    }
  }, [isComplete, showContent, duration, phase])

  // 🔧 优化：内容样式，完成后释放资源
  const contentStyle = useMemo((): CSSProperties => {
    if (isComplete) {
      // 完成后移除所有动画相关属性
      return {
        opacity: 1,
      }
    }

    return {
      opacity: isContentVisible ? 1 : 0,
      transform: contentTransform,
      transition: `opacity ${duration}ms cubic-bezier(0.22, 1, 0.36, 1), transform ${duration}ms cubic-bezier(0.22, 1, 0.36, 1)`,
      willChange: 'opacity, transform',
    }
  }, [isComplete, isContentVisible, contentTransform, duration])

  return (
    <div className={`${className}`} style={containerStyle}>
      {/* 🔧 优化：动画完成后不渲染骨架屏，彻底释放资源 */}
      {!isComplete && showSkeleton && (
        <div style={skeletonStyle}>
          {skeleton}
        </div>
      )}

      {/* 内容层 */}
      {showContent && (
        <div
          ref={contentRef}
          style={contentStyle}
          onTransitionEnd={phase === SkeletonPhase.CONTENT_ANIMATING ? handleContentTransitionEnd : undefined}
        >
          {children}
        </div>
      )}
    </div>
  )
}
