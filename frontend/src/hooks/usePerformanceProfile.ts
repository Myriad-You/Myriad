import { useEffect, useRef, useState } from 'react'

/**
 * 设备性能画像与动态特性检测
 * - 用于在移动端 / 低性能设备 / 降低动效场景下自动降级动画与计算频率
 */
export interface PerformanceProfile {
  isMobile: boolean
  reduceMotion: boolean
  lowEndDevice: boolean
  hardwareConcurrency: number | null
  deviceMemory: number | null
}

// SSR 安全的默认值 - 乐观策略：假设为中高端设备
const DEFAULT_PROFILE: PerformanceProfile = {
  isMobile: false,
  reduceMotion: false,
  lowEndDevice: false,
  hardwareConcurrency: null,
  deviceMemory: null,
}

// 🔧 性能优化：全局缓存检测结果，避免重复检测
let cachedProfile: PerformanceProfile | null = null
let hasDetected = false

function detectPerformanceProfile(): PerformanceProfile {
  // 🔧 优化：如果已经检测过，直接返回缓存
  if (hasDetected && cachedProfile) {
    return cachedProfile
  }

  // 每次调用时检测浏览器环境
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
    return DEFAULT_PROFILE
  }

  try {
    const isMobile = window.matchMedia('(hover: none) and (pointer: coarse)').matches
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const hardwareConcurrency = (navigator as any).hardwareConcurrency ?? null
    const deviceMemory = (navigator as any).deviceMemory ?? null

    // 🔧 极简判定逻辑
    // 移动端：全部判定为中高端，现代手机性能都足够
    // 只有用户主动开启 reduceMotion 才降级
    let lowEndDevice = reduceMotion

    // 桌面端：根据硬件判断
    if (!lowEndDevice && !isMobile) {
      lowEndDevice = (
        (hardwareConcurrency !== null && hardwareConcurrency <= 4)
        || (deviceMemory !== null && deviceMemory <= 4)
      )
    }

    const profile = { isMobile, reduceMotion, lowEndDevice, hardwareConcurrency, deviceMemory }

    // 🔧 缓存结果
    cachedProfile = profile
    hasDetected = true

    return profile
  }
  catch (e) {
    console.warn('Failed to detect performance profile:', e)
    return DEFAULT_PROFILE
  }
}

/**
 * 同步获取性能配置（用于模块初始化时，非 React 上下文）
 * 返回当前检测到的设备性能画像
 */
export function getPerformanceProfileSync(): PerformanceProfile {
  return detectPerformanceProfile()
}

export function usePerformanceProfile(): PerformanceProfile {
  // 🔧 SSR 安全：始终使用默认值作为初始状态，避免 hydration 不匹配
  const [profile, setProfile] = useState<PerformanceProfile>(DEFAULT_PROFILE)
  const hasInitialized = useRef(false)

  // 客户端初始化：在组件挂载后检测真实性能配置
  useEffect(() => {
    if (hasInitialized.current)
      return
    hasInitialized.current = true

    const detected = detectPerformanceProfile()
    // 只有在检测结果与默认值不同时才更新，避免不必要的重渲染
    if (detected.isMobile !== DEFAULT_PROFILE.isMobile
      || detected.reduceMotion !== DEFAULT_PROFILE.reduceMotion
      || detected.lowEndDevice !== DEFAULT_PROFILE.lowEndDevice) {
      setProfile(detected)
    }
  }, [])

  // 监听 reduceMotion 变化
  useEffect(() => {
    if (typeof window === 'undefined')
      return

    const mediaQuery = window.matchMedia('(prefers-reduced-motion: reduce)')
    const handler = () => {
      // 🔧 重置缓存，重新检测
      hasDetected = false
      cachedProfile = null
      setProfile(detectPerformanceProfile())
    }

    mediaQuery.addEventListener('change', handler)
    return () => mediaQuery.removeEventListener('change', handler)
  }, [])

  return profile
}
