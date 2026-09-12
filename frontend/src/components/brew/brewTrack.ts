/** 不进 skin。 */

export function trackBrew(
  event: 'BREW_OPEN_ITEM' | 'BREW_STAR' | 'BREW_UNSTAR',
  target: number,
  throttleMs: number,
): void {
  void import('../../utils/analyticsEvents').then(
    ({ trackProductEvent, AnalyticsEvents }) => {
      trackProductEvent(AnalyticsEvents[event], { target, throttleMs })
    },
  )
}
