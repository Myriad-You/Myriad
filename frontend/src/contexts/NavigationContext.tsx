import type { ReactNode } from 'react'

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from 'react'

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

/** 就地改传入的 Set（调用方是 ref）；返回此刻是否该隐藏导航岛。 */
export function toggleImmersiveReason(
  reasons: Set<string>,
  reason: string,
  active: boolean,
): boolean {
  if (active) reasons.add(reason)
  else reasons.delete(reason)
  return reasons.size > 0
}

interface NavigationContextValue {
  secondaryNav: SecondaryNavConfig | null
  registerSecondaryNav: (config: SecondaryNavConfig) => void
  unregisterSecondaryNav: (routePath: string) => void
  updateSecondaryNav: (
    updates: Partial<Pick<SecondaryNavConfig, 'activeId' | 'expanded'>>,
  ) => void
  isAnimating: boolean
  setIsAnimating: (value: boolean) => void
  /** 动画期间锁定渲染 */
  renderModeRef: React.RefObject<'normal' | 'secondary'>
  /** 任一理由成立即隐藏导航岛 */
  immersiveMode: boolean
  /** 所有理由撤销后导航岛才回来。页面侧用 `useImmersiveChrome`。 */
  setImmersiveReason: (reason: string, active: boolean) => void
}

const NavigationContext = createContext<NavigationContextValue | null>(null)

export function NavigationProvider({ children }: { children: ReactNode }) {
  const [secondaryNav, setSecondaryNav] = useState<SecondaryNavConfig | null>(
    null,
  )
  const [isAnimating, setIsAnimating] = useState(false)
  const [immersiveMode, setImmersiveMode] = useState(false)
  const renderModeRef = useRef<'normal' | 'secondary'>('normal')

  /** 多路理由；最后一条撤销才恢复导航岛。 */
  const immersiveReasons = useRef<Set<string>>(new Set())

  const setImmersiveReason = useCallback((reason: string, active: boolean) => {
    setImmersiveMode(toggleImmersiveReason(immersiveReasons.current, reason, active))
  }, [])

  const registerSecondaryNav = useCallback((config: SecondaryNavConfig) => {
    setSecondaryNav(config)
  }, [])

  const unregisterSecondaryNav = useCallback((routePath: string) => {
    setSecondaryNav((prev) => {
      if (prev?.routePath === routePath) {
        // 子路由（如 /brew/item/xxx）保留，避免闪掉
        if (window.location.pathname.startsWith(`${routePath}/`)) {
          return prev
        }
        return null
      }
      return prev
    })
  }, [])

  const updateSecondaryNav = useCallback(
    (updates: Partial<Pick<SecondaryNavConfig, 'activeId' | 'expanded'>>) => {
      setSecondaryNav((prev) => {
        if (!prev) return null
        return { ...prev, ...updates }
      })
    },
    [],
  )

  const value = useMemo(
    () => ({
      secondaryNav,
      registerSecondaryNav,
      unregisterSecondaryNav,
      updateSecondaryNav,
      isAnimating,
      setIsAnimating,
      renderModeRef,
      immersiveMode,
      setImmersiveReason,
    }),
    [
      secondaryNav,
      registerSecondaryNav,
      unregisterSecondaryNav,
      updateSecondaryNav,
      isAnimating,
      immersiveMode,
      setImmersiveReason,
    ],
  )

  return (
    <NavigationContext.Provider value={value}>
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

/** `reason` 仅调试；实例用 `useId` 后缀，多处挂载互不顶掉。卸载或 `active=false` 撤销。 */
export function useImmersiveChrome(reason: string, active: boolean): void {
  const { setImmersiveReason } = useNavigation()
  const instanceId = useId()
  const key = `${reason}#${instanceId}`

  useEffect(() => {
    setImmersiveReason(key, active)
    return () => setImmersiveReason(key, false)
  }, [key, active, setImmersiveReason])
}

export function useSecondaryNav(config: {
  routePath: string
  items: SecondaryNavItem[]
  defaultActiveId: string
  expandHint?: string
}) {
  const {
    secondaryNav,
    registerSecondaryNav,
    unregisterSecondaryNav,
    updateSecondaryNav,
  } = useNavigation()
  const isSameGroup = secondaryNav?.routePath === config.routePath
  const existingActiveId = isSameGroup ? secondaryNav.activeId : null
  const existingExpanded = isSameGroup
    ? (secondaryNav.expanded ?? false)
    : false
  const [activeId, setActiveIdLocal] = useState(
    existingActiveId ?? config.defaultActiveId,
  )
  const [expanded, setExpandedLocal] = useState(existingExpanded)

  // updater 内再调 updateSecondaryNav 会触发 React 18 "setState during render"
  const activeIdRef = useRef(activeId)
  activeIdRef.current = activeId
  const expandedRef = useRef(expanded)
  expandedRef.current = expanded

  const setActiveId = useCallback(
    (value: string | ((prev: string) => string)) => {
      const next =
        typeof value === 'function' ? value(activeIdRef.current) : value
      setActiveIdLocal(next)
      updateSecondaryNav({ activeId: next })
    },
    [updateSecondaryNav],
  )

  const setExpanded = useCallback(
    (value: boolean | ((prev: boolean) => boolean)) => {
      const next =
        typeof value === 'function' ? value(expandedRef.current) : value
      setExpandedLocal(next)
      updateSecondaryNav({ expanded: next })
    },
    [updateSecondaryNav],
  )

  const handleChange = useCallback(
    (id: string) => {
      setActiveId(id)
    },
    [setActiveId],
  )

  const handleToggleExpand = useCallback(() => {
    setExpanded((prev) => !prev)
  }, [setExpanded])

  const handleChangeRef = useRef(handleChange)
  handleChangeRef.current = handleChange
  const handleToggleExpandRef = useRef(handleToggleExpand)
  handleToggleExpandRef.current = handleToggleExpand

  useEffect(() => {
    registerSecondaryNav({
      routePath: config.routePath,
      items: config.items,
      activeId,
      onChange: (id: string) => handleChangeRef.current(id),
      expanded,
      onToggleExpand: () => handleToggleExpandRef.current(),
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
  ])

  return {
    activeId,
    setActiveId,
    expanded,
    setExpanded,
    toggleExpand: handleToggleExpand,
  }
}
