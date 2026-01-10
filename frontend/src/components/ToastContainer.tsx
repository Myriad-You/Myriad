/**
 * Toast 容器组件
 *
 * 在应用根级别使用，监听全局 Toast 事件并显示通知
 *
 * 使用方式:
 * 1. 在 AppLayout 或 App 组件中引入 <ToastContainer />
 * 2. 使用 showToast() 或快捷方法在任意位置触发通知
 *
 * 示例:
 * ```tsx
 * // App.tsx
 * import { ToastContainer } from './components/ToastContainer';
 *
 * function App() {
 *   return (
 *     <>
 *       <Routes />
 *       <ToastContainer />
 *     </>
 *   );
 * }
 *
 * // 任意组件中
 * import { showSuccess } from '@/utils/toastManager';
 * showSuccess('操作成功！');
 * ```
 */

import type { ToastEvent } from '../utils/toastManager'
import { useCallback, useEffect, useState } from 'react'
import { subscribeToast } from '../utils/toastManager'
import Toast from './Toast'

/** Toast 队列项 */
interface ToastItem extends ToastEvent {
  id: string
}

/** 生成唯一 ID */
let idCounter = 0
function generateId(): string {
  return `toast-${++idCounter}-${Date.now()}`
}

/**
 * Toast 容器组件
 * 管理多个 Toast 的显示和队列
 */
export function ToastContainer() {
  const [toasts, setToasts] = useState<ToastItem[]>([])

  // 移除 Toast
  const removeToast = useCallback((id: string) => {
    setToasts(prev => prev.filter(t => t.id !== id))
  }, [])

  // 添加 Toast
  const addToast = useCallback((event: ToastEvent) => {
    const newToast: ToastItem = {
      ...event,
      id: generateId(),
    }

    setToasts((prev) => {
      // 限制最大同时显示数量为 5
      const updated = [...prev, newToast]
      if (updated.length > 5) {
        return updated.slice(-5)
      }
      return updated
    })
  }, [])

  // 订阅全局 Toast 事件
  useEffect(() => {
    const unsubscribe = subscribeToast(addToast)
    return unsubscribe
  }, [addToast])

  // 渲染 Toast 队列
  if (toasts.length === 0) {
    return null
  }

  return (
    <div className="toast-container-wrapper">
      {toasts.map((toast, index) => (
        <div
          key={toast.id}
          className="toast-container-item"
          style={{
            transform: `translateY(${index * 72}px)`,
            zIndex: 9999 - index,
          }}
        >
          <Toast
            message={toast.message}
            title={toast.title}
            type={toast.type}
            duration={toast.duration}
            showCloseButton={toast.showCloseButton}
            icon={toast.icon}
            onClose={() => removeToast(toast.id)}
          />
        </div>
      ))}
    </div>
  )
}

export default ToastContainer
