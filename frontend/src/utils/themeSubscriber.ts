import { useEffect, useState } from 'react'

type ThemeCallback = (isDark: boolean) => void

let isDarkMode =
  typeof document !== 'undefined'
    ? document.documentElement.classList.contains('dark')
    : false

const subscribers = new Set<ThemeCallback>()
let observer: MutationObserver | null = null

function notifySubscribers() {
  const newIsDark = document.documentElement.classList.contains('dark')
  if (newIsDark !== isDarkMode) {
    isDarkMode = newIsDark
    // Snapshot listeners before notify.
    const subscriberArray = Iterator.from(subscribers).toArray()
    subscriberArray.forEach((callback) => {
      try {
        callback(isDarkMode)
      } catch (e) {
        console.error('Theme subscriber error:', e)
      }
    })
  }
}

function ensureObserver() {
  if (observer || typeof document === 'undefined') return

  observer = new MutationObserver((mutations) => {
    for (const mutation of mutations) {
      if (
        mutation.type === 'attributes' &&
        mutation.attributeName === 'class'
      ) {
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

export function subscribeToTheme(callback: ThemeCallback): () => void {
  subscribers.add(callback)
  ensureObserver()

  callback(isDarkMode)

  return () => {
    subscribers.delete(callback)
    cleanupObserver()
  }
}

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
