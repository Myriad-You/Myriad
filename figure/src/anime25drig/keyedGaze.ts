import type { Anime25DTurnKeyforms, BoundTurnKeyform } from './turnKeyforms'
import type { Anime25DEyeAnchor, Anime25DPlaybackLayer } from './types'
import { bindTurnKeyform, turnKeyformOffset } from './turnKeyforms'

/** How far a full `eyeX` / `eyeY` moves the iris at rest, in face-scale px. */
export const GAZE_REACH_X = 11
export const GAZE_REACH_Y = 6

/**
 * The gaze of a keyed head. The turned drawings already look somewhere (at the
 * viewer, so a turned or lowered head's irises sit toward a corner of the
 * white): that look is the key's, and the runtime gaze adds to it. Both
 * together reach no further across the white than the gaze does at rest, and
 * the gaze travels as far as the white is wide this frame (a far eye
 * foreshortened to half its width moves its iris half as far).
 */
export interface KeyedGaze {
  /** The eye's key at the white's centre, left, right, top and bottom (rest points). */
  white: BoundTurnKeyform
  /** The iris's key (its own, else the eye's) at the iris centre. */
  iris: BoundTurnKeyform
  rest: { cx: number; cy: number; hx: number; hy: number; ix: number; iy: number }
  /** This frame's gaze move of the iris, px (keyedGazeShift). */
  shift: { x: number; y: number }
}

/** Binds an iris layer's keyed gaze; null when the head has no key for this eye. */
export function bindKeyedGaze(
  keyforms: Readonly<Anime25DTurnKeyforms> | undefined,
  source: Pick<Anime25DPlaybackLayer, 'group' | 'role' | 'side'> & { name?: string },
  eye: Readonly<Anime25DEyeAnchor>,
): KeyedGaze | null {
  const cx = (eye.x0 + eye.x1) / 2
  const cy = (eye.y0 + eye.y1) / 2
  const white = bindTurnKeyform(
    keyforms,
    { group: source.group, role: 'eyewhite', side: source.side },
    Float32Array.of(cx, cy, eye.x0, cy, eye.x1, cy, cx, eye.y0, cx, eye.y1),
  )
  const iris = bindTurnKeyform(keyforms, source, Float32Array.of(eye.icx, eye.icy))
  if (!white || !iris) return null
  return {
    white,
    iris,
    rest: {
      cx,
      cy,
      hx: Math.max(1, (eye.x1 - eye.x0) / 2),
      hy: Math.max(1, (eye.y1 - eye.y0) / 2),
      ix: eye.icx,
      iy: eye.icy,
    },
    shift: { x: 0, y: 0 },
  }
}

const probe = { x: 0, y: 0 }

/** Where rest point `vertex` of a bound key is at this turn and nod. */
function keyed(bound: Readonly<BoundTurnKeyform>, vertex: number, x: number, y: number, amount: number, nod: number) {
  turnKeyformOffset(bound, vertex, amount, probe, nod)
  return { x: x + probe.x, y: y + probe.y }
}

/** The gaze move along one axis, in rest px: the key's look plus `wanted`, kept within ±reach of the rest look. */
function room(restLook: number, keyLook: number, wanted: number, reach: number): number {
  // The key's own look is never undone; the gaze only cannot push past it.
  const lo = Math.min(0, restLook - reach - keyLook)
  const hi = Math.max(0, restLook + reach - keyLook)
  return Math.max(lo, Math.min(hi, wanted))
}

/** Writes `gaze.shift`: the iris's move for `eyeX` / `eyeY` at this turn and nod. */
export function keyedGazeShift(
  gaze: KeyedGaze,
  amount: number,
  nod: number,
  eyeX: number,
  eyeY: number,
  faceScale: number,
): void {
  const { rest } = gaze
  const center = keyed(gaze.white, 0, rest.cx, rest.cy, amount, nod)
  const left = keyed(gaze.white, 1, rest.cx - rest.hx, rest.cy, amount, nod)
  const right = keyed(gaze.white, 2, rest.cx + rest.hx, rest.cy, amount, nod)
  const top = keyed(gaze.white, 3, rest.cx, rest.cy - rest.hy, amount, nod)
  const bottom = keyed(gaze.white, 4, rest.cx, rest.cy + rest.hy, amount, nod)
  const iris = keyed(gaze.iris, 0, rest.ix, rest.iy, amount, nod)
  // The white's size this frame over its size at rest, per axis.
  const sx = Math.max(0.05, Math.abs(right.x - left.x) / (2 * rest.hx))
  const sy = Math.max(0.05, Math.abs(bottom.y - top.y) / (2 * rest.hy))
  // Where the iris sits in the white, in rest px: at rest, and as keyed.
  const lookX = (iris.x - center.x) / sx
  const lookY = (iris.y - center.y) / sy
  const x = room(rest.ix - rest.cx, lookX, eyeX * GAZE_REACH_X * faceScale, GAZE_REACH_X * faceScale)
  const y = room(rest.iy - rest.cy, lookY, eyeY * GAZE_REACH_Y * faceScale, GAZE_REACH_Y * faceScale)
  gaze.shift.x = x * sx
  gaze.shift.y = y * sy
}
