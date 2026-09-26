export interface IdleBreathOffset {
  angleX: number
  angleY: number
  angleZ: number
  body: number
}

export function idleBreathOffset(
  timeSeconds: number,
  output: IdleBreathOffset = { angleX: 0, angleY: 0, angleZ: 0, body: 0 },
): IdleBreathOffset {
  const time = Number.isFinite(timeSeconds) ? timeSeconds : 0
  output.angleX =
    0.13 * Math.sin(time * 0.42) + 0.05 * Math.sin(time * 1.13)
  output.angleY = 0.08 * Math.sin(time * 0.31 + 1.7)
  output.angleZ = 0.1 * Math.sin(time * 0.23 + 0.5)
  output.body = 0.16 * Math.sin(time * 0.19 + 2.1)
  return output
}
