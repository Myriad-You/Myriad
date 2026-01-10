/**
 * Service Worker 注册工具
 * 提供离线缓存和性能优化
 */

export async function registerServiceWorker(): Promise<ServiceWorkerRegistration | null> {
  // 只在生产环境和支持 Service Worker 的浏览器中注册
  if (
    import.meta.env.PROD
      && 'serviceWorker' in navigator
      && window.location.protocol === 'https:' || window.location.hostname === 'localhost'
  ) {
    try {
      const registration = await navigator.serviceWorker.register('/sw.js', {
        scope: '/',
      })

      // 监听更新
      registration.addEventListener('updatefound', () => {
        const newWorker = registration.installing
        if (newWorker) {
          newWorker.addEventListener('statechange', () => {
            if (newWorker.state === 'installed' && navigator.serviceWorker.controller) {
              // 新版本可用 - 可以在这里通知用户
              const event = new CustomEvent('sw-update-available', {
                detail: { registration },
              })
              window.dispatchEvent(event)
            }
          })
        }
      })

      // 检查更新
      registration.update()

      return registration
    }
    catch (error) {
      console.error('[SW] Service Worker registration failed:', error)
      return null
    }
  }

  return null
}

/**
 * 取消注册 Service Worker
 */
export async function unregisterServiceWorker(): Promise<boolean> {
  if ('serviceWorker' in navigator) {
    try {
      const registration = await navigator.serviceWorker.getRegistration()
      if (registration) {
        const success = await registration.unregister()
        return success
      }
    }
    catch (error) {
      console.error('[SW] Service Worker unregister failed:', error)
    }
  }
  return false
}

/**
 * 清除所有缓存
 */
export async function clearServiceWorkerCache(): Promise<void> {
  if ('serviceWorker' in navigator && 'caches' in window) {
    try {
      const keys = await caches.keys()
      await Promise.all(keys.map(key => caches.delete(key)))
    }
    catch (error) {
      console.error('[SW] Failed to clear caches:', error)
    }
  }
}

/**
 * 发送消息给 Service Worker
 */
export async function sendMessageToSW(message: any): Promise<any> {
  if ('serviceWorker' in navigator) {
    const controller = navigator.serviceWorker.controller
    if (controller) {
      return new Promise((resolve, reject) => {
        const messageChannel = new MessageChannel()

        messageChannel.port1.onmessage = (event) => {
          if (event.data.error) {
            reject(event.data.error)
          }
          else {
            resolve(event.data)
          }
        }

        controller.postMessage(message, [messageChannel.port2])
      })
    }
  }
  throw new Error('Service Worker not available')
}
