import assert from 'node:assert/strict'
import test from 'node:test'
import { createHairChain } from './hairChain'
import {
  frontHairUpperParallaxScale,
  HAIR_CHAIN_LINKS,
  hairLockCharacter,
  hairStrandDynamics,
  stepAnime25DHairLayerSprings,
} from './hairPhysics'

function tip(spring: ReturnType<typeof strand>) {
  return {
  x: spring.chain.offsetX[spring.chain.links],
  y: spring.chain.offsetY[spring.chain.links],
}
}

test('a lock trails a moving root, then settles back to hanging as drawn', () => {
  const layers = springLayers()
  const springs = layers.flatMap(layer => layer.springs ?? [])
  const frame = hairFrame()
  stepAnime25DHairLayerSprings(layers, frame, 1 / 60)
  for (const spring of springs) spring.supportX = 24
  stepAnime25DHairLayerSprings(layers, frame, 1 / 60)
  stepAnime25DHairLayerSprings(layers, frame, 1 / 60)
  for (const spring of springs) assert.ok(tip(spring).x < -5, `tip ${tip(spring).x}`)
  for (let i = 0; i < 1800; i++) stepAnime25DHairLayerSprings(layers, frame, 1 / 60)
  for (const spring of springs) {
    assert.ok(Math.abs(tip(spring).x) < 0.01)
    assert.ok(Math.abs(tip(spring).y) < 0.01)
  }
})

test('a root lifted or dropped is trailed along the lock, not stretched', () => {
  const layers = springLayers()
  const springs = layers.flatMap(layer => layer.springs ?? [])
  const frame = hairFrame()
  stepAnime25DHairLayerSprings(layers, frame, 1 / 60)
  for (const goal of [24, -24, 0]) {
    for (const spring of springs) spring.supportY = goal
    for (let i = 0; i < 120; i++) stepAnime25DHairLayerSprings(layers, frame, 1 / 60)
    for (const spring of springs) {
      const { x, y, links, linkLength } = spring.chain
      for (let index = 1; index <= links; index++) {
        assert.ok(Math.abs(Math.hypot(x[index] - x[index - 1], y[index] - y[index - 1]) - linkLength) < 1e-6)
      }
    }
  }
})

test('the swing does not depend on the frame rate', () => {
  const run = (fps: number) => {
    const layers = springLayers()
    const springs = layers.flatMap(layer => layer.springs ?? [])
    const frame = hairFrame()
    stepAnime25DHairLayerSprings(layers, frame, 1 / fps)
    for (let i = 0; i < fps; i++) {
      for (const spring of springs) spring.supportX = 40 * Math.min(1, i / (fps * 0.3))
      stepAnime25DHairLayerSprings(layers, frame, 1 / fps)
    }
    return springs.map(spring => tip(spring).x)
  }
  const reference = run(120)
  for (const fps of [30, 60]) {
    run(fps).forEach((value, index) => assert.ok(Math.abs(value - reference[index]) < 0.5, `${fps} fps ${value} vs ${reference[index]}`))
  }
})

test('idle air sways a lock by a few pixels whatever its stiffness, and still air not at all', () => {
  for (const idle of [true, false]) {
    const layers = springLayers()
    const springs = layers.flatMap(layer => layer.springs ?? [])
    const frame = { ...hairFrame(), idle }
    let most = 0
    for (let i = 0; i < 1200; i++) {
      frame.time = i / 60
      stepAnime25DHairLayerSprings(layers, frame, 1 / 60)
      for (const spring of springs) most = Math.max(most, Math.abs(tip(spring).x))
    }
    if (idle) assert.ok(most > 1 && most < 12, `idle sway ${most}`)
    else assert.equal(most, 0)
  }
})

test('bangs hold their shape better than long rear hair', () => {
  const front = { frontHair: true, springs: [strand(0, 1)] }
  const rear = { frontHair: false, springs: [strand(0, 1)] }
  const frame = hairFrame()
  stepAnime25DHairLayerSprings([front, rear], frame, 1 / 60)
  let frontMost = 0
  let rearMost = 0
  for (let i = 0; i < 120; i++) {
    front.springs[0].supportX = rear.springs[0].supportX = 60 * Math.min(1, i / 12)
    stepAnime25DHairLayerSprings([front, rear], frame, 1 / 60)
    frontMost = Math.max(frontMost, Math.abs(tip(front.springs[0]).x))
    rearMost = Math.max(rearMost, Math.abs(tip(rear.springs[0]).x))
  }
  assert.ok(frontMost < rearMost, `front ${frontMost} rear ${rearMost}`)
})

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

test('matches strand displacement to projected pixel length within safe bounds', () => {
  const referenceHeight = 500

  assert.deepEqual(hairStrandDynamics(100, 600, referenceHeight), {
    amplitudeScale: 1,
    stiffnessScale: 1,
    dampingScale: 1,
  })
  assert.equal(hairStrandDynamics(100, 1100, referenceHeight).amplitudeScale, 2)
  assert.equal(
    hairStrandDynamics(100, 200, referenceHeight).amplitudeScale,
    0.5,
  )
  assert.equal(
    hairStrandDynamics(100, 2100, referenceHeight).amplitudeScale,
    2.5,
  )
})

test('length response scaling preserves the authored damping ratio', () => {
  const dynamics = hairStrandDynamics(100, 1100, 500)
  const stiffness = 70
  const damping = 9
  const authoredRatio = damping / (2 * Math.sqrt(stiffness))
  const adaptedRatio =
    (damping * dynamics.dampingScale) /
    (2 * Math.sqrt(stiffness * dynamics.stiffnessScale))

  assert.ok(Math.abs(adaptedRatio - authoredRatio) < 1e-12)
  assert.ok(dynamics.stiffnessScale < 1)
  assert.ok(dynamics.dampingScale < 1)
  assert.deepEqual(hairStrandDynamics(600, 100, 500), {
    amplitudeScale: 1,
    stiffnessScale: 1,
    dampingScale: 1,
  })
})

function hairFrame() {
  return { enabled: true, idle: false, faceScale: 1, time: 0, frontSoft: 0.4, rearSoft: 2 }
}

function strand(phase: number, scale: number) {
  return {
    supportX: 0,
    supportY: 0,
    rootX: 100,
    rootY: 50,
    chain: createHairChain(100, 50, 100, 350, HAIR_CHAIN_LINKS),
    phase,
    amplitudeScale: 1,
    stiffnessScale: scale,
    dampingScale: Math.sqrt(scale),
  }
}

function springLayers() {
  return [
    { frontHair: true, springs: [strand(0.3, 0.8), strand(1.7, 1.2)] },
    { frontHair: false, springs: null },
    { frontHair: false, springs: [strand(2.8, 0.64)] },
  ]
}

test('locks of one front hair are each tuned apart, and a tuft above the face is springier', () => {
  const face = { y0: 100, y1: 500 }
  const scales = [1, 2, 3, 4, 5, 6].map((n) => hairLockCharacter({ name: `front-hair-${n}`, y: 120, h: 300 }, face).frequencyScale)
  // Neighbours differ, and none by more than the detune.
  for (let i = 1; i < scales.length; i++) assert.ok(Math.abs(scales[i] - scales[i - 1]) > 0.03, `${scales}`)
  assert.ok(scales.every((scale) => scale >= 0.88 && scale <= 1.12), `${scales}`)
  // A layer not cut into locks keeps the shared tuning.
  assert.deepEqual(hairLockCharacter({ name: 'front-hair', y: 120, h: 300 }, face), { frequencyScale: 1, dampingRatioScale: 1 })
  const tuft = hairLockCharacter({ name: 'front-hair-7', y: 20, h: 70 }, face)
  assert.ok(tuft.frequencyScale > 1.3 && tuft.dampingRatioScale < 1)
})
