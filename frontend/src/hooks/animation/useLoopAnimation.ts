/**
 * 循环动画 Hook
 *
 * 用于管理触发式循环动画（如弹幕、音乐气泡等）
 * 每次触发播放一轮，播放完成后自动释放资源，不自动重播
 *
 * 使用方式：
 * - 传入 trigger 参数，当 trigger 变化时触发一轮动画
 * - 动画播放完成后自动停止，不占用任何资源
 * - 下次 trigger 变化时再次播放
 */

import { useCallback, useEffect, useRef, useState } from 'react'

interface UseLoopAnimationOptions {
  /** 动画持续时间(ms) */
  duration?: number
  /** 触发器：值变化时触发一轮动画 */
  trigger?: unknown
  /** 是否启用（可用于暂停功能） */
  enabled?: boolean
}

interface UseLoopAnimationResult {
  /** 是否正在播放动画 */
  isAnimating: boolean
  /** 手动触发一轮动画 */
  triggerAnimation: () => void
  /** 手动停止动画 */
  stopAnimation: () => void
}

/**
 * 触发式循环动画 Hook
 *
 * @example
 * ```tsx
 * // 报告卡片弹幕动画 - 状态切换时触发
 * const [currentIndex, setCurrentIndex] = useState(0);
 * const { isAnimating } = useLoopAnimation({
 *   duration: 11000,
 *   trigger: currentIndex, // 切换时触发
 *   enabled: anim.loop,
 * });
 *
 * // 手动触发
 * const { isAnimating, triggerAnimation } = useLoopAnimation({ duration: 5000 });
 * <button onClick={triggerAnimation}>播放动画</button>
 * ```
 */
export function useLoopAnimation(options: UseLoopAnimationOptions = {}): UseLoopAnimationResult {
  const {
    duration = 3000,
    trigger,
    enabled = true,
  } = options

  const [isAnimating, setIsAnimating] = useState(false)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const mountedRef = useRef(true)
  const isFirstRender = useRef(true)

  // 清理定时器
  const clearTimer = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current)
      timerRef.current = null
    }
  }, [])

  // 开始一轮动画
  const startAnimation = useCallback(() => {
    if (!mountedRef.current || !enabled)
      return

    clearTimer()
    setIsAnimating(true)

    // 动画结束后自动停止
    timerRef.current = setTimeout(() => {
      if (mountedRef.current) {
        setIsAnimating(false)
      }
      timerRef.current = null
    }, duration)
  }, [duration, enabled, clearTimer])

  // 手动触发
  const triggerAnimation = useCallback(() => {
    startAnimation()
  }, [startAnimation])

  // 手动停止
  const stopAnimation = useCallback(() => {
    clearTimer()
    setIsAnimating(false)
  }, [clearTimer])

  // trigger 变化时触发动画（跳过首次渲染）
  useEffect(() => {
    if (isFirstRender.current) {
      isFirstRender.current = false
      // 首次渲染时也触发一次（如果启用）
      if (enabled && trigger !== undefined) {
        startAnimation()
      }
      return
    }

    if (enabled && trigger !== undefined) {
      startAnimation()
    }
  }, [trigger, enabled, startAnimation])

  // 清理
  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
      clearTimer()
    }
  }, [clearTimer])

  return {
    isAnimating,
    triggerAnimation,
    stopAnimation,
  }
}

export default useLoopAnimation
