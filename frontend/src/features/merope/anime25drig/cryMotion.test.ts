import assert from 'node:assert/strict'
import test from 'node:test'
import {
  cryTearHorizontalOffset,
  cryTearVerticalOffset,
  sampleCryMouthMotion,
} from './cryMotion'

test('cry mouth keeps a sad form while opening with a restrained tremble', () => {
  const output = { mouthOpen: 0, mouthForm: 0, mouthCY: 0, mouthScale: 0 }
  let minimumOpen = 1
  let maximumOpen = 0
  let minimumForm = 0
  let minimumScale = 1
  let maximumScale = 0
  for (let frame = 0; frame < 360; frame += 1) {
    sampleCryMouthMotion(1, frame / 60, output)
    minimumOpen = Math.min(minimumOpen, output.mouthOpen)
    maximumOpen = Math.max(maximumOpen, output.mouthOpen)
    minimumForm = Math.min(minimumForm, output.mouthForm)
    minimumScale = Math.min(minimumScale, output.mouthScale)
    maximumScale = Math.max(maximumScale, output.mouthScale)
    assert.ok(Math.abs(output.mouthCY) <= 0.0161)
  }
  assert.ok(minimumOpen > 0.39)
  assert.ok(maximumOpen < 0.5)
  assert.ok(maximumOpen - minimumOpen > 0.075)
  assert.ok(minimumForm < -0.59)
  assert.ok(minimumScale >= 0.1)
  assert.ok(maximumScale <= 0.1251)
})

test('tear flow is slow, bounded, and independently phased per eye', () => {
  let sideDifference = 0
  for (let frame = 0; frame < 360; frame += 1) {
    const time = frame / 60
    const leftY = cryTearVerticalOffset(time, 'L', 1, 1)
    const rightY = cryTearVerticalOffset(time, 'R', 1, 1)
    const leftX = cryTearHorizontalOffset(time, 'L', 1, 1)
    assert.ok(leftY >= 0.69 && leftY <= 2.91)
    assert.ok(Math.abs(leftX) <= 0.61)
    sideDifference += Math.abs(leftY - rightY)
  }
  assert.ok(sideDifference > 100)
  assert.equal(cryTearVerticalOffset(1, 'L', 0, 1), 0)
})

test('zero crying intensity resets a reused mouth sample', () => {
  const output = { mouthOpen: 1, mouthForm: 1, mouthCY: 1, mouthScale: 1 }
  sampleCryMouthMotion(0, 1, output)
  assert.deepEqual(output, {
    mouthOpen: 0,
    mouthForm: 0,
    mouthCY: 0,
    mouthScale: 0,
  })
})
