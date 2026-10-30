import type { Anime25DPart, Anime25DPartProfile, Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { bendHanging, bindHangBend, bindHangingPhysics } from './earwearPhysics'
import { movingPartIn, PENDULUM_DEFAULT, pendulumTuning, skirtTuning } from './partPhysics'
import { splitHangingParts } from './partSplit'

function part(patch: Partial<Anime25DPart>): Anime25DPart {
  return {
    name: 'charm',
    layer: 'recovered-objects',
    separate: true,
    box: [100, 400, 140, 520],
    anchor: [120, 400],
    bone: 'chest',
    motion: 'dangle',
    material: 'metal',
    stiffness: 0.5,
    weight: 0.5,
    ...patch,
  }
}

const profile = (parts: Anime25DPart[]): Anime25DPartProfile => ({ version: 1, source: 'ai-vision', parts })

test('a part swings by what it is made of', () => {
  const stiff = pendulumTuning(part({ stiffness: 1 }))
  const limp = pendulumTuning(part({ stiffness: 0 }))
  assert.ok(stiff.stiffness > limp.stiffness, 'stiff swings back faster')
  assert.ok(stiff.reach < limp.reach, 'and not as far')
  const ribbon = pendulumTuning(part({ motion: 'flutter', material: 'ribbon', stiffness: 0.1, weight: 0.1 }))
  const metal = pendulumTuning(part({ material: 'metal', stiffness: 0.8, weight: 0.9 }))
  assert.ok(ribbon.reach > metal.reach && ribbon.stiffness < metal.stiffness && ribbon.damping < metal.damping)
  const organza = skirtTuning(part({ layer: 'bottomwear', motion: 'sway', stiffness: 0.9 }))!
  const chiffon = skirtTuning(part({ layer: 'bottomwear', motion: 'sway', stiffness: 0.1 }))!
  assert.ok(organza.frequency > chiffon.frequency)
  assert.equal(skirtTuning(part({ motion: 'rigid' })), null)
  assert.equal(skirtTuning(null), null)
})

test('only a part that moves, drawn in the layer, is found', () => {
  const found = profile([part({ motion: 'rigid' }), part({ name: 'chain' })])
  assert.equal(movingPartIn(found, 'recovered-objects')?.name, 'chain')
  assert.equal(movingPartIn(found, 'topwear'), null)
  assert.equal(movingPartIn(undefined, 'recovered-objects'), null)
})

test('a hanging part swings from its attachment; without an inventory an earring keeps its own rule', () => {
  const anchors = { face: { x0: 0, y0: 100, x1: 200, y1: 300, cx: 300, cy: 200 } } as never
  const source = { name: 'recovered-objects', role: 'objects', x: 100, y: 400, w: 40, h: 120 } as never
  const swing = bindHangingPhysics(source, { x: 120, y: 400 }, { anchors, partProfile: profile([part({})]) }, () => null)
  assert.ok(swing, 'the charm swings')
  assert.equal(bindHangingPhysics(source, { x: 120, y: 400 }, { anchors, partProfile: undefined }, () => null), null)
  assert.equal(bindHangingPhysics(source, null, { anchors, partProfile: profile([part({})]) }, () => null), null)
  assert.deepEqual(PENDULUM_DEFAULT, { stiffness: 1, damping: 0.65, reach: 1 })
  // A charm drawn into a hair ornament's layer with the rest of it does not swing that whole layer.
  const headwear = { name: 'headwear', role: 'headwear', x: 100, y: 400, w: 200, h: 120 } as never
  const shared = profile([part({ layer: 'headwear', separate: false })])
  assert.equal(bindHangingPhysics(headwear, { x: 120, y: 400 }, { anchors, partProfile: shared }, () => null), null)
})

test('a cut ribbon bends from its cut by its swing; a cut gem only at its link', () => {
  const host = { name: 'legwear', role: 'legwear', x: 0, y: 0, w: 100, h: 200, atlas: { x: 0, y: 0, w: 1, h: 1 } } as Anime25DPlaybackLayer
  const pixels = new Uint8ClampedArray(100 * 200 * 4)
  // Cloth above y 40; a ribbon hangs from it at x 40–60, down to y 160, over nothing.
  for (let y = 0; y < 200; y++) {
    for (let x = 0; x < 100; x++) {
      const ribbon = x >= 40 && x < 60 && y >= 30 && y < 160
      if (y < 40 || ribbon) pixels.set(ribbon ? [120, 80, 200, 255] : [240, 220, 230, 255], (y * 100 + x) * 4)
    }
  }
  const ribbonPart = part({ name: 'ribbon', layer: 'legwear', separate: false, box: [40, 30, 60, 160], anchor: [50, 40], motion: 'dangle', stiffness: 0.3 })
  const profile: Anime25DPartProfile = { version: 1, source: 'ai-vision', parts: [ribbonPart] }
  const [cut] = splitHangingParts([host], profile, { width: 1000, height: 1000 }, () => ({ pixels, width: 100, height: 200 }))
  assert.ok(cut, 'the ribbon is cut out')
  const rest = Float32Array.of(50, 40, 50, 160)
  const layer = { source: cut.source, rest, attachment: { y: 40 }, earwearPhysics: {} as never, hangBend: null as Float32Array | null, deformed: null as Float32Array | null }
  bindHangBend(layer, profile)
  assert.ok(layer.hangBend && layer.deformed)
  assert.equal(layer.hangBend[0], 0, 'at its cut it stays on its host')
  assert.equal(layer.hangBend[1], 1, 'at its end it swings')
  // The host carries it as drawn; its swing would turn it whole about (50, 40).
  const hostCarry = Float32Array.of(1, 0, 0, 0, 1, 0, 0, 0, 1)
  const angle = 0.2
  const c = Math.cos(angle)
  const s = Math.sin(angle)
  const swung = Float32Array.of(c, s, 0, -s, c, 0, 50 - c * 50 + s * 40, 40 - s * 50 - c * 40, 1)
  assert.ok(bendHanging(layer.hangBend, hostCarry, swung, rest, layer.deformed))
  assert.deepEqual(Array.from(layer.deformed.subarray(0, 2)), [50, 40], 'the cut does not open')
  assert.ok(Math.abs(layer.deformed[2] - (50 - s * 120)) < 1e-3 && Math.abs(layer.deformed[3] - (40 + c * 120)) < 1e-3, 'the end swings with it')
  const gem = { ...layer, rest: Float32Array.of(50, 40, 50, 80, 50, 160), hangBend: null as Float32Array | null, deformed: null }
  bindHangBend(gem, { ...profile, parts: [{ ...ribbonPart, stiffness: 0.6 }] })
  assert.ok(gem.hangBend)
  assert.equal(gem.hangBend[0], 0, 'a stiff piece stays on its host at its cut too')
  assert.equal(gem.hangBend[1], 1, 'and swings whole below its link')
})
