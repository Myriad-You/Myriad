import assert from 'node:assert/strict'
import test from 'node:test'
import {
  createDizzyEyeBitmap,
  dizzyEyeDisplayScale,
  dizzyEyeGeneratedSize,
  sampleDizzyEyeTint,
} from './dizzyEye'

test('generates independent mirrored spiral artwork with bounded RGBA data', () => {
  const tint = { red: 42, green: 31, blue: 68 }
  const left = createDizzyEyeBitmap(48, tint, 'left')
  const right = createDizzyEyeBitmap(48, tint, 'right')
  assert.equal(left.width, 48)
  assert.equal(left.height, 48)
  assert.equal(left.data.length, 48 * 48 * 4)
  assert.notDeepEqual(left.data, right.data)
  const visible = left.data.filter(
    (_, index) => index % 4 === 3 && left.data[index] > 0,
  )
  assert.ok(visible.length > 150)
  assert.ok(visible.length < 48 * 48 * 0.5)
  for (let index = 0; index < left.data.length; index += 4) {
    if (left.data[index + 3] === 0) continue
    assert.equal(left.data[index], tint.red)
    assert.equal(left.data[index + 1], tint.green)
    assert.equal(left.data[index + 2], tint.blue)
  }
})

test('samples the dark eyelash color instead of transparent or bright pixels', () => {
  const pixels = new Uint8ClampedArray([
    255, 255, 255, 0, 230, 220, 210, 255, 36, 24, 52, 255, 40, 28, 56, 255,
  ])
  const tint = sampleDizzyEyeTint(pixels)
  assert.ok(tint.red < 60)
  assert.ok(tint.green < 50)
  assert.ok(tint.blue < 75)
})

test('generated dizzy eyes follow the longer eyewhite span', () => {
  assert.equal(
    dizzyEyeGeneratedSize({ x0: 10, x1: 90, y0: 20, y1: 52 }),
    Math.round(80 * 0.92),
  )
})

test('display scale enlarges undersized spirals and leaves fitted artwork alone', () => {
  const eye = { x0: 10, x1: 90, y0: 20, y1: 52 }
  const generated = dizzyEyeDisplayScale(28, 28, eye)
  assert.ok(generated > 1.5)
  assert.ok(generated <= 2.2)
  assert.equal(dizzyEyeDisplayScale(74, 74, eye), 1)
})
