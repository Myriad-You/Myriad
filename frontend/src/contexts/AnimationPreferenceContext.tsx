import type { ReactNode } from 'react'
import { createContext, useContext, useState } from 'react'

/**
 * 动效偏好设置类型
 * - 'auto': 自动检测设备性能
 * - 'standard': 强制使用标准动效（中高性能）
 * - 'light': 强制使用轻量动效（低性能）
 */
export type AnimationPreference = 'auto' | 'standard' | 'light'

interface AnimationPreferenceContextType {
  preference: AnimationPreference
  setPreference: (pref: AnimationPreference) => void
  togglePerformanceMode: () => void // 在 standard 和 light 之间切换
}

const AnimationPreferenceContext = createContext<AnimationPreferenceContextType | undefined>(undefined)

// Export the context for direct useContext access
export { AnimationPreferenceContext }

const STORAGE_KEY = 'animation-preference'

export function AnimationPreferenceProvider({ children }: { children: ReactNode }) {
  const [preference, setPreferenceState] = useState<AnimationPreference>(() => {
    // 从 localStorage 读取用户偏好
    if (typeof window !== 'undefined') {
      const stored = localStorage.getItem(STORAGE_KEY)
      if (stored === 'auto' || stored === 'standard' || stored === 'light') {
        return stored
      }
    }
    return 'auto' // 默认自动检测
  })

  // 保存偏好到 localStorage
  const setPreference = (pref: AnimationPreference) => {
    setPreferenceState(pref)
    if (typeof window !== 'undefined') {
      localStorage.setItem(STORAGE_KEY, pref)
    }
  }

  // 切换性能模式（在 standard 和 light 之间）
  const togglePerformanceMode = () => {
    const currentMode = preference === 'auto' ? 'standard' : preference
    const newMode = currentMode === 'standard' ? 'light' : 'standard'
    setPreference(newMode)
  }

  return (
    <AnimationPreferenceContext.Provider value={{ preference, setPreference, togglePerformanceMode }}>
      {children}
    </AnimationPreferenceContext.Provider>
  )
}

export function useAnimationPreference() {
  const context = useContext(AnimationPreferenceContext)
  if (context === undefined) {
    throw new Error('useAnimationPreference must be used within AnimationPreferenceProvider')
  }
  return context
}
