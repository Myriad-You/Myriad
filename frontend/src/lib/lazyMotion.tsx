import { useEffect, useSyncExternalStore } from 'react'

let globalFM: { motion: any; AnimatePresence: any } | null = null
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
  listeners.forEach((listener) => listener())
}

function loadFramerMotion() {
  if (globalFM) return Promise.resolve()
  if (loadPromise) return loadPromise

  isLoading = true
  loadPromise = import('motion/react')
    .then((mod) => {
      globalFM = { motion: mod.motion, AnimatePresence: mod.AnimatePresence }
      isLoading = false
      notifyListeners()
    })
    .catch(() => {
      isLoading = false
      // Stay static if motion fails to load.
    })

  return loadPromise
}

/** Await before staggered mount; driving canAnimate on the shim flashes every card but the first. */
export function ensureMotionReady(): Promise<void> {
  return loadFramerMotion()
}

export function isMotionReady(): boolean {
  return globalFM !== null
}

export function useLazyMotion(shouldAnimate: boolean) {
  const FM = useSyncExternalStore(subscribe, getSnapshot, getSnapshot)

  useEffect(() => {
    if (shouldAnimate && !globalFM && !isLoading) {
      loadFramerMotion()
    }
  }, [shouldAnimate])

  const MDiv: any = FM ? FM.motion.div : 'div'
  const MSpan: any = FM ? FM.motion.span : 'span'

  return {
    motion: FM?.motion,
    AnimatePresence: FM?.AnimatePresence,
    MDiv,
    MSpan,
  } as const
}
