import { useContext, useEffect, useMemo } from 'react'
import { AnimationPreferenceContext } from '../contexts/AnimationPreferenceContext'
import { configureAnimationCoordinator } from './animation'
import { getPerformanceProfileSync, usePerformanceProfile } from './usePerformanceProfile'

export type AnimationLevel = 'none' | 'light' | 'standard'

export interface AnimationConfig {
  level: AnimationLevel
  // helpers
  loop: boolean // allow infinite loops
  spring: boolean // allow spring physics
  durationScale: number // multiply base duration
}

/**
 * 同步获取动画配置（用于模块初始化时）
 * 注意：无法获取用户手动设置的偏好（需要 Context），仅用于首屏渲染
 */
export function getAnimationConfigSync(): AnimationConfig {
  const perf = getPerformanceProfileSync()

  // prefers-reduced-motion 优先
  if (perf.reduceMotion) {
    return { level: 'none', loop: false, spring: false, durationScale: 0.0 }
  }

  // 低端设备
  if (perf.lowEndDevice) {
    return { level: 'light', loop: false, spring: false, durationScale: 0.6 }
  }

  // 标准设备
  return { level: 'standard', loop: true, spring: true, durationScale: 1.0 }
}

export function useAnimationLevel(): AnimationConfig {
  const perf = usePerformanceProfile()
  const prefContext = useContext(AnimationPreferenceContext)

  const config = useMemo(() => {
    // prefers-reduced-motion 优先（无法被手动覆盖）
    if (perf.reduceMotion) {
      return { level: 'none' as const, loop: false, spring: false, durationScale: 0.0 }
    }

    // 如果有手动设置的偏好，使用手动偏好
    if (prefContext?.preference && prefContext.preference !== 'auto') {
      if (prefContext.preference === 'light') {
        return { level: 'light' as const, loop: false, spring: false, durationScale: 0.6 }
      }
      else if (prefContext.preference === 'standard') {
        return { level: 'standard' as const, loop: true, spring: true, durationScale: 1.0 }
      }
    }

    // 自动检测：低端设备
    if (perf.lowEndDevice) {
      return { level: 'light' as const, loop: false, spring: false, durationScale: 0.6 }
    }
    // 自动检测：标准设备
    return { level: 'standard' as const, loop: true, spring: true, durationScale: 1.0 }
  }, [perf.reduceMotion, perf.lowEndDevice, prefContext?.preference])

  // 根据性能级别自动配置动画协调器
  useEffect(() => {
    const isMobile = perf.isMobile

    switch (config.level) {
      case 'none':
        // 完全禁用动画时，最小化并发
        configureAnimationCoordinator({
          baseConcurrent: 4,
          burstConcurrent: 8,
          burstDuration: 3000,
          maxLoopSlots: 2,
        })
        break
      case 'light':
        // 低端设备，限制同时动画数量
        configureAnimationCoordinator({
          baseConcurrent: isMobile ? 6 : 10,
          burstConcurrent: isMobile ? 16 : 24,
          burstDuration: 6000,
          maxLoopSlots: isMobile ? 4 : 6,
        })
        break
      case 'standard':
        // 标准/高性能设备，允许更多并发动画
        // 🔥 提高并发限制以充分利用高端 GPU
        configureAnimationCoordinator({
          baseConcurrent: isMobile ? 12 : 20,
          burstConcurrent: isMobile ? 32 : 64,
          burstDuration: 10000,
          maxLoopSlots: isMobile ? 8 : 16,
        })
        break
    }
  }, [config.level, perf.isMobile])

  return config
}
