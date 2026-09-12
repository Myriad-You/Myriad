import type { ToastType } from '../components/Toast'

export interface ToastEvent {
  message: string
  title?: string
  type?: ToastType
  /** ms */
  duration?: number
  showCloseButton?: boolean
  icon?: string
  onClick?: () => void
  timestamp?: number
}

type ToastListener = (event: ToastEvent) => void

const listeners = new Set<ToastListener>()

export function subscribeToast(listener: ToastListener): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function showToast(event: ToastEvent): void {
  const eventWithTimestamp: ToastEvent = {
    ...event,
    timestamp: Date.now(),
  }

  listeners.forEach((listener) => {
    try {
      listener(eventWithTimestamp)
    } catch (error) {
      console.error('[ToastManager] Listener error:', error)
    }
  })
}

export function showSuccess(message: string, title?: string): void {
  showToast({ message, title, type: 'success' })
}

export function showError(message: string, title?: string): void {
  showToast({ message, title, type: 'error' })
}

export function showWarning(message: string, title?: string): void {
  showToast({ message, title, type: 'warning' })
}

export function showInfo(message: string, title?: string): void {
  showToast({ message, title, type: 'info' })
}

export default {
  subscribe: subscribeToast,
  show: showToast,
  success: showSuccess,
  error: showError,
  warning: showWarning,
  info: showInfo,
}
