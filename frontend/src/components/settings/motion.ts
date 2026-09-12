/* keep numeric values in sync with settings-motion.css */

export const SETTINGS_DURATION = {
  instant: 0.09,
  fast: 0.14,
  base: 0.22,
  slow: 0.32,
} as const

export const SETTINGS_DURATION_MS = {
  instant: 90,
  fast: 140,
  base: 220,
  slow: 320,
} as const

export const SETTINGS_EASE = {
  standard: [0.32, 0.72, 0, 1],
  enter: [0.16, 1, 0.3, 1],
  exit: [0.4, 0, 1, 1],
  emphasis: [0.22, 1, 0.36, 1],
  spring: [0.34, 1.4, 0.64, 1],
} as const

export const SETTINGS_PAGE_MOTION = {
  initial: { opacity: 0, y: 12 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, y: -8 },
  transition: {
    duration: SETTINGS_DURATION.slow,
    ease: SETTINGS_EASE.enter,
  },
} as const

export const SETTINGS_SIDEBAR_MOTION = {
  initial: { opacity: 0, x: -8 },
  animate: { opacity: 1, x: 0 },
  transition: {
    duration: SETTINGS_DURATION.slow,
    ease: SETTINGS_EASE.enter,
    delay: 0.04,
  },
} as const

/** zero wait-before-unmount; reduced-motion CSS is 1ms, timeouts would still be 320ms */
export function prefersReducedMotion(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}
