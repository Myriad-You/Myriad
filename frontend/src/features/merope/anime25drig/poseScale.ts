/** Head roll about the neck at `angleZ` = ±1, in radians. */
export const HEAD_ROLL_RADIANS = 0.21

/** Upper-body lean at `body` = ±1, in radians; the canvas cut stays put. */
export const BODY_ROLL_RADIANS = 0.07

/**
 * How much of the lean a point at `y` takes: none on the canvas cut at
 * `pivotY`, all of it from `bendHeight` above. Mirrors the vertex shader.
 */
export function bodyLeanShare(y: number, pivotY: number, bendHeight: number): number {
  if (!(bendHeight > 0)) return 1
  const t = Math.max(0, Math.min(1, (pivotY - y) / bendHeight))
  return t * t * (3 - 2 * t)
}
