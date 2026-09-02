export interface CryMouthMotion {
  mouthOpen: number
  mouthForm: number
  mouthCY: number
  mouthScale: number
}

/** Samples a restrained sobbing mouth without allocating in the render loop. */
export function sampleCryMouthMotion(
  intensity: number,
  timeSeconds: number,
  output: CryMouthMotion,
): void {
  const amount = clamp(intensity, 0, 1)
  if (amount <= 0.001) {
    output.mouthOpen = 0
    output.mouthForm = 0
    output.mouthCY = 0
    output.mouthScale = 0
    return
  }
  const sob = 0.5 + 0.5 * Math.sin(timeSeconds * 2.55 + 0.35)
  const tremble = Math.sin(timeSeconds * 4.4 + 0.3)
  const uneven = Math.sin(timeSeconds * 2.15 + 1.1)
  output.mouthOpen = amount * (0.415 + sob * 0.055 + tremble * 0.018)
  output.mouthForm =
    amount * (-0.58 + Math.sin(timeSeconds * 3.2 + 0.8) * 0.022)
  output.mouthCY =
    amount * (uneven * 0.012 + Math.sin(timeSeconds * 5.9 + 1.6) * 0.004)
  output.mouthScale = amount * (0.1 + sob * 0.025)
}

export function cryTearVerticalOffset(
  timeSeconds: number,
  side: 'L' | 'R' | null,
  intensity: number,
  faceScale: number,
): number {
  const phase = side === 'L' ? 0.35 : 2.05
  const flow =
    1.8 +
    Math.sin(timeSeconds * 2.15 + phase) * 0.75 +
    Math.sin(timeSeconds * 0.83 + phase * 1.7) * 0.35
  return flow * clamp(intensity, 0, 1) * Math.max(0, faceScale)
}

export function cryTearHorizontalOffset(
  timeSeconds: number,
  side: 'L' | 'R' | null,
  intensity: number,
  faceScale: number,
): number {
  const phase = side === 'L' ? 0.35 : 2.05
  const drift =
    Math.sin(timeSeconds * 1.27 + phase) +
    Math.sin(timeSeconds * 0.61 + phase * 1.4) * 0.35
  return drift * 0.45 * clamp(intensity, 0, 1) * Math.max(0, faceScale)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}
