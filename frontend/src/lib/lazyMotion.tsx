import { useEffect, useSyncExternalStore } from 'react'

/**
 * 全局 framer-motion 加载状态
 * 使用单例模式确保所有组件共享同一份加载状态
 */
let globalFM: { motion: any, AnimatePresence: any } | null = null
let isLoading = false
let loadPromise: Promise<void> | null = null
const listeners = new Set<() => void>()

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

function getSnapshot() {
  return globalFM
}

function notifyListeners() {
  listeners.forEach(listener => listener())
}

function loadFramerMotion() {
  if (globalFM)
    return Promise.resolve()
  if (loadPromise)
    return loadPromise

  isLoading = true
  loadPromise = import('framer-motion')
    .then((mod) => {
      globalFM = { motion: mod.motion, AnimatePresence: mod.AnimatePresence }
      isLoading = false
      notifyListeners()
    })
    .catch(() => {
      isLoading = false
      // 忽略加载失败，保持静态渲染
    })

  return loadPromise
}

/**
 * 通用 framer-motion 按需加载工具：在确有动画需求时才动态引入
 * 使用全局单例状态，确保所有组件共享同一份加载状态
 * 返回占位元素：未加载时使用原生标签，已加载时使用 motion.*
 */
export function useLazyMotion(shouldAnimate: boolean) {
  const FM = useSyncExternalStore(subscribe, getSnapshot, getSnapshot)

  useEffect(() => {
    if (shouldAnimate && !globalFM && !isLoading) {
      loadFramerMotion()
    }
  }, [shouldAnimate])

  const MDiv: any = FM ? FM.motion.div : 'div'
  const MSpan: any = FM ? FM.motion.span : 'span'

  return { motion: FM?.motion, AnimatePresence: FM?.AnimatePresence, MDiv, MSpan } as const
}
