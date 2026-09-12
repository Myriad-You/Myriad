import type { ToastEvent } from '../utils/toastManager'
import { useCallback, useEffect, useState } from 'react'
import { subscribeToast } from '../utils/toastManager'
import Toast from './Toast'

interface ToastItem extends ToastEvent {
  id: string
}

let idCounter = 0
function generateId(): string {
  return `toast-${++idCounter}-${Date.now()}`
}

export function ToastContainer() {
  const [toasts, setToasts] = useState<ToastItem[]>([])

  const removeToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id))
  }, [])

  const addToast = useCallback((event: ToastEvent) => {
    const newToast: ToastItem = {
      ...event,
      id: generateId(),
    }

    setToasts((prev) => [...prev, newToast])
  }, [])

  useEffect(() => {
    const unsubscribe = subscribeToast(addToast)
    return unsubscribe
  }, [addToast])

  if (toasts.length === 0) {
    return null
  }

  return (
    <div className="toast-container-wrapper">
      {toasts.slice(0, 5).map((toast, index) => (
        <div
          key={toast.id}
          className="toast-container-item"
          style={{
            transform: `translate(-50%, ${index * 72}px)`,
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
            onClick={toast.onClick}
            onClose={() => removeToast(toast.id)}
          />
        </div>
      ))}
    </div>
  )
}

export default ToastContainer
