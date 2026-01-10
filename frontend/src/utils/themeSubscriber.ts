/**
 * 共享主题订阅器 - 避免每个组件都创建 MutationObserver
 * 通过单例模式共享一个 MutationObserver，降低主线程工作量
 */

/**
 * React Hook：订阅主题变化
 * 使用共享的 MutationObserver，避免每个组件都创建自己的 observer
 */
import { useEffect, useState } from 'react'

type ThemeCallback = (isDark: boolean) => void

let isDarkMode
  = typeof document !== 'undefined'
    ? document.documentElement.classList.contains('dark')
    : false

const subscribers = new Set<ThemeCallback>()
let observer: MutationObserver | null = null

function notifySubscribers() {
  const newIsDark = document.documentElement.classList.contains('dark')
  if (newIsDark !== isDarkMode) {
    isDarkMode = newIsDark
    // 🎯 使用 Array.from 创建快照，确保所有订阅者都被通知
    const subscriberArray = Array.from(subscribers)
    subscriberArray.forEach((callback) => {
      try {
        callback(isDarkMode)
      }
      catch (e) {
        console.error('Theme subscriber error:', e)
      }
    })
  }
}

function ensureObserver() {
  if (observer || typeof document === 'undefined')
    return

  observer = new MutationObserver((mutations) => {
    for (const mutation of mutations) {
      if (mutation.type === 'attributes' && mutation.attributeName === 'class') {
        notifySubscribers()
        break
      }
    }
  })

  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['class'],
    attributeOldValue: false,
  })
}

function cleanupObserver() {
  if (subscribers.size === 0 && observer) {
    observer.disconnect()
    observer = null
  }
}

/**
 * 订阅主题变化
 * @param callback 主题变化时的回调
 * @returns 取消订阅函数
 */
export function subscribeToTheme(callback: ThemeCallback): () => void {
  subscribers.add(callback)
  ensureObserver()

  // 立即调用一次，确保初始状态同步
  callback(isDarkMode)

  return () => {
    subscribers.delete(callback)
    cleanupObserver()
  }
}

/**
 * 获取当前主题状态（同步）
 */
export function getIsDarkMode(): boolean {
  return isDarkMode
}

export function useThemeMode(): boolean {
  const [isDark, setIsDark] = useState(() =>
    typeof document !== 'undefined'
      ? document.documentElement.classList.contains('dark')
      : false,
  )

  useEffect(() => {
    return subscribeToTheme(setIsDark)
  }, [])

  return isDark
}
