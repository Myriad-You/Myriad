/**
 * Compensates one display frame plus part of the final driver response.
 *
 * Only already-scheduled semantic controllers use this clock. Ambient motion,
 * pointer tracking and physical secondary motion stay on observed time, so an
 * incorrect prediction can change an envelope but cannot inject fake motion.
 */
export const SCHEDULED_CONTROL_PREDICTION_SECONDS = 0.04

export function predictedControlTime(timeSeconds: number): number {
  const now = Number.isFinite(timeSeconds) ? Math.max(0, timeSeconds) : 0
  return now + SCHEDULED_CONTROL_PREDICTION_SECONDS
}
