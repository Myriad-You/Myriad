/**
 * 页内子分类 TOC：顶层 SettingGroup 自动登记，SettingSection 右侧渲染跳转芯片。
 */

import type { ReactNode } from 'react'
import React, {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
} from 'react'

export interface SettingsTocItem {
  id: string
  label: string
  /** 稳定排序：注册序号 */
  order: number
}

interface SettingsTocContextValue {
  items: SettingsTocItem[]
  register: (id: string, label: string) => void
  unregister: (id: string) => void
}

const SettingsTocContext = createContext<SettingsTocContextValue | null>(null)

export function useSettingsToc(): SettingsTocContextValue | null {
  return useContext(SettingsTocContext)
}

export const SettingsTocProvider: React.FC<{ children: ReactNode }> = ({
  children,
}) => {
  const [items, setItems] = useState<SettingsTocItem[]>([])
  const orderRef = useRef(0)

  const register = useCallback((id: string, label: string) => {
    setItems((prev) => {
      const existing = prev.find((x) => x.id === id)
      if (existing) {
        if (existing.label === label) return prev
        return prev.map((x) => (x.id === id ? { ...x, label } : x))
      }
      const order = orderRef.current++
      return [...prev, { id, label, order }].sort((a, b) => a.order - b.order)
    })
  }, [])

  const unregister = useCallback((id: string) => {
    setItems((prev) => prev.filter((x) => x.id !== id))
  }, [])

  const value = useMemo(
    () => ({ items, register, unregister }),
    [items, register, unregister],
  )

  return (
    <SettingsTocContext.Provider value={value}>
      {children}
    </SettingsTocContext.Provider>
  )
}

/** title → 稳定 DOM id（页内跳转；不依赖 URL hash） */
export function slugifySettingGroupId(title: string): string {
  const s = title
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48)
  return s ? `sg-${s}` : ''
}
