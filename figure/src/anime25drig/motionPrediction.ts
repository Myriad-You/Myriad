/** Only already-scheduled semantic controllers use this clock. */
const DISPLAY_FRAME_SECONDS = 0.0167
const RESPONSE_LEAD_SECONDS = 0.0233

export const SCHEDULED_CONTROL_PREDICTION_SECONDS =
  DISPLAY_FRAME_SECONDS + RESPONSE_LEAD_SECONDS

export function predictedControlTime(
  timeSeconds: number,
  responseScale = 1,
): number {
  const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
  const scale =
    Number.isFinite(responseScale) && responseScale > 0 ? responseScale : 1
  return now + DISPLAY_FRAME_SECONDS + RESPONSE_LEAD_SECONDS / scale
}

export function monotonicControlTime(
  previous: number,
  timeSeconds: number,
  responseScale = 1,
): number {
  const next = predictedControlTime(timeSeconds, responseScale)
  return Number.isFinite(previous) ? Math.max(previous, next) : next
}
