/**
 * 共享主色调订阅器 - 避免每个组件都创建 MutationObserver
 * 通过单例模式共享一个 MutationObserver，降低主线程工作量
 *
 * 与 themeSubscriber 类似的设计，用于监听 --color-primary CSS 变量的变化
 */

/**
 * React Hook：订阅主色调变化
 * 使用共享的 MutationObserver，避免每个组件都创建自己的 observer
 */
import { useEffect, useState } from 'react'

type ColorCallback = (color: string) => void

let currentColor
  = typeof document !== 'undefined'
    ? getComputedStyle(document.documentElement).getPropertyValue('--color-primary').trim() || '#8b5cf6'
    : '#8b5cf6'

const subscribers = new Set<ColorCallback>()
let observer: MutationObserver | null = null

function notifySubscribers() {
  const newColor = getComputedStyle(document.documentElement)
    .getPropertyValue('--color-primary')
    .trim()

  if (newColor && newColor !== currentColor) {
    currentColor = newColor
    // 🎯 使用 Array.from 创建快照，确保所有订阅者都被通知
    const subscriberArray = Array.from(subscribers)
    subscriberArray.forEach((callback) => {
      try {
        callback(currentColor)
      }
      catch (e) {
        console.error('Color subscriber error:', e)
      }
    })
  }
}

// 🎯 防抖标记，避免多次 requestAnimationFrame 调用
let rafScheduled = false

function scheduleNotify() {
  if (rafScheduled)
    return
  rafScheduled = true
  requestAnimationFrame(() => {
    rafScheduled = false
    notifySubscribers()
  })
}

function ensureObserver() {
  if (observer || typeof document === 'undefined')
    return

  observer = new MutationObserver((mutations) => {
    for (const mutation of mutations) {
      if (mutation.type === 'attributes' && mutation.attributeName === 'style') {
        // 🎯 使用防抖调度，避免频繁触发
        scheduleNotify()
        break
      }
    }
  })

  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['style'],
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
 * 订阅主色调变化
 * @param callback 颜色变化时的回调
 * @returns 取消订阅函数
 */
export function subscribeToPrimaryColor(callback: ColorCallback): () => void {
  subscribers.add(callback)
  ensureObserver()

  // 立即调用一次，确保初始状态同步
  // 🎯 修复：获取最新颜色而不是使用可能过期的 currentColor
  callback(getPrimaryColor())

  return () => {
    subscribers.delete(callback)
    cleanupObserver()
  }
}

/**
 * 获取当前主色调（同步）
 * 每次调用都会尝试从 DOM 获取最新值，确保在 observer 启动前也能获取到正确颜色
 */
export function getPrimaryColor(): string {
  if (typeof document !== 'undefined') {
    const newColor = getComputedStyle(document.documentElement)
      .getPropertyValue('--color-primary')
      .trim()
    if (newColor && newColor !== currentColor) {
      currentColor = newColor
    }
  }
  return currentColor
}

export function usePrimaryColor(): string {
  const [color, setColor] = useState(() =>
    typeof document !== 'undefined'
      ? getComputedStyle(document.documentElement).getPropertyValue('--color-primary').trim() || '#8b5cf6'
      : '#8b5cf6',
  )

  useEffect(() => {
    return subscribeToPrimaryColor(setColor)
  }, [])

  return color
}
