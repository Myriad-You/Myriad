import assert from 'node:assert/strict'
import test from 'node:test'
import { writeAnime25DLayerGlobalTransform } from './layerTransform'

test('matches the legacy head, parallax, special-offset and breath sequence', () => {
  const input = {
    headFollow: 0.73,
    headRotationCosine: Math.cos(0.18),
    headRotationSine: Math.sin(0.18),
    neckPivotX: 384,
    neckPivotY: 520,
    faceScale: 1.2,
    angleX: -0.45,
    angleY: 0.31,
    depthOffset: 0.17,
    faceCenterY: 310,
    specialOffsetY: 4.2,
    breathOffset: 1.3,
  }
  const matrix = writeAnime25DLayerGlobalTransform(input, new Float32Array(9))
  for (const [x, y] of [
    [120, 80],
    [384, 520],
    [710, 930],
  ]) {
    const expected = legacyTransform(x, y, input)
    const actual = {
      x: matrix[0] * x + matrix[3] * y + matrix[6],
      y: matrix[1] * x + matrix[4] * y + matrix[7],
    }
    assert.ok(Math.abs(actual.x - expected.x) < 0.0001)
    assert.ok(Math.abs(actual.y - expected.y) < 0.0001)
  }
})

function legacyTransform(
  xInput: number,
  yInput: number,
  input: Parameters<typeof writeAnime25DLayerGlobalTransform>[0],
): { x: number; y: number } {
  let x = xInput
  let y = yInput
  const rotationX = x - input.neckPivotX
  const rotationY = y - input.neckPivotY
  const rotatedX =
    rotationX * input.headRotationCosine - rotationY * input.headRotationSine
  const rotatedY =
    rotationX * input.headRotationSine + rotationY * input.headRotationCosine
  x += (rotatedX - rotationX) * input.headFollow
  y += (rotatedY - rotationY) * input.headFollow
  x +=
    input.headFollow *
    input.faceScale *
    (input.angleX * (14 + 40 * input.depthOffset) +
      input.angleX * (input.neckPivotY - y) * 0.028)
  y +=
    input.headFollow *
    input.faceScale *
    (-input.angleY * (9 + 30 * input.depthOffset) -
      input.angleY * input.depthOffset * (y - input.faceCenterY) * 0.05)
  y += input.specialOffsetY
  y -= input.breathOffset * input.faceScale
  return { x, y }
}
