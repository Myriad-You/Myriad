/**
 * 导航上下文 - 支持页面声明式二级导航
 *
 * 设计理念：
 * - 一级导航（主导航按钮）由 AppLayout 统一管理
 * - 二级导航（筛选、标签等）由各页面声明
 * - 支持动画过渡和状态同步
 */

import type { ReactNode } from 'react'
import { createContext, useCallback, useContext, useEffect, useRef, useState } from 'react'

// 二级导航项配置
export interface SecondaryNavItem {
  id: string
  icon: ReactNode
  label: string
  title?: string
  ariaLabel?: string
}

// 二级导航配置
export interface SecondaryNavConfig {
  /** 唯一标识符，对应路由路径 */
  routePath: string
  /** 导航项列表 */
  items: SecondaryNavItem[]
  /** 当前选中的项 ID */
  activeId: string
  /** 选中项变化回调 */
  onChange: (id: string) => void
  /** 是否展开二级导航 */
  expanded: boolean
  /** 展开/收起切换回调 */
  onToggleExpand: () => void
  /** 展开按钮的提示文案 */
  expandHint?: string
}

// 导航上下文值
interface NavigationContextValue {
  /** 当前注册的二级导航配置 */
  secondaryNav: SecondaryNavConfig | null
  /** 注册二级导航（页面调用） */
  registerSecondaryNav: (config: SecondaryNavConfig) => void
  /** 注销二级导航（页面卸载时调用） */
  unregisterSecondaryNav: (routePath: string) => void
  /** 更新二级导航状态 */
  updateSecondaryNav: (updates: Partial<Pick<SecondaryNavConfig, 'activeId' | 'expanded'>>) => void
  /** 动画状态 */
  isAnimating: boolean
  /** 设置动画状态 */
  setIsAnimating: (value: boolean) => void
  /** 渲染模式引用（用于动画期间锁定渲染） */
  renderModeRef: React.MutableRefObject<'normal' | 'secondary'>
  /** 沉浸模式（隐藏导航岛和控制面板） */
  immersiveMode: boolean
  /** 设置沉浸模式 */
  setImmersiveMode: (value: boolean) => void
}

const NavigationContext = createContext<NavigationContextValue | null>(null)

export function NavigationProvider({ children }: { children: ReactNode }) {
  const [secondaryNav, setSecondaryNav] = useState<SecondaryNavConfig | null>(null)
  const [isAnimating, setIsAnimating] = useState(false)
  const [immersiveMode, setImmersiveMode] = useState(false)
  const renderModeRef = useRef<'normal' | 'secondary'>('normal')

  // 注册二级导航
  const registerSecondaryNav = useCallback((config: SecondaryNavConfig) => {
    setSecondaryNav(config)
  }, [])

  // 注销二级导航
  const unregisterSecondaryNav = useCallback((routePath: string) => {
    setSecondaryNav((prev) => {
      if (prev?.routePath === routePath) {
        return null
      }
      return prev
    })
  }, [])

  // 更新二级导航状态
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

// Hook：使用导航上下文
export function useNavigation() {
  const context = useContext(NavigationContext)
  if (!context) {
    throw new Error('useNavigation must be used within NavigationProvider')
  }
  return context
}

// Hook：页面声明二级导航
export function useSecondaryNav(config: {
  routePath: string
  items: SecondaryNavItem[]
  defaultActiveId: string
  /** 展开按钮的提示文案 */
  expandHint?: string
}) {
  const { registerSecondaryNav, unregisterSecondaryNav, updateSecondaryNav } = useNavigation()
  const [activeId, setActiveId] = useState(config.defaultActiveId)
  const [expanded, setExpanded] = useState(false)

  // 选中项变化处理
  const handleChange = useCallback((id: string) => {
    setActiveId(id)
  }, [])

  // 展开/收起切换
  const handleToggleExpand = useCallback(() => {
    setExpanded(prev => !prev)
  }, [])

  // 注册二级导航
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
    activeId,
    expanded,
    registerSecondaryNav,
    unregisterSecondaryNav,
    handleChange,
    handleToggleExpand,
  ])

  // 同步更新到上下文
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
