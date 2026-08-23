import assert from 'node:assert/strict'
import test from 'node:test'
import { frontHairUpperParallaxScale } from './hairPhysics'

test('reduces only composite front-hair upper parallax and preserves its lower locks', () => {
  const face = { y0: 170, y1: 658 }
  const composite = { y: 113, h: 774 }
  const compact = { y: 113, h: 630 }
  const at = (progress: number) => composite.y + composite.h * progress

  assert.ok(
    Math.abs(frontHairUpperParallaxScale(at(0.3), composite, face) - 0.2) <
      1e-9,
  )
  assert.ok(
    Math.abs(frontHairUpperParallaxScale(at(0.45), composite, face) - 0.2) <
      1e-9,
  )
  assert.ok(
    Math.abs(frontHairUpperParallaxScale(at(0.6), composite, face) - 0.6) <
      1e-9,
  )
  assert.equal(frontHairUpperParallaxScale(at(0.75), composite, face), 1)
  assert.equal(frontHairUpperParallaxScale(at(0.3), compact, face), 1)
})
