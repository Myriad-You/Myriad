/** Head roll about the neck per unit of `angleZ` in everyday motion, in radians. */
export const HEAD_ROLL_RADIANS = 0.15

/**
 * The head's full tilt at `angleZ` = ±1, about 24°: a rigger's AngleZ range.
 * Everyday motion stays below `HEAD_TILT_KNEE` and keeps its old, linear
 * roll; only a directed tilt goes past it, curving up to the full tilt.
 */
export const HEAD_TILT_RADIANS = 0.42
const HEAD_TILT_KNEE = 0.45

/** Head roll about the neck at `angleZ`, in radians. */
export function headRollRadians(angleZ: number): number {
  const amount = Math.abs(angleZ)
  const past = Math.max(0, (Math.min(1, amount) - HEAD_TILT_KNEE) / (1 - HEAD_TILT_KNEE))
  return Math.sign(angleZ) * (amount * HEAD_ROLL_RADIANS + past * past * (HEAD_TILT_RADIANS - HEAD_ROLL_RADIANS))
}

/** The `angleZ` that rolls the head by `radians` (headRollRadians undone), up to the full tilt. */
export function angleZForHeadRoll(radians: number): number {
  const roll = Math.min(HEAD_TILT_RADIANS, Math.abs(radians))
  const knee = HEAD_TILT_KNEE * HEAD_ROLL_RADIANS
  if (roll <= knee) return Math.sign(radians) * roll / HEAD_ROLL_RADIANS
  // roll = knee + linear·p + curve·p², p the share of the way past the knee.
  const linear = (1 - HEAD_TILT_KNEE) * HEAD_ROLL_RADIANS
  const curve = HEAD_TILT_RADIANS - HEAD_ROLL_RADIANS
  const past = (-linear + Math.sqrt(linear * linear + 4 * curve * (roll - knee))) / (2 * curve)
  return Math.sign(radians) * (HEAD_TILT_KNEE + (1 - HEAD_TILT_KNEE) * past)
}

/** Upper-body lean at `body` = ±1, in radians; the canvas cut stays put. */
export const BODY_ROLL_RADIANS = 0.05

/**
 * How much of the lean a point at `y` takes: none on the canvas cut at
 * `pivotY`, all of it from `bendHeight` above. Mirrors the vertex shader.
 */
export function bodyLeanShare(y: number, pivotY: number, bendHeight: number): number {
  if (!(bendHeight > 0)) return 1
  const t = Math.max(0, Math.min(1, (pivotY - y) / bendHeight))
  return t * t * (3 - 2 * t)
}
