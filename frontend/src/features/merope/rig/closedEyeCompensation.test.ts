import type { ClosedEyeCompensationPart } from './closedEyeCompensation'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  compensateSyntheticClosedEyeAngles,
  rgbaPrincipalAngleDegrees,
} from './closedEyeCompensation'

test('compensates whichever generated eye deviates from its own open eyelash', () => {
  for (const angles of [
    { closedL: -20, openL: -5, closedR: 7, openR: 6 },
    { closedL: -6, openL: -5, closedR: 20, openR: 5 },
  ]) {
    const parts = eyeParts(angles)
    const before = placement(parts)
    compensateSyntheticClosedEyeAngles(parts)
    for (const side of ['L', 'R'] as const) {
      const closed = find(parts, `eye_close_${side.toLowerCase()}`)
      const open = find(parts, `eyelash_${side.toLowerCase()}`)
      const beforeAngle = before[side].angle
      const targetAngle = angle(open)
      const error = targetAngle - beforeAngle
      const expectedCorrection =
        Math.abs(error) <= 2 ? 0 : Math.max(-10, Math.min(10, error))
      assert.ok(
        Math.abs(angle(closed) - beforeAngle - expectedCorrection) < 1,
        `${side} did not receive its own bounded correction`,
      )
      assert.ok(Math.abs(centerX(closed) - before[side].centerX) <= 0.5)
      assert.ok(Math.abs(closeY(closed) - before[side].closeY) <= 0.5)
    }
  }
})

test('leaves authored closed-eye artwork unchanged', () => {
  const authored = part('eye_close_l', 'L', -20, false)
  const image = authored.img
  const parts = [part('eyelash_l', 'L', -5, false), authored]
  compensateSyntheticClosedEyeAngles(parts)
  assert.equal(authored.img, image)
  assert.equal(angle(authored), rgbaPrincipalAngleDegrees(image))
})

function eyeParts(angles: {
  closedL: number
  openL: number
  closedR: number
  openR: number
}): ClosedEyeCompensationPart[] {
  return [
    part('eyelash_l', 'L', angles.openL, false),
    part('eyelash_r', 'R', angles.openR, false),
    part('eye_close_l', 'L', angles.closedL, true),
    part('eye_close_r', 'R', angles.closedR, true),
  ]
}

function part(
  name: string,
  side: 'L' | 'R',
  degrees: number,
  synthetic: boolean,
): ClosedEyeCompensationPart {
  return {
    name,
    side,
    synthetic,
    x: side === 'L' ? 20 : 80,
    y: 30,
    w: 41,
    h: 41,
    img: lineImage(degrees),
  }
}

function lineImage(degrees: number) {
  const width = 41
  const height = 41
  const data = new Uint8ClampedArray(width * height * 4)
  const slope = Math.tan((degrees * Math.PI) / 180)
  for (let x = 4; x < width - 4; x += 1) {
    const centerY = Math.round((height - 1) / 2 + (x - 20) * slope)
    for (let y = centerY - 1; y <= centerY + 1; y += 1) {
      const offset = (y * width + x) * 4
      data[offset] = 32
      data[offset + 1] = 24
      data[offset + 2] = 20
      data[offset + 3] = 255
    }
  }
  return { width, height, data }
}

function placement(parts: ClosedEyeCompensationPart[]) {
  return Object.fromEntries(
    (['L', 'R'] as const).map(side => {
      const closed = find(parts, `eye_close_${side.toLowerCase()}`)
      return [
        side,
        {
          angle: angle(closed),
          centerX: centerX(closed),
          closeY: closeY(closed),
        },
      ]
    }),
  ) as Record<'L' | 'R', { angle: number; centerX: number; closeY: number }>
}

function find(parts: ClosedEyeCompensationPart[], name: string) {
  return parts.find(part => part.name === name)!
}

function angle(part: ClosedEyeCompensationPart): number {
  return rgbaPrincipalAngleDegrees(part.img)!
}

function centerX(part: ClosedEyeCompensationPart): number {
  return part.x + part.w / 2
}

function closeY(part: ClosedEyeCompensationPart): number {
  return part.y + part.h * 0.55
}
