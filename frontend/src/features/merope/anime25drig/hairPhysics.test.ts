import assert from 'node:assert/strict'
import test from 'node:test'
import {
  frontHairUpperParallaxScale,
  hairStrandDynamics,
  stepAnime25DHairLayerSprings,
  stepHairSpring,
} from './hairPhysics'

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

test('hair spring response stays consistent across rendering frame rates', () => {
  const simulate = (fps: number) => {
    const spring = { x: 0, v: 0, dx: 0 }
    const dynamics = hairStrandDynamics(100, 1100, 500)
    const frames = Math.round(fps * 1.5)
    for (let frame = 0; frame < frames; frame += 1) {
      stepHairSpring(
        spring,
        20,
        70 * dynamics.stiffnessScale,
        9 * dynamics.dampingScale,
        2.2,
        1 / fps,
      )
    }
    return spring
  }

  const at30 = simulate(30)
  const at60 = simulate(60)
  const at120 = simulate(120)
  assert.ok(Math.abs(at30.x - at120.x) < 1e-10)
  assert.ok(Math.abs(at60.x - at120.x) < 1e-10)
  assert.ok(Math.abs(at30.dx - at120.dx) < 1e-10)
})

test('layer spring orchestration matches the frozen player loop', () => {
  const actual = springLayers()
  const expected = springLayers()
  for (let frameIndex = 0; frameIndex < 120; frameIndex += 1) {
    const time = frameIndex / 60
    const frame = {
      enabled: frameIndex % 17 !== 0,
      idle: frameIndex % 11 !== 0,
      angleX: Math.sin(time * 1.7) * 0.8,
      angleZ: Math.cos(time * 1.1) * 0.65,
      faceScale: 0.83,
      neckPivotY: 220,
      faceCenterY: 126,
      time,
    }
    const elapsedSeconds = frameIndex % 9 === 0 ? 1 / 30 : 1 / 60
    legacyStepLayerSprings(expected, frame, elapsedSeconds)
    stepAnime25DHairLayerSprings(actual, frame, elapsedSeconds)
    assert.deepEqual(actual, expected, `frame ${frameIndex}`)
  }
})

function springLayers() {
  const spring = (phase: number, scale: number) => ({
    stiff: { x: 0, v: 0, dx: 0 },
    soft: { x: 0, v: 0, dx: 0 },
    phase,
    stiffnessScale: scale,
    dampingScale: Math.sqrt(scale),
  })
  return [
    { springs: [spring(0.3, 0.8), spring(1.7, 1.2)] },
    { springs: null },
    { springs: [spring(2.8, 0.64)] },
  ]
}

function legacyStepLayerSprings(
  layers: ReturnType<typeof springLayers>,
  frame: {
    enabled: boolean
    idle: boolean
    angleX: number
    angleZ: number
    faceScale: number
    neckPivotY: number
    faceCenterY: number
    time: number
  },
  elapsedSeconds: number,
): void {
  if (!frame.enabled) return
  const headOffsetX =
    (frame.angleX * 14 +
      frame.angleZ * 0.07 * (frame.neckPivotY - frame.faceCenterY)) *
    frame.faceScale
  const windAmplitude = frame.idle ? 1 : 0
  for (const layer of layers) {
    if (!layer.springs) continue
    for (const spring of layer.springs) {
      const wind =
        windAmplitude *
        (1.8 * Math.sin(frame.time * 0.8 + spring.phase) +
          Math.sin(frame.time * 1.9 + spring.phase * 2.3))
      const target = headOffsetX + wind * frame.faceScale
      stepHairSpring(
        spring.stiff,
        target,
        70 * spring.stiffnessScale,
        9 * spring.dampingScale,
        2.2,
        elapsedSeconds,
      )
      stepHairSpring(
        spring.soft,
        target,
        16 * spring.stiffnessScale,
        1.3 * spring.dampingScale,
        3,
        elapsedSeconds,
      )
    }
  }
}
