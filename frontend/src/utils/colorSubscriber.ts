import { useEffect, useState } from 'react'

type ColorCallback = (color: string) => void

let currentColor =
  typeof document !== 'undefined'
    ? getComputedStyle(document.documentElement)
        .getPropertyValue('--color-primary')
        .trim() || '#8b5cf6'
    : '#8b5cf6'

const subscribers = new Set<ColorCallback>()
let observer: MutationObserver | null = null

function notifySubscribers() {
  const newColor = getComputedStyle(document.documentElement)
    .getPropertyValue('--color-primary')
    .trim()

  if (newColor && newColor !== currentColor) {
    currentColor = newColor
    // Snapshot listeners before notify.
    const subscriberArray = Iterator.from(subscribers).toArray()
    subscriberArray.forEach((callback) => {
      try {
        callback(currentColor)
      } catch (e) {
        console.error('Color subscriber error:', e)
      }
    })
  }
}

let rafScheduled = false

function scheduleNotify() {
  if (rafScheduled) return
  rafScheduled = true
  requestAnimationFrame(() => {
    rafScheduled = false
    notifySubscribers()
  })
}

function ensureObserver() {
  if (observer || typeof document === 'undefined') return

  observer = new MutationObserver((mutations) => {
    for (const mutation of mutations) {
      if (
        mutation.type === 'attributes' &&
        mutation.attributeName === 'style'
      ) {
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

export function subscribeToPrimaryColor(callback: ColorCallback): () => void {
  subscribers.add(callback)
  ensureObserver()

  // Do not close over a stale color.
  callback(getPrimaryColor())

  return () => {
    subscribers.delete(callback)
    cleanupObserver()
  }
}

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
      ? getComputedStyle(document.documentElement)
          .getPropertyValue('--color-primary')
          .trim() || '#8b5cf6'
      : '#8b5cf6',
  )

  useEffect(() => {
    return subscribeToPrimaryColor(setColor)
  }, [])

  return color
}
