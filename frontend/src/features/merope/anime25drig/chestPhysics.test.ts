import assert from 'node:assert/strict'
import test from 'node:test'
import {
  buildChestWeightField,
  chestBodyExcitationY,
  chestBreathResidual,
  chestBreathTargetY,
  chestDeformationWeight,
  chestFollowMix,
  chestMotionTarget,
  chestProfileUsesGeometryWeights,
  chestResponseMix,
  createChestSpringState,
  deriveGeometryChestProfile,
  resolveChestDeformationRegion,
  resolveChestDynamics,
  resolveChestMotionScale,
  resolveChestSpatialField,
  sampleChestVerticalWeight,
  sampleChestWeight,
  stepChestSpring,
  topwearMotionAtChest,
} from './chestPhysics'

test('uses the import-authored deformation region without runtime repair', () => {
  const region = resolveChestDeformationRegion({
    source: 'ai-vision',
    centerX: 542,
    centerY: 1014,
    radiusX: 102,
    radiusY: 84,
    visibleScale: 0.55,
  })
  assert.deepEqual(region, {
    centerX: 542,
    centerY: 1014,
    radiusX: 102,
    radiusY: 84,
  })
})

test('authors a complete deterministic geometry profile before AI refinement', () => {
  const profile = deriveGeometryChestProfile({
    pixelCanvas: { width: 768, height: 1024 },
    anchors: {
      face: { x0: 230, y0: 92, x1: 538, y1: 368, cx: 384, cy: 246 },
      neckPivot: { x: 384, y: 390 },
      neckTop: 368,
      neckBottom: 428,
      bodyPivot: { x: 384, y: 1024 },
      mouth: { x0: 350, y0: 300, x1: 418, y1: 330, cx: 384, cy: 315 },
      faceScale: 308 / 333,
    },
    layers: [
      {
        name: 'topwear',
        role: 'topwear',
        z: 0,
        depth: 0.9,
        group: 'body',
        phys: null,
        fade: null,
        side: null,
        x: 138.24,
        y: 368.64,
        w: 491.52,
        h: 655.36,
        atlas: { x: 0, y: 0, w: 1, h: 1 },
        strands: [],
      },
    ],
  })
  assert.equal(profile.version, 2)
  assert.equal(profile.source, 'geometry-fallback')
  assert.equal(profile.centerX, 384)
  assert.equal(profile.radiusX, 308 * 0.6)
  assert.equal(profile.radiusY, 276 * 0.32)
})

test('AI deformation peaks on the paired lobes instead of the sternum', () => {
  const field = resolveChestSpatialField({
    source: 'ai-vision',
    supportScale: 0.2,
    garmentMotionScale: 1,
  })
  const center = chestDeformationWeight(field, 0, 0, 1)
  const left = chestDeformationWeight(field, -0.58, 0, 1)
  const right = chestDeformationWeight(field, 0.58, 0, 1)
  assert.ok(center < left)
  assert.equal(left, right)
  assert.equal(left, 1)
})

test('shares one asymmetric vertical envelope across chest consumers', () => {
  assert.equal(sampleChestVerticalWeight(-1.2), 0)
  assert.equal(sampleChestVerticalWeight(0), 1)
  assert.equal(sampleChestVerticalWeight(1.35), 0)
  assert.ok(sampleChestVerticalWeight(0.6) > sampleChestVerticalWeight(-0.6))
  assert.equal(sampleChestVerticalWeight(Number.NaN), 0)
})

test('derives a flatter and quieter field for structured garments', () => {
  const soft = resolveChestSpatialField({
    source: 'ai-vision',
    supportScale: 0.1,
    garmentMotionScale: 1,
  })
  const structured = resolveChestSpatialField({
    source: 'ai-vision',
    supportScale: 0.95,
    garmentMotionScale: 0.1,
  })
  assert.ok(soft.centerBridge > structured.centerBridge)
  assert.ok(soft.depthRatio > structured.depthRatio)
  assert.ok(soft.nearDepthGain > structured.nearDepthGain)
  assert.ok(soft.silhouetteRatio > structured.silhouetteRatio)
  assert.ok(soft.breathVolumeGain > structured.breathVolumeGain)
})

test('uses a neutral, bounded breathing signal for volume and travel', () => {
  assert.equal(chestBreathResidual(0), 0)
  const peakAt = 3.4 / 4
  assert.ok(Math.abs(chestBreathResidual(peakAt) - 0.5) < 1e-12)
  assert.ok(chestBreathTargetY(0.5, 2, 1) < 0)
  assert.equal(chestBreathTargetY(0, 2, 1), 0)
})

test('uses one authoritative chest region instead of intersecting AI with geometry', () => {
  assert.equal(
    chestProfileUsesGeometryWeights({ enabled: true, source: 'ai-vision' }),
    false,
  )
  assert.equal(
    chestProfileUsesGeometryWeights({
      enabled: true,
      source: 'geometry-fallback',
    }),
    true,
  )
  assert.equal(
    chestProfileUsesGeometryWeights({
      enabled: false,
      source: 'gender-policy',
    }),
    false,
  )
})

test('restrains small AI profiles with a smooth size cap', () => {
  const resolve = (visibleScale: number, motionScale = 1.14) =>
    resolveChestMotionScale({
      enabled: true,
      source: 'ai-vision',
      visibleScale,
      motionScale,
    })

  assert.ok(Math.abs(resolve(0.35, 0.958) - 0.4585) < 0.001)
  assert.ok(resolve(0.2) < resolve(0.35))
  assert.ok(resolve(0.35) < resolve(0.5))
  assert.ok(resolve(0.5) < resolve(0.8))
  assert.equal(resolve(0.35, 0.3), 0.3)
  assert.equal(
    resolveChestMotionScale({
      enabled: true,
      source: 'geometry-fallback',
      visibleScale: 0.35,
      motionScale: 1,
    }),
    1,
  )
  assert.equal(
    resolveChestMotionScale({
      enabled: false,
      source: 'gender-policy',
      visibleScale: 0,
      motionScale: 0,
    }),
    0,
  )
})

test('combines apparent size with garment support without losing bounded control', () => {
  const freelyVisible = resolveChestDynamics({
    enabled: true,
    source: 'ai-vision',
    visibleScale: 0.8,
    motionScale: 1.14,
    frequencyScale: 0.94,
    supportScale: 0.15,
    garmentMotionScale: 0.95,
  })
  const structured = resolveChestDynamics({
    enabled: true,
    source: 'ai-vision',
    visibleScale: 0.8,
    motionScale: 1.14,
    frequencyScale: 0.94,
    supportScale: 0.9,
    garmentMotionScale: 0.2,
  })
  const small = resolveChestDynamics({
    enabled: true,
    source: 'ai-vision',
    visibleScale: 0.3,
    motionScale: 0.36,
    frequencyScale: 1.06,
    supportScale: 0.15,
    garmentMotionScale: 0.95,
  })

  assert.ok(freelyVisible.responseScale > structured.responseScale)
  assert.ok(freelyVisible.followScale > structured.followScale)
  assert.ok(small.followScale < freelyVisible.followScale * 0.35)
  assert.ok(small.responseScale < freelyVisible.responseScale * 0.15)
  assert.ok(freelyVisible.followScale >= freelyVisible.responseScale)
  assert.ok(freelyVisible.frequencyScale < structured.frequencyScale)
  assert.ok(freelyVisible.dampingScale < structured.dampingScale)
  assert.ok(freelyVisible.inertiaGain > 5)
  assert.equal(freelyVisible.bodyExcitationScale, 1)
  assert.equal(small.inertiaGain, 1)
  assert.equal(small.bodyExcitationScale, 0)
  assert.ok(chestResponseMix(2.5, freelyVisible.responseScale) <= 0.94)
  assert.ok(chestFollowMix(2.5, freelyVisible.followScale) <= 1)
  assert.ok(chestResponseMix(2.5, 0.5) > 0.5)
  assert.ok(chestFollowMix(2.5, 0.5) > 0.5)
  assert.equal(chestResponseMix(0, freelyVisible.responseScale), 0)
})

test('maps resolved horizontal and vertical pose travel to independent chest axes', () => {
  const reusableTarget = { x: 0, y: 0 }
  const horizontal = chestMotionTarget(
    { angleX: 1, angleY: 0, angleZ: 0, body: 0 },
    2,
    reusableTarget,
  )
  const vertical = chestMotionTarget(
    { angleX: 0, angleY: 1, angleZ: 0, body: 0 },
    2,
  )
  const parentHorizontal = topwearMotionAtChest(
    { angleX: 1, angleY: 0, angleZ: 0, body: 0 },
    {
      faceScale: 2,
      faceCenterY: 0,
      neckX: 0,
      neckY: 0,
      centerX: 0,
      centerY: 0,
      depth: 1,
    },
  )
  const wholeBodyRotation = chestMotionTarget(
    { angleX: 0, angleY: 0, angleZ: 0, body: 1 },
    2,
  )
  assert.equal(horizontal.x, 11)
  assert.equal(horizontal.y, 0)
  assert.equal(horizontal, reusableTarget)
  assert.equal(vertical.x, 0)
  assert.equal(vertical.y, -9)
  assert.equal(parentHorizontal.x, 4.48)
  assert.deepEqual(wholeBodyRotation, { x: 0, y: 0 })
})

test('injects whole-body motion only as size-conditioned spring excitation', () => {
  assert.equal(chestBodyExcitationY(1, 2, 0), 0)
  assert.equal(chestBodyExcitationY(1, 2, 1), 40)
  assert.equal(chestBodyExcitationY(-0.5, 2, 0.5), -10)
})

test('turns a normal emphasize cue into clearly visible large-profile inertia', () => {
  const dynamics = resolveChestDynamics({
    enabled: true,
    source: 'ai-vision',
    visibleScale: 0.8,
    motionScale: 1.14,
    frequencyScale: 0.94,
    supportScale: 0.15,
    garmentMotionScale: 0.95,
  })
  const state = createChestSpringState()
  const dt = 1 / 60
  let body = 0
  let peak = 0
  for (let frame = 0; frame < 180; frame += 1) {
    const time = frame * dt
    const envelope =
      time < 0.25
        ? time / 0.25
        : time < 0.8
          ? 1
          : time < 1.15
            ? (1.15 - time) / 0.35
            : 0
    const targetBody = 0.22 * Math.max(0, envelope)
    body += (targetBody - body) * Math.min(1, dt * 14)
    stepChestSpring(
      state,
      0,
      chestBodyExcitationY(body, 1, dynamics.bodyExcitationScale),
      dt,
      dynamics.frequencyScale,
      dynamics.dampingScale,
    )
    peak = Math.max(
      peak,
      Math.abs(
        state.offsetY *
          chestResponseMix(2.5, dynamics.responseScale) *
          dynamics.inertiaGain,
      ),
    )
  }
  assert.ok(peak > 8)
  assert.ok(peak < 12)
})

test('extracts and bilinearly samples the authored chest joint weights', () => {
  const vertex = (x: number, y: number, chestWeight: number) => ({
    position: { x, y },
    joints: [0, 1, 0, 0],
    weights: [1 - chestWeight, chestWeight, 0, 0],
  })
  const field = buildChestWeightField({
    bones: [{ id: 'body' }, { id: 'a25d-chest' }],
    parts: [
      {
        id: 'a25d-topwear',
        vertices: [
          vertex(0, 0, 0),
          vertex(1, 0, 0.2),
          vertex(0, 1, 0.8),
          vertex(1, 1, 1),
        ],
      },
    ],
  })

  assert.ok(field)
  assert.ok(Math.abs(sampleChestWeight(field, 0.5, 0.5) - 0.5) < 1e-6)
  assert.equal(sampleChestWeight(field, -1, -1), 0)
  assert.equal(sampleChestWeight(field, 2, 2), 1)
})

test('two-axis chest response moves with its base before the relative lag', () => {
  const state = createChestSpringState()
  stepChestSpring(state, 0, 0, 1 / 60)
  stepChestSpring(state, 6, -4, 1 / 60)
  assert.ok(state.offsetX < 0)
  assert.ok(state.offsetY > 0)
  const responseMix = 0.8
  assert.ok(6 + state.offsetX * responseMix > 0)
  assert.ok(-4 + state.offsetY * responseMix < 0)

  for (let frame = 0; frame < 240; frame += 1) {
    stepChestSpring(state, 6, -4, 1 / 60)
  }
  assert.ok(Math.abs(state.offsetX) < 1e-5)
  assert.ok(Math.abs(state.offsetY) < 1e-5)
})

test('chest spring reverses only after following a moving base', () => {
  const state = createChestSpringState()
  stepChestSpring(state, 0, 0, 1 / 60)

  let firstVisible = 0
  for (let frame = 0; frame < 20; frame += 1) {
    stepChestSpring(state, 6, 0, 1 / 60)
    const visible = 6 + state.offsetX * 0.8
    if (frame === 0) firstVisible = visible
  }
  assert.ok(firstVisible > 0)

  let forwardAfterStop = 0
  let reverseAfterStop = 0
  for (let frame = 0; frame < 120; frame += 1) {
    stepChestSpring(state, 0, 0, 1 / 60)
    const visible = state.offsetX * 0.8
    forwardAfterStop = Math.max(forwardAfterStop, visible)
    reverseAfterStop = Math.min(reverseAfterStop, visible)
  }

  assert.ok(forwardAfterStop > 0)
  assert.ok(reverseAfterStop < 0)
  assert.ok(Math.abs(state.offsetX) < 0.01)
})

test('chest spring response stays stable across common render frame rates', () => {
  // The chest base moves every frame in the player, so the invariant is about
  // a base in motion. The relative offset this spring reports is driven by the
  // base's acceleration, and a single instantaneous jump carries none that a
  // sampled input can reproduce at two different rates.
  const base = (seconds: number) => {
    const progress = Math.min(1, seconds / 0.25)
    const eased = progress * progress * (3 - 2 * progress)
    return { x: 6 * eased, y: -4 * eased }
  }
  const simulate = (fps: number) => {
    const state = createChestSpringState()
    stepChestSpring(state, 0, 0, 1 / fps)
    for (let frame = 1; frame <= fps / 2; frame += 1) {
      const at = base(frame / fps)
      stepChestSpring(state, at.x, at.y, 1 / fps)
    }
    return state
  }

  const at30Fps = simulate(30)
  const at60Fps = simulate(60)
  assert.ok(Math.abs(at30Fps.offsetX - at60Fps.offsetX) < 0.02)
  assert.ok(Math.abs(at30Fps.offsetY - at60Fps.offsetY) < 0.02)
})

test('adaptive frequency changes timing while keeping the spring stable', () => {
  const simulate = (frequencyScale: number) => {
    const state = createChestSpringState()
    stepChestSpring(state, 0, 0, 1 / 60, frequencyScale)
    for (let frame = 0; frame < 6; frame += 1) {
      stepChestSpring(state, 6, 0, 1 / 60, frequencyScale)
    }
    return state.offsetX
  }

  const slower = simulate(0.8)
  const faster = simulate(1.2)
  assert.ok(Math.abs(slower) > Math.abs(faster))
  assert.ok(Number.isFinite(slower))
  assert.ok(Number.isFinite(faster))
})
