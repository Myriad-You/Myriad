/**
 * Nav island idle-hide / edge-reveal helpers.
 *
 * The rail lives inside the proximity band (desktop: left 100px; mobile: bottom
 * 100px). Treating every mousemove in that band as activity makes idle hide
 * impossible — optical jitter keeps restarting the timer, and the first jitter
 * after hide immediately shows the island again.
 *
 * Reveal is rising-edge only: leave the band and re-enter, or push into the
 * thin hot edge (macOS-dock style). The first pointer sample only seeds
 * occupancy — it is not an enter. Hover on the island itself still pauses
 * hide; that is handled in the hook, not here.
 */

import type { NavLayout } from './navLayout'

export const NAV_EDGE_THRESHOLD = 100
export const NAV_HOT_EDGE_THRESHOLD = 4
export const NAV_SCROLL_DOWN_THRESHOLD = 50
export const NAV_PAGE_TOP_THRESHOLD = 100

/** Desktop: left of `threshold`. Mobile: below `windowHeight - threshold`. */
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

/**
 * Scroll should hide/show the island, but trackpad noise and rubber-banding at
 * the page top must not refresh the idle timer — most pages live in that zone.
 */
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
