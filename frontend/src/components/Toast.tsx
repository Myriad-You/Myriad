/**
 * 统一的Toast提示组件
 *
 * 设计参考页面信息条（InfoBar）风格：
 * - Glass morphism 背景
 * - 圆角设计
 * - 主题色适配
 * - 支持多种消息类型
 *
 * 这是统一 Toast 容器的渲染原语。持久业务通知由通知中心流投递到这里；
 * 当前页面的瞬时操作反馈也可直接复用该原语，但不会伪装成通知中心记录。
 */

import { useCallback, useEffect, useState } from 'react'
import { currentCopy } from '../i18n/localeCopy'

import './Toast.css'

/** Toast 消息类型 */
export type ToastType = 'success' | 'error' | 'warning' | 'info'

const STATUS_ICON_ASSETS = {
  success: '/icons/status/success.webp',
  error: '/icons/status/error.webp',
  warning: '/icons/status/warning.webp',
  info: '/icons/status/info.webp',
} satisfies Record<ToastType, string>

/** Toast 配置接口 */
export interface ToastProps {
  /** 消息内容 */
  message: string
  /** 可选标题 */
  title?: string
  /** 消息类型 */
  type?: ToastType
  /** 关闭回调 */
  onClose?: () => void
  /** 显示时长（毫秒），0 表示不自动关闭 */
  duration?: number
  /** 是否显示关闭按钮 */
  showCloseButton?: boolean
  /** 自定义图标（emoji 或 React 节点） */
  icon?: React.ReactNode
  /** 点击整条 toast 的动作（点击后自动关闭） */
  onClick?: () => void
}

/** 类型配置映射 */
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

/**
 * Toast 提示组件
 */
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

  // 获取类型配置
  const config = TYPE_CONFIG[type]

  // 处理关闭
  const handleClose = useCallback(() => {
    setIsHiding(true)
    // 等待动画完成后调用 onClose
    setTimeout(() => {
      onClose?.()
    }, 300)
  }, [onClose])

  // 自动关闭计时器
  useEffect(() => {
    if (duration <= 0 || isPaused) return

    const hideTimer = setTimeout(() => {
      handleClose()
    }, duration)

    return () => {
      clearTimeout(hideTimer)
    }
  }, [duration, isPaused, handleClose])

  // 鼠标悬停时暂停自动关闭
  const handleMouseEnter = useCallback(() => {
    setIsPaused(true)
  }, [])

  const handleMouseLeave = useCallback(() => {
    setIsPaused(false)
  }, [])

  // 渲染图标
  const renderIcon = () => {
    if (icon) {
      return <span className="toast-icon toast-icon-custom">{icon}</span>
    }

    return renderToastAssetIcon(type)
  }

  // 点击整条 toast：执行动作并关闭
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
        {/* 图标区域 */}
        <div className="toast-icon-wrapper">{renderIcon()}</div>

        {/* 内容区域 */}
        <div className="toast-content">
          {title && <div className="toast-title">{title}</div>}
          <div className="toast-text">{message}</div>
        </div>

        {/* 关闭按钮（可选） */}
        {showCloseButton && (
          <button
            className="toast-close-btn"
            onClick={(e) => {
              // 阻止冒泡到可点击 toast 本体，关闭不应触发跳转动作
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
