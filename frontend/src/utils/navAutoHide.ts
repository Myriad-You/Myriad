/** Proximity: desktop left 100px; mobile bottom 100px. */

import type { NavLayout } from './navLayout'

export const NAV_EDGE_THRESHOLD = 100
export const NAV_HOT_EDGE_THRESHOLD = 4
export const NAV_SCROLL_DOWN_THRESHOLD = 50
export const NAV_PAGE_TOP_THRESHOLD = 100

export function isNearNavEdge(
  layout: NavLayout,
  clientX: number,
  clientY: number,
  windowHeight: number,
  threshold = NAV_EDGE_THRESHOLD,
): boolean {
  return layout === 'desktop'
    ? clientX < threshold
    : clientY > windowHeight - threshold
}

export function edgeRevealShouldShow(opts: {
  visible: boolean
  primed: boolean
  wasInsideProximity: boolean
  isInsideProximity: boolean
  wasInsideHot: boolean
  isInsideHot: boolean
}): {
  show: boolean
  primed: boolean
  insideProximity: boolean
  insideHot: boolean
} {
  if (!opts.primed) {
    return {
      show: false,
      primed: true,
      insideProximity: opts.isInsideProximity,
      insideHot: opts.isInsideHot,
    }
  }
  const enteredProximity =
    opts.isInsideProximity && !opts.wasInsideProximity
  const enteredHot = opts.isInsideHot && !opts.wasInsideHot
  return {
    show: !opts.visible && (enteredProximity || enteredHot),
    primed: true,
    insideProximity: opts.isInsideProximity,
    insideHot: opts.isInsideHot,
  }
}

/** Trackpad/rubber-band at top must not refresh the idle timer. */
export function navScrollDecision(
  currentY: number,
  lastY: number,
): 'hide' | 'show' | 'none' {
  const delta = currentY - lastY
  if (delta > NAV_SCROLL_DOWN_THRESHOLD && currentY > NAV_PAGE_TOP_THRESHOLD) {
    return 'hide'
  }
  if (delta < -NAV_SCROLL_DOWN_THRESHOLD) return 'show'
  return 'none'
}
