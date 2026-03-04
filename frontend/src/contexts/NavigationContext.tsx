import type { ReactNode } from 'react'
import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react'

export interface SecondaryNavItem {
  id: string
  icon: ReactNode
  label: string
  title?: string
  ariaLabel?: string
}

export interface SecondaryNavConfig {
  routePath: string
  items: SecondaryNavItem[]
  activeId: string
  onChange: (id: string) => void
  expanded: boolean
  onToggleExpand: () => void
  expandHint?: string
}

interface NavigationContextValue {
  secondaryNav: SecondaryNavConfig | null
  secondaryNavRoutePath: string | null
  setSecondaryNavRoutePath: (routePath: string | null) => void
  registerSecondaryNav: (config: SecondaryNavConfig) => void
  unregisterSecondaryNav: (routePath: string) => void
  updateSecondaryNav: (updates: Partial<Pick<SecondaryNavConfig, 'activeId' | 'expanded'>>) => void
  isAnimating: boolean
  setIsAnimating: (value: boolean) => void
  renderModeRef: React.MutableRefObject<'normal' | 'secondary'>
  immersiveMode: boolean
  setImmersiveMode: (value: boolean) => void
}

const NavigationContext = createContext<NavigationContextValue | null>(null)

export function NavigationProvider({ children }: { children: ReactNode }) {
  const [secondaryNav, setSecondaryNav] = useState<SecondaryNavConfig | null>(null)
  const [secondaryNavRoutePath, setSecondaryNavRoutePath] = useState<string | null>(null)
  const [isAnimating, setIsAnimating] = useState(false)
  const [immersiveMode, setImmersiveMode] = useState(false)
  const renderModeRef = useRef<'normal' | 'secondary'>('normal')

  const registerSecondaryNav = useCallback((config: SecondaryNavConfig) => {
    setSecondaryNav(config)
  }, [])

  const unregisterSecondaryNav = useCallback((routePath: string) => {
    setSecondaryNav((prev) => {
      if (prev?.routePath === routePath) {
        return null
      }
      return prev
    })
  }, [])

  const updateSecondaryNav = useCallback((updates: Partial<Pick<SecondaryNavConfig, 'activeId' | 'expanded'>>) => {
    setSecondaryNav((prev) => {
      if (!prev)
        return null
      return { ...prev, ...updates }
    })
  }, [])

  return (
    <NavigationContext.Provider
      value={{
        secondaryNav,
        secondaryNavRoutePath,
        setSecondaryNavRoutePath,
        registerSecondaryNav,
        unregisterSecondaryNav,
        updateSecondaryNav,
        isAnimating,
        setIsAnimating,
        renderModeRef,
        immersiveMode,
        setImmersiveMode,
      }}
    >
      {children}
    </NavigationContext.Provider>
  )
}

export function useNavigation() {
  const context = useContext(NavigationContext)
  if (!context) {
    throw new Error('useNavigation must be used within NavigationProvider')
  }
  return context
}

export function useSecondaryNav(config: {
  routePath: string
  items: SecondaryNavItem[]
  defaultActiveId: string
  expandHint?: string
}) {
  const { registerSecondaryNav, unregisterSecondaryNav, updateSecondaryNav } = useNavigation()
  const [activeId, setActiveId] = useState(config.defaultActiveId)
  const [expanded, setExpanded] = useState(false)

  const handleChange = useCallback((id: string) => {
    setActiveId(id)
  }, [])

  const handleToggleExpand = useCallback(() => {
    setExpanded(prev => !prev)
  }, [])

  useEffect(() => {
    registerSecondaryNav({
      routePath: config.routePath,
      items: config.items,
      activeId,
      onChange: handleChange,
      expanded,
      onToggleExpand: handleToggleExpand,
      expandHint: config.expandHint,
    })

    return () => {
      unregisterSecondaryNav(config.routePath)
    }
  }, [
    config.routePath,
    config.items,
    config.expandHint,
    registerSecondaryNav,
    unregisterSecondaryNav,
    handleChange,
    handleToggleExpand,
  ])

  useEffect(() => {
    updateSecondaryNav({ activeId, expanded })
  }, [activeId, expanded, updateSecondaryNav])

  return {
    activeId,
    setActiveId,
    expanded,
    setExpanded,
    toggleExpand: handleToggleExpand,
  }
}
