/**
 * Compensates one display frame plus part of the final driver response.
 *
 * Only already-scheduled semantic controllers use this clock. Ambient motion,
 * pointer tracking and physical secondary motion stay on observed time, so an
 * incorrect prediction can change an envelope but cannot inject fake motion.
 *
 * The display half is fixed; the response half is not. `poseResponseScale`
 * moves the pose filter's bandwidth with the delivery's manner, so a quick,
 * direct beat arrives sooner and needs less lead than a fluid one. A single
 * constant under-compensated the first and over-compensated the second.
 */
const DISPLAY_FRAME_SECONDS = 0.0167
const RESPONSE_LEAD_SECONDS = 0.0233

/** Lead for a delivery moving at the rig's own rate. */
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

/**
 * The read clock scheduled controllers share, held forward.
 *
 * With a fixed lead this could not regress. It is no longer fixed: a lead that
 * shrinks by more than one frame's own length would hand every scheduled
 * controller a time earlier than the last one, stepping every envelope
 * backwards at once.
 */
export function monotonicControlTime(
  previous: number,
  timeSeconds: number,
  responseScale = 1,
): number {
  const next = predictedControlTime(timeSeconds, responseScale)
  return Number.isFinite(previous) ? Math.max(previous, next) : next
}
