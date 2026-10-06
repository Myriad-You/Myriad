import type { HeadTurn } from './headTurn'
import type { BoundTurnKeyform } from './turnKeyforms'
import { headTurnNeckMove } from './headTurn'
import { addKeyedTurn } from './turnKeyforms'

/**
 * Moves a point below the head (the neck, a collar on it) with the head. A
 * keyed neck takes the shape measured on the turned pictures, across and up
 * and down; the roll above and the body below still carry it. Otherwise the
 * point takes the flat carry (`carriedX/Y`), and the top of the neck goes
 * with the chin instead, across and up and down, faded down the neck by
 * `blend`: the jaw never leaves the neck, and the shadow drawn under the chin
 * rises and falls with it.
 */
export function followHeadBelow(
  point: { x: number; y: number },
  keyform: Readonly<BoundTurnKeyform> | null | undefined,
  turn: Readonly<HeadTurn> | undefined,
  vertex: number,
  x: number,
  y: number,
  carriedX: number,
  carriedY: number,
  blend: number,
): void {
  if (keyform && turn) {
    addKeyedTurn(point, keyform, vertex, turn.amount, turn.nodAmount)
    return
  }
  const carryX = carriedX - point.x
  const carryY = carriedY - point.y
  point.x = carriedX
  point.y = carriedY
  if (!(blend > 0) || !turn?.active) return
  const jaw = headTurnNeckMove(turn, x, y)
  point.x += (jaw.x - carryX) * blend
  point.y += (jaw.y - carryY) * blend
}
