/**
 * Toast 通知管理器
 *
 * 提供全局 Toast 通知的发布/订阅机制
 * 可以从任何组件或工具函数中触发 Toast 通知
 *
 * 使用示例:
 * ```tsx
 * import { showToast, ToastEvent } from '@/utils/toastManager';
 *
 * // 显示成功通知
 * showToast({ message: '保存成功', type: 'success' });
 *
 * // 显示带标题的通知
 * showToast({ title: 'Tapp', message: '应用已启动', type: 'info' });
 * ```
 */

import type { ToastType } from '../components/Toast'

/** Toast 事件数据 */
export interface ToastEvent {
  /** 消息内容 */
  message: string
  /** 可选标题 */
  title?: string
  /** 消息类型 */
  type?: ToastType
  /** 显示时长（毫秒） */
  duration?: number
  /** 是否显示关闭按钮 */
  showCloseButton?: boolean
  /** 自定义图标 */
  icon?: string
  /** 时间戳（用于去重） */
  timestamp?: number
}

/** Toast 事件监听器 */
type ToastListener = (event: ToastEvent) => void

/** 监听器集合 */
const listeners = new Set<ToastListener>()

/**
 * 订阅 Toast 事件
 * @param listener 事件监听器
 * @returns 取消订阅函数
 */
export function subscribeToast(listener: ToastListener): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

/**
 * 发布 Toast 通知
 * @param event Toast 事件数据
 */
export function showToast(event: ToastEvent): void {
  const eventWithTimestamp: ToastEvent = {
    ...event,
    timestamp: Date.now(),
  }

  listeners.forEach((listener) => {
    try {
      listener(eventWithTimestamp)
    }
    catch (error) {
      console.error('[ToastManager] Listener error:', error)
    }
  })
}

/**
 * 快捷方法：显示成功通知
 */
export function showSuccess(message: string, title?: string): void {
  showToast({ message, title, type: 'success' })
}

/**
 * 快捷方法：显示错误通知
 */
export function showError(message: string, title?: string): void {
  showToast({ message, title, type: 'error' })
}

/**
 * 快捷方法：显示警告通知
 */
export function showWarning(message: string, title?: string): void {
  showToast({ message, title, type: 'warning' })
}

/**
 * 快捷方法：显示信息通知
 */
export function showInfo(message: string, title?: string): void {
  showToast({ message, title, type: 'info' })
}

/**
 * 默认导出
 */
export default {
  subscribe: subscribeToast,
  show: showToast,
  success: showSuccess,
  error: showError,
  warning: showWarning,
  info: showInfo,
}
