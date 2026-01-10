/**
 * 统一的Toast提示组件
 *
 * 设计参考页面信息条（InfoBar）风格：
 * - Glass morphism 背景
 * - 圆角设计
 * - 主题色适配
 * - 支持多种消息类型
 *
 * 与 Tapp 系统的集成：
 * - 通过 TappSandbox 的 onNotification 回调接收通知
 * - 支持 success/error/warning/info 四种类型
 * - 可选标题和消息组合
 */

import { useCallback, useEffect, useMemo, useState } from 'react'
import { TappIcon } from '../tapp/components/TappIcon'
import './Toast.css'

/** Toast 消息类型 */
export type ToastType = 'success' | 'error' | 'warning' | 'info'

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
}

/** 类型配置映射 */
const TYPE_CONFIG = {
  success: {
    iconPath: 'M16.6667 5L7.50004 14.1667L3.33337 10',
    colorClass: 'toast-success',
    defaultIcon: '✓',
  },
  error: {
    iconPath: 'M15 5L5 15M5 5L15 15',
    colorClass: 'toast-error',
    defaultIcon: '✗',
  },
  warning: {
    iconPath: 'M10 6V10M10 14H10.01',
    colorClass: 'toast-warning',
    defaultIcon: '⚠',
  },
  info: {
    iconPath: 'M10 14V10M10 6H10.01',
    colorClass: 'toast-info',
    defaultIcon: 'ℹ',
  },
} as const

/**
 * 从消息内容自动推断类型
 */
function inferTypeFromMessage(message: string): ToastType {
  if (message.startsWith('✓') || message.startsWith('✔'))
    return 'success'
  if (message.startsWith('✗') || message.startsWith('✘') || message.startsWith('❌'))
    return 'error'
  if (message.startsWith('⚠') || message.startsWith('⚡'))
    return 'warning'
  if (message.startsWith('ℹ') || message.startsWith('💡'))
    return 'info'
  return 'info' // 默认为 info 类型
}

/**
 * 清理消息中的前缀符号
 */
function cleanMessagePrefix(message: string): string {
  return message.replace(/^[✓✔✗✘❌⚠⚡ℹ💡]\s*/, '')
}

/**
 * Toast 提示组件
 */
export default function Toast({
  message,
  title,
  type,
  onClose,
  duration = 3000,
  showCloseButton = false,
  icon,
}: ToastProps) {
  const [isHiding, setIsHiding] = useState(false)
  const [isPaused, setIsPaused] = useState(false)

  // 自动推断类型
  const toastType = useMemo(() => type || inferTypeFromMessage(message), [type, message])

  // 清理消息前缀
  const cleanMessage = useMemo(() => cleanMessagePrefix(message), [message])

  // 获取类型配置
  const config = TYPE_CONFIG[toastType]

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
    if (duration <= 0 || isPaused)
      return

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

    // 使用 SVG 图标
    return (
      <svg
        className="toast-icon"
        width="18"
        height="18"
        viewBox="0 0 20 20"
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
      >
        <path
          d={config.iconPath}
          stroke="currentColor"
          strokeWidth="2.5"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </svg>
    )
  }

  return (
    <div
      className={`toast-container ${isHiding ? 'toast-hiding' : ''}`}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
    >
      <div className={`toast-message ${config.colorClass}`}>
        {/* 图标区域 */}
        <div className="toast-icon-wrapper">
          {renderIcon()}
        </div>

        {/* 内容区域 */}
        <div className="toast-content">
          {title && <div className="toast-title">{title}</div>}
          <div className="toast-text">{cleanMessage}</div>
        </div>

        {/* 关闭按钮（可选） */}
        {showCloseButton && (
          <button
            className="toast-close-btn"
            onClick={handleClose}
            aria-label="关闭通知"
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none" xmlns="http://www.w3.org/2000/svg">
              <path d="M10.5 3.5L3.5 10.5M3.5 3.5L10.5 10.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
            </svg>
          </button>
        )}
      </div>
    </div>
  )
}

/**
 * Tapp 专用 Toast 组件
 * 用于 TappRunPage 等页面显示 Tapp 发出的通知
 */
export interface TappToastProps {
  /** 通知标题 */
  title?: string
  /** 通知消息 */
  message: string
  /** 通知类型 */
  type?: ToastType
  /** 关闭回调 */
  onClose?: () => void
  /** 显示时长 */
  duration?: number
  /** Tapp 名称（用于显示来源） */
  tappName?: string
  /** Tapp 图标（emoji 或 URL） */
  tappIcon?: string
  /** Tapp 图标（SVG 代码） */
  tappIconSvg?: string
}

/**
 * Tapp 通知 Toast
 * 增强版 Toast，显示 Tapp 来源信息
 */
export function TappToast({
  title,
  message,
  type = 'info',
  onClose,
  duration = 3000,
  tappName,
  tappIcon,
  tappIconSvg,
}: TappToastProps) {
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
    if (duration <= 0 || isPaused)
      return

    const timer = setTimeout(() => {
      handleClose()
    }, duration)

    return () => clearTimeout(timer)
  }, [duration, isPaused, handleClose])

  return (
    <div
      className={`toast-container toast-tapp ${isHiding ? 'toast-hiding' : ''}`}
      onMouseEnter={() => setIsPaused(true)}
      onMouseLeave={() => setIsPaused(false)}
    >
      <div className={`toast-message toast-message-tapp ${config.colorClass}`}>
        {/* Tapp 来源标识 */}
        {(tappName || tappIcon || tappIconSvg) && (
          <div className="toast-tapp-source">
            {(tappIcon || tappIconSvg) && (
              <span className="toast-tapp-icon">
                <TappIcon
                  icon={tappIcon}
                  iconSvg={tappIconSvg}
                  name={tappName || 'Tapp'}
                  sizeClass="w-4 h-4"
                  textSizeClass="text-xs"
                />
              </span>
            )}
            {tappName && <span className="toast-tapp-name">{tappName}</span>}
            <span className="toast-tapp-separator">·</span>
          </div>
        )}

        {/* 图标 */}
        <div className="toast-icon-wrapper">
          <svg
            className="toast-icon"
            width="18"
            height="18"
            viewBox="0 0 20 20"
            fill="none"
          >
            <path
              d={config.iconPath}
              stroke="currentColor"
              strokeWidth="2.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        </div>

        {/* 内容 */}
        <div className="toast-content">
          {title && <div className="toast-title">{title}</div>}
          <div className="toast-text">{message}</div>
        </div>

        {/* 关闭按钮 */}
        <button
          className="toast-close-btn"
          onClick={handleClose}
          aria-label="关闭通知"
        >
          <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
            <path d="M10.5 3.5L3.5 10.5M3.5 3.5L10.5 10.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
        </button>
      </div>
    </div>
  )
}
