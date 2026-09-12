export const TRACK_HEIGHT_PERCENT = 0.3
export const MIN_THUMB_HEIGHT = 40

export interface ScrollbarViewport {
  windowHeight: number
  documentHeight: number
  scrollTop: number
}

export interface ThumbLayout {
  scrollableHeight: number
  trackHeight: number
  trackTop: number
  thumbHeight: number
  availableTrackHeight: number
  thumbTop: number
  percentage: number
}

export interface LockedDragMetrics {
  pointerId: number
  grabOffsetY: number
  trackTop: number
  availableTrackHeight: number
  thumbHeight: number
  scrollableHeight: number
}

export function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value))
}

export function computeThumbLayout(viewport: ScrollbarViewport): ThumbLayout {
  const { windowHeight, documentHeight, scrollTop } = viewport
  const scrollableHeight = documentHeight - windowHeight
  const trackHeight = windowHeight * TRACK_HEIGHT_PERCENT
  const trackTop = (windowHeight - trackHeight) / 2

  if (scrollableHeight <= 0 || trackHeight <= 0) {
    return {
      scrollableHeight: Math.max(0, scrollableHeight),
      trackHeight,
      trackTop,
      thumbHeight: MIN_THUMB_HEIGHT,
      availableTrackHeight: 0,
      thumbTop: 0,
      percentage: 0,
    }
  }

  const viewportRatio = windowHeight / Math.max(documentHeight, 1)
  const thumbHeight = Math.max(MIN_THUMB_HEIGHT, trackHeight * viewportRatio)
  const availableTrackHeight = Math.max(1, trackHeight - thumbHeight)
  const percentage = clamp(scrollTop / scrollableHeight, 0, 1)
  const thumbTop = percentage * availableTrackHeight

  return {
    scrollableHeight,
    trackHeight,
    trackTop,
    thumbHeight,
    availableTrackHeight,
    thumbTop,
    percentage,
  }
}

export function thumbTopFromPointer(
  clientY: number,
  metrics: Pick<
    LockedDragMetrics,
    'trackTop' | 'grabOffsetY' | 'availableTrackHeight'
  >,
): number {
  return clamp(
    clientY - metrics.trackTop - metrics.grabOffsetY,
    0,
    metrics.availableTrackHeight,
  )
}

export function scrollTopFromThumb(
  thumbTop: number,
  availableTrackHeight: number,
  scrollableHeight: number,
): number {
  if (availableTrackHeight <= 0) return 0
  return clamp(
    (thumbTop / availableTrackHeight) * scrollableHeight,
    0,
    scrollableHeight,
  )
}

export function grabOffsetFromThumbPointer(
  clientY: number,
  thumbTopViewport: number,
  thumbHeight: number,
): number {
  return clamp(clientY - thumbTopViewport, 0, Math.max(thumbHeight, 1))
}
