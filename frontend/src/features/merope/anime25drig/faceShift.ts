import type { FaceShift } from '../events/performanceContract'
import type { BodyControl } from '../performance/types'
import { BODY_CONTROLS } from '../events/performanceContract'
import { mappedControlValue } from './actingField'

/**
 * The order a change of face travels through her: the eyes go first, the
 * brows and mouth with them, the head after, the body last.
 */
function stage(control: BodyControl): number {
  if (control.startsWith('gaze')) return 0
  if (control.startsWith('eye') || control === 'cry' || control === 'dizzy') return 1
  if (control.startsWith('head')) return 3
  if (control.startsWith('torso') || control.includes('Arm') || control.endsWith('Sway')) return 4
  return 2
}

/**
 * The head, body and eyes' aim carry momentum: they may go past their goal
 * and come back, wind up first, and travel in arcs. The lids, brows and
 * mouth only open and close: they reach their goal and stop.
 */
export function carriesMomentum(control: BodyControl): boolean {
  const order = stage(control)
  return order === 0 || order === 3 || order === 4
}

/**
 * A move along one control bows another off the straight line, at its
 * middle: a turning head dips, a nod tips the head aside a little, a look
 * across sinks slightly (the control bowed, and by what share of the move).
 */
export const ARCS: Partial<Record<BodyControl, { onto: BodyControl; share: number; sided?: true }>> = {
  headTurn: { onto: 'headNod', share: -0.45 },
  headNod: { onto: 'headTilt', share: 0.35, sided: true },
  gazeHorizontal: { onto: 'gazeVertical', share: -0.25 },
  torsoTurn: { onto: 'torsoRise', share: -0.3 },
}

/**
 * When a control sets off from where it is (`from`, a driver value) toward
 * its new goal (`to`), and how quickly it gets there as a spring rate: a far
 * way takes longer than a short one.
 */
export function shiftTiming(
  shift: Readonly<FaceShift>,
  control: BodyControl,
  from: number,
  to: number,
): { delay: number; omega: number } {
  const limits = BODY_CONTROLS[control]
  const span = Math.abs(mappedControlValue(control, limits.max) - mappedControlValue(control, limits.min)) || 1
  const share = Math.min(1, Math.abs(to - from) / span)
  const seconds = (shift.durationMs / 1000) * (0.6 + 0.8 * share)
  return {
    delay: (shift.pauseMs + stage(control) * shift.staggerMs) / 1000,
    omega: 6 / seconds,
  }
}
