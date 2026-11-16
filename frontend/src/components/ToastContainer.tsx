import type { ToastEvent } from '../utils/toastManager'
import { useCallback, useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useNavigate } from 'react-router-dom'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { AI_CONFIGURATION_REQUIRED_EVENT } from '../utils/aiConfiguration'
import { layoutToastStack, showToast, subscribeToast } from '../utils/toastManager'
import Toast from './Toast'

interface ToastItem extends ToastEvent {
  id: string
  leaving?: boolean
}

let idCounter = 0
function generateId(): string {
  return `toast-${++idCounter}-${Date.now()}`
}

function ToastStackItem({
  toast,
  stackIndex,
  leaving,
  onLeaving,
  onClose,
}: {
  toast: ToastItem
  stackIndex: number
  leaving: boolean
  onLeaving: () => void
  onClose: () => void
}) {
  const enterRole = useRef(stackIndex > 0 ? 'follow' : 'lead')
  const [ready, setReady] = useState(false)

  useEffect(() => {
    setReady(true)
  }, [])

  const expanded = ready && !leaving

  return (
    <div
      className={`toast-container-item${expanded ? ' toast-container-item-open' : ''}${leaving ? ' toast-container-item-leaving' : ''}`}
      data-enter={enterRole.current}
    >
      <Toast
        message={toast.message}
        title={toast.title}
        type={toast.type}
        duration={toast.duration}
        showCloseButton={toast.showCloseButton}
        sticky={toast.sticky}
        icon={toast.icon}
        onClick={toast.onClick}
        onLeaving={onLeaving}
        onClose={onClose}
      />
    </div>
  )
}

export function ToastContainer() {
  const { isAdmin } = useAuth()
  const { t } = useI18n()
  const navigate = useNavigate()

  useEffect(() => {
    const onRequired = () => showToast({
      type: 'warning',
      message: isAdmin ? t.errors.aiNotConfigured : t.errors.aiContactAdmin,
      title: isAdmin ? t.errors.aiConfigureAction : undefined,
      onClick: isAdmin ? () => navigate('/config?section=ai') : undefined,
      duration: 10000,
      replaceKey: 'ai-configuration-required',
    })
    window.addEventListener(AI_CONFIGURATION_REQUIRED_EVENT, onRequired)
    return () => window.removeEventListener(AI_CONFIGURATION_REQUIRED_EVENT, onRequired)
  }, [isAdmin, navigate, t])

  const [toasts, setToasts] = useState<ToastItem[]>([])

  const removeToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((toast) => toast.id !== id))
  }, [])

  const beginLeave = useCallback((id: string) => {
    setToasts((prev) => {
      const target = prev.find((toast) => toast.id === id)
      if (!target || target.leaving) return prev
      return prev.map((toast) =>
        toast.id === id ? { ...toast, leaving: true } : toast,
      )
    })
  }, [])

  const addToast = useCallback((event: ToastEvent) => {
    const newToast: ToastItem = {
      ...event,
      id: generateId(),
    }

    setToasts((prev) => {
      const next = event.replaceKey
        ? prev.filter((toast) => toast.replaceKey !== event.replaceKey)
        : prev
      return [...next, newToast]
    })
  }, [])

  useEffect(() => {
    const unsubscribe = subscribeToast(addToast)
    return unsubscribe
  }, [addToast])

  if (toasts.length === 0) {
    return null
  }

  // 挂到 body 上：#app-root 带着 view-transition-name（site-transition.css），自成一层，
  // 提示关在里面时 z-index 再高也只在层内比，会被挂在 body 上的弹层（创作台写作器 z-index 9999）
  // 盖住，透过它半透明的底只剩一层淡影。毛玻璃也只能取到 #app-root 里的内容。
  return createPortal(
    <div className="toast-container-wrapper">
      {layoutToastStack(toasts).map((toast, stackIndex) => (
        <ToastStackItem
          key={toast.id}
          toast={toast}
          stackIndex={stackIndex}
          leaving={Boolean(toast.leaving)}
          onLeaving={() => beginLeave(toast.id)}
          onClose={() => {
            toast.onClose?.()
            removeToast(toast.id)
          }}
        />
      ))}
    </div>,
    document.body,
  )
}

export default ToastContainer
