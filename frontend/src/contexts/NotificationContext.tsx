/**
 * 全局通知上下文
 * 统一管理所有角落通知（加载提示、错误提示等）
 */

import type { ReactNode } from 'react'
import { createContext, useCallback, useContext, useMemo, useState } from 'react'

interface Notification {
  id: string
  type: 'loading' | 'info' | 'error'
  message: string
}

interface NotificationContextType {
  notifications: Notification[]
  showLoading: (message: string, id?: string) => string
  hideLoading: (id: string) => void
  showInfo: (message: string) => void
  showError: (message: string) => void
}

const NotificationContext = createContext<NotificationContextType | undefined>(undefined)

// 全局计数器确保 ID 唯一
let notificationCounter = 0

export function NotificationProvider({ children }: { children: ReactNode }) {
  const [notifications, setNotifications] = useState<Notification[]>([])

  const showLoading = useCallback((message: string, id?: string) => {
    const notificationId = id || `loading-${Date.now()}-${++notificationCounter}`
    setNotifications(prev => [
      ...prev,
      { id: notificationId, type: 'loading', message },
    ])
    return notificationId
  }, [])

  const hideLoading = useCallback((id: string) => {
    setNotifications(prev => prev.filter(n => n.id !== id))
  }, [])

  const showInfo = useCallback((message: string) => {
    const id = `info-${Date.now()}-${++notificationCounter}`
    setNotifications(prev => [
      ...prev,
      { id, type: 'info', message },
    ])
    // Auto-hide after 3 seconds
    setTimeout(() => {
      setNotifications(prev => prev.filter(n => n.id !== id))
    }, 3000)
  }, [])

  const showError = useCallback((message: string) => {
    const id = `error-${Date.now()}-${++notificationCounter}`
    setNotifications(prev => [
      ...prev,
      { id, type: 'error', message },
    ])
    // Auto-hide after 5 seconds
    setTimeout(() => {
      setNotifications(prev => prev.filter(n => n.id !== id))
    }, 5000)
  }, [])

  // 🔧 性能优化：使用 useMemo 缓存 context value
  const value = useMemo(() => ({
    notifications,
    showLoading,
    hideLoading,
    showInfo,
    showError,
  }), [notifications, showLoading, hideLoading, showInfo, showError])

  return (
    <NotificationContext.Provider value={value}>
      {children}
    </NotificationContext.Provider>
  )
}

export function useNotification() {
  const context = useContext(NotificationContext)
  if (!context) {
    throw new Error('useNotification must be used within NotificationProvider')
  }
  return context
}
