// 0×0 或入场 transform 不能当离屏，否则会永久 hold。

export const TAPP_WIDGET_VIEWPORT_RECHECK_MS = 400
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
