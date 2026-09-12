import { useCallback, useEffect, useState } from 'react'
import { currentCopy } from '../i18n/localeCopy'

import './Toast.css'

export type ToastType = 'success' | 'error' | 'warning' | 'info'

const STATUS_ICON_ASSETS = {
  success: '/icons/status/success.webp',
  error: '/icons/status/error.webp',
  warning: '/icons/status/warning.webp',
  info: '/icons/status/info.webp',
} satisfies Record<ToastType, string>

export interface ToastProps {
  message: string
  title?: string
  type?: ToastType
  onClose?: () => void
  duration?: number
  showCloseButton?: boolean
  icon?: React.ReactNode
  onClick?: () => void
}

export const TYPE_CONFIG = {
  success: {
    icon: STATUS_ICON_ASSETS.success,
    colorClass: 'toast-success',
  },
  error: {
    icon: STATUS_ICON_ASSETS.error,
    colorClass: 'toast-error',
  },
  warning: {
    icon: STATUS_ICON_ASSETS.warning,
    colorClass: 'toast-warning',
  },
  info: {
    icon: STATUS_ICON_ASSETS.info,
    colorClass: 'toast-info',
  },
} as const

export function renderToastAssetIcon(type: ToastType) {
  return (
    <img
      src={TYPE_CONFIG[type].icon}
      alt=""
      aria-hidden="true"
      className="toast-icon toast-icon-asset"
      draggable={false}
      decoding="async"
    />
  )
}

export default function Toast({
  message,
  title,
  type = 'info',
  onClose,
  duration = 3000,
  showCloseButton = false,
  icon,
  onClick,
}: ToastProps) {
  const [isHiding, setIsHiding] = useState(false)
  const [isPaused, setIsPaused] = useState(false)

  const config = TYPE_CONFIG[type]

  const handleClose = useCallback(() => {
    setIsHiding(true)
    setTimeout(() => {
      onClose?.()
    }, 300)
  }, [onClose])

  useEffect(() => {
    if (duration <= 0 || isPaused) return

    const hideTimer = setTimeout(() => {
      handleClose()
    }, duration)

    return () => {
      clearTimeout(hideTimer)
    }
  }, [duration, isPaused, handleClose])

  const handleMouseEnter = useCallback(() => {
    setIsPaused(true)
  }, [])

  const handleMouseLeave = useCallback(() => {
    setIsPaused(false)
  }, [])

  const renderIcon = () => {
    if (icon) {
      return <span className="toast-icon toast-icon-custom">{icon}</span>
    }

    return renderToastAssetIcon(type)
  }

  const handleBodyClick = useCallback(() => {
    if (!onClick) return
    onClick()
    handleClose()
  }, [onClick, handleClose])

  return (
    <div
      className={`toast-container ${isHiding ? 'toast-hiding' : ''}`}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
    >
      <div
        className={`toast-message ${config.colorClass}${onClick ? ' toast-clickable' : ''}`}
        role={onClick ? 'button' : undefined}
        tabIndex={onClick ? 0 : undefined}
        onClick={onClick ? handleBodyClick : undefined}
        onKeyDown={
          onClick
            ? (e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault()
                  handleBodyClick()
                }
              }
            : undefined
        }
      >
        <div className="toast-icon-wrapper">{renderIcon()}</div>

        <div className="toast-content">
          {title && <div className="toast-title">{title}</div>}
          <div className="toast-text">{message}</div>
        </div>

        {showCloseButton && (
          <button
            className="toast-close-btn"
            onClick={(e) => {
              e.stopPropagation()
              handleClose()
            }}
            aria-label={currentCopy().common.closeNotification}
          >
            <svg
              width="14"
              height="14"
              viewBox="0 0 14 14"
              fill="none"
              xmlns="http://www.w3.org/2000/svg"
            >
              <path
                d="M10.5 3.5L3.5 10.5M3.5 3.5L10.5 10.5"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
              />
            </svg>
          </button>
        )}
      </div>
    </div>
  )
}
