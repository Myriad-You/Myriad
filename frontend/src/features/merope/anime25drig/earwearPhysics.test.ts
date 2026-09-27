import type { Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { bindEarwearPhysics, EarwearPhysics } from './earwearPhysics'

const anchor = { x: 30, y: 40 }
test('silhouette gate uses occupied pixels, rejects studs and unresolved pairs, and preserves explicit physics', () => {
  const source = {
    role: 'earwear',
    group: 'head',
    side: 'L',
    x: 0,
    y: 0,
    w: 200,
    h: 200,
    phys: null,
    fade: null,
  } as Anime25DPlaybackLayer
  const art = (w: number, h: number) => {
    const pixels = new Uint8ClampedArray(200 * 200 * 4)
    for (let y = 10; y < 10 + h; y++) {
      for (let x = 10; x < 10 + w; x++) pixels[(y * 200 + x) * 4 + 3] = 255
}
    return { width: 200, height: 200, pixels }
  }
  assert.ok(bindEarwearPhysics(source, { y: 12 }, art(20, 100), 450))
  assert.equal(bindEarwearPhysics(source, { y: 12 }, art(20, 20), 450), null)
  assert.equal(bindEarwearPhysics(source, { y: 12 }, art(180, 100), 450), null)
  assert.equal(
    bindEarwearPhysics(
      { ...source, role: 'neckwear' },
      { y: 12 },
      art(20, 100),
      450,
    ),
    null,
  )
  assert.equal(
    bindEarwearPhysics(
      { ...source, phys: 'hair' },
      { y: 12 },
      art(20, 100),
      450,
    ),
    null,
  )
  assert.equal(bindEarwearPhysics(source, null, art(20, 100), 450), null)
  assert.equal(bindEarwearPhysics(source, { y: 12 }, null, 450), null)
})
function matrix(x = 0, roll = 0) {
  const c = Math.cos(roll)
  const s = Math.sin(roll)
  return new Float32Array([c, s, 0, -s, c, 0, x, 0, 1])
}

test('pendant has no invented idle motion, and disabling physics preserves the exact host matrix', () => {
  const p = new EarwearPhysics(100, 450, 1)
  for (let i = 0; i < 300; i++) {
    const m = matrix()
    p.apply(anchor, m, i / 60, 0, 0, true)
    assert.deepEqual(m, matrix())
  }
  const m = matrix(70, 0.4)
  const before = m.slice()
  p.apply(anchor, m, 5, 1, 1, false)
  assert.deepEqual(m, before)
})

test('moving support excites visible swing, fixed root, bounded projection and damped settling', () => {
  const p = new EarwearPhysics(100, 450, 1)
  let peak = 0
  let tail = 0
  for (let i = 0; i <= 720; i++) {
    const t = i / 60
    const x = t < 2 ? 40 * Math.sin(Math.PI * t) : 0
    const m = matrix(x)
    p.apply(anchor, m, t, t < 2 ? Math.sin(Math.PI * t) : 0, 0, true)
    assert.ok(
      Math.abs(m[0] * anchor.x + m[3] * anchor.y + m[6] - anchor.x - x) < 1e-4,
    )
    assert.ok(
      Math.abs(m[1] * anchor.x + m[4] * anchor.y + m[7] - anchor.y) < 1e-4,
    )
    assert.ok(m[0] * m[4] - m[1] * m[3] > 0.75)
    assert.ok(Math.hypot(m[3], m[4]) <= 1.000001)
    if (t > 2 && t < 3) peak = Math.max(peak, Math.abs(m[3]) * 100)
    tail = Math.abs(m[3]) * 100
  }
  assert.ok(peak > 5, `stop must leave visible residual swing: ${peak}`)
  assert.ok(tail < 0.01)
})

test('30/60/120 Hz sample the same physical response closely', () => {
  const results = [30, 60, 120].map((fps) => {
    const p = new EarwearPhysics(100, 450, -1)
    const samples: number[] = []
    for (let i = 0; i <= fps * 6; i++) {
      const t = i / fps
      const m = matrix(30 * (1 - Math.cos(t * 3)))
      p.apply(anchor, m, t, 0.5 * Math.sin(t * 2), 0.2 * Math.sin(t), true)
      if (i % (fps / 2) === 0) samples.push(m[3])
    }
    return samples
  })
  for (const samples of results) {
    for (let i = 0; i < samples.length; i++) {
      assert.ok(Math.abs(samples[i] - results[2][i]) < 0.035)
    }
  }
})

test('depth changes foreshorten without flipping; pause resets energy and repeated renders do not step', () => {
  const p = new EarwearPhysics(100, 450, 1)
  let minimum = 1
  for (let i = 0; i < 120; i++) {
    const m = matrix()
    p.apply(anchor, m, i / 60, Math.sin(i / 20), Math.sin(i / 10), true)
    minimum = Math.min(minimum, m[0] * m[4] - m[1] * m[3])
    const repeat = matrix()
    p.apply(anchor, repeat, i / 60, Math.sin(i / 20), Math.sin(i / 10), true)
    assert.deepEqual(repeat, m)
  }
  assert.ok(minimum < 0.98)
  const m = matrix()
  p.apply(anchor, m, 20, 0, 0, true)
  assert.deepEqual(m, matrix())
})
