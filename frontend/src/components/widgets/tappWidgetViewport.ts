/**
 * Viewport gate for home Tapp tiles.
 *
 * IntersectionObserver can report 0×0 (or a transformed entrance pose) before
 * the grid cell has a real box. Treating that as off-screen unmounts the
 * sandbox; with no later scroll/resize, the tile stays on the hold surface.
 * Cached chunks make this first callback more likely (host mounts before
 * layout). Disable-cache F5 delays mount until geometry is stable.
 */

export const TAPP_WIDGET_VIEWPORT_RECHECK_MS = 400
/** Rechecks after a laid-out non-intersection before adopting off-screen. */
export const TAPP_WIDGET_VIEWPORT_OFFSCREEN_RECHECKS = 3

export function intersectionKeepsTappWidgetMounted(entry: {
  isIntersecting: boolean
  boundingClientRect: { width: number; height: number }
}): boolean {
  if (
    entry.boundingClientRect.width < 1 ||
    entry.boundingClientRect.height < 1
  ) {
    return true
  }
  return entry.isIntersecting
}
