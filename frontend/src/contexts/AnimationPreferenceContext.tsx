import type { ReactNode } from 'react'
import { createContext, useContext, useState } from 'react'
import { clearAutoDemoteMemory } from '../utils/animationAutoAdapt'

/** auto may rAF-demote; standard/light map in useAnimationLevel (no sampling). prefers-reduced-motion → exlight. */
export type AnimationPreference = 'auto' | 'standard' | 'light'

interface AnimationPreferenceContextType {
  preference: AnimationPreference
  setPreference: (pref: AnimationPreference) => void
  togglePerformanceMode: () => void
}

const AnimationPreferenceContext = createContext<
  AnimationPreferenceContextType | undefined
>(undefined)

export { AnimationPreferenceContext }

const STORAGE_KEY = 'animation-preference'

export function AnimationPreferenceProvider({
  children,
}: {
  children: ReactNode
}) {
  const [preference, setPreferenceState] = useState<AnimationPreference>(() => {
    if (typeof window !== 'undefined') {
      const stored = localStorage.getItem(STORAGE_KEY)
      if (stored === 'auto' || stored === 'standard' || stored === 'light') {
        return stored
      }
    }
    return 'auto'
  })

  const setPreference = (pref: AnimationPreference) => {
    setPreferenceState(pref)
    if (typeof window !== 'undefined') {
      localStorage.setItem(STORAGE_KEY, pref)
    }
    // Choosing high clears auto-demote memory.
    if (pref === 'standard') {
      clearAutoDemoteMemory()
    }
  }

  // Toggle standard ↔ light; hardware mapping is in useAnimationLevel.
  const togglePerformanceMode = () => {
    const currentMode = preference === 'auto' ? 'standard' : preference
    const newMode = currentMode === 'standard' ? 'light' : 'standard'
    setPreference(newMode)
  }

  return (
    <AnimationPreferenceContext.Provider
      value={{ preference, setPreference, togglePerformanceMode }}
    >
      {children}
    </AnimationPreferenceContext.Provider>
  )
}

export function useAnimationPreference() {
  const context = useContext(AnimationPreferenceContext)
  if (context === undefined) {
    throw new Error(
      'useAnimationPreference must be used within AnimationPreferenceProvider',
    )
  }
  return context
}
