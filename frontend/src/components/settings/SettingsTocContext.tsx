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
      return [...prev, { id, label, order }].toSorted((a, b) => a.order - b.order)
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

export function slugifySettingGroupId(title: string): string {
  const s = title
    .trim()
    .toLowerCase()
    .replaceAll(/[^\p{L}\p{N}]+/gu, '-')
    .replaceAll(/^-+|-+$/g, '')
    .slice(0, 48)
  return s ? `sg-${s}` : ''
}
