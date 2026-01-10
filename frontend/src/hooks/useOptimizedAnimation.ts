/**
 * Framer Motion 动画优化工具
 *
 * 提供按需动画、视口检测、性能优化等功能
 */

import type { TargetAndTransition, Transition } from 'framer-motion'
import type { AnimationConfig } from './useAnimationLevel'
import { useMemo } from 'react'
import { useAnimationLevel } from './useAnimationLevel'

/**
 * 根据性能等级返回优化后的过渡配置
 */
export function useOptimizedTransition(
  baseTransition: Transition = {},
): Transition {
  const { level, durationScale } = useAnimationLevel()

  return useMemo(() => {
    if (level === 'none') {
      return { duration: 0 }
    }

    const duration = typeof baseTransition.duration === 'number'
      ? baseTransition.duration * durationScale
      : undefined

    // 低端设备禁用弹簧动画
    if (level === 'light' && baseTransition.type === 'spring') {
      return {
        ...baseTransition,
        type: 'tween',
        duration: duration ?? 0.2,
      }
    }

    return {
      ...baseTransition,
      duration,
    }
  }, [baseTransition, level, durationScale])
}

/**
 * 创建按需动画的 animate 属性
 * 只在需要时才返回动画配置
 */
export function useConditionalAnimate(
  shouldAnimate: boolean,
  animate: TargetAndTransition,
  fallback: TargetAndTransition = {},
): TargetAndTransition {
  const { level } = useAnimationLevel()

  return useMemo(() => {
    if (level === 'none') {
      return fallback
    }
    return shouldAnimate ? animate : fallback
  }, [shouldAnimate, animate, fallback, level])
}

/**
 * 优化后的循环动画配置
 * 在低端设备或 reduce motion 时禁用循环
 */
export function useLoopAnimation(
  animation: TargetAndTransition,
  transition: Transition & { repeat?: number },
): { animate: TargetAndTransition, transition: Transition } {
  const { loop, durationScale } = useAnimationLevel()

  return useMemo(() => {
    if (!loop) {
      // 禁用循环，执行一次后停止
      return {
        animate: animation,
        transition: {
          ...transition,
          repeat: 0,
          duration: typeof transition.duration === 'number'
            ? transition.duration * durationScale
            : undefined,
        },
      }
    }

    return { animate: animation, transition }
  }, [animation, transition, loop, durationScale])
}

/**
 * 静态动画配置 - 禁用所有动画
 */
export const STATIC_ANIMATE: TargetAndTransition = {}
export const STATIC_TRANSITION: Transition = { duration: 0 }

/**
 * 快速淡入配置
 */
export const FADE_IN_FAST: TargetAndTransition = { opacity: 1 }
export const FADE_IN_FAST_TRANSITION: Transition = { duration: 0.15 }

/**
 * 获取动画配置的工具函数
 */
export function getAnimationConfig(config: AnimationConfig) {
  return {
    /** 是否应该循环 */
    shouldLoop: config.loop,
    /** 是否应该使用弹簧 */
    shouldSpring: config.spring,
    /** 持续时间缩放 */
    scale: config.durationScale,
    /** 快速判断是否禁用动画 */
    isDisabled: config.level === 'none',
    /** 是否为低端设备 */
    isLowEnd: config.level === 'light',
  }
}
