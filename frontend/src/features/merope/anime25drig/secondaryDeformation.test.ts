import type { Anime25DLayerSpringBinding } from './layerBinding'
import type {
  Anime25DSecondaryDeformationBinding,
  Anime25DSecondaryDeformationFrame,
} from './secondaryDeformation'
import type { Anime25DPlaybackLayer, Anime25DShellProfile } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { ARM_SHRUG, bindArmRigMesh } from './armRig'
import {
  chestDeformationWeight,
  resolveChestSpatialField,
} from './chestPhysics'
import {
  BODY_HEAD_FOLLOW,
  FRONT_COLLAR_FLEX_REGION,
  FRONT_COLLAR_HEAD_FOLLOW,
  FRONT_COLLAR_INNER_REGION,
  HIGH_COLLAR_NECK_FOLLOW_POWER,
} from './collarRuntime'
import { IDENTITY_DRIVER } from './driver'
import { bindPoseCorrections, writePoseCorrectionWeights } from './poseCorrections'
import {
  createAnime25DSecondaryDeformationBinding,
  deformAnime25DHairPoint,
  deformAnime25DSecondaryPoint,
} from './secondaryDeformation'
import { writeAnime25DShellRotation } from './shellDeformation'
import { applySurfaceContact, bindSurfaceContact } from './surfaceContact'
import {
  anime25DTorsoShellModeForLayer,
  deformAnime25DTorsoShellPoint,
  SLEEVE_TORSO_TRANSMISSION,
} from './torsoDeformation'

const VERTEX_COUNT = 48

test('torso root transports the whole head once and blends through the neck without dragging the garment', () => {
  const frame = secondaryFrame(0.46, 17)
  for (const offset of [-24, 24]) {
    for (const role of ['face', 'eyelash', 'mouth-open', 'front-hair', 'back-hair']) {
      const binding = secondaryBinding(role, 'head', false)
      for (const [x, y] of [[80, 110], [140, 180]]) {
        const before = { x, y }; const after = { x, y }
        frame.torsoNeckOffsetX = 0
        deformAnime25DSecondaryPoint(before, x, y, 0, binding, frame)
        frame.torsoNeckOffsetX = offset
        deformAnime25DSecondaryPoint(after, x, y, 0, binding, frame)
        assert.ok(Math.abs(after.x - before.x - offset) < 1e-8)
        assert.equal(after.y, before.y)
      }
    }
    for (const highCollar of [false, true]) {
      frame.highCollar = highCollar
      for (const [role, y, weight] of [
        ['neck', frame.neckFollowTop, 1], ['neck', frame.neckBottom, 0],
        ['topwear', frame.neckBottom, 0], ['handwear', frame.neckBottom, 0],
      ] as const) {
        const binding = secondaryBinding(role, 'body', false)
        const x = frame.neckPivotX
        const before = { x, y }; const after = { x, y }
        frame.torsoNeckOffsetX = 0
        deformAnime25DSecondaryPoint(before, x, y, 0, binding, frame)
        frame.torsoNeckOffsetX = offset
        deformAnime25DSecondaryPoint(after, x, y, 0, binding, frame)
        assert.ok(Math.abs(after.x - before.x - offset * weight) < 1e-8, `${role} at ${y}`)
        assert.equal(after.y, before.y)
      }
    }
  }
})

test('facial art shares one projected surface without changing independent feature coordinates', () => {
  const frame = secondaryFrame(0.46, 17)
  frame.shellProfile = shellProfile()
  frame.shellActivation = 1
  frame.shellBlend = 0.5
  const face = secondaryBinding('face', 'head', false)
  face.shellMode = 'head'
  face.source.depth = 1
  const roles = ['eyewhite', 'eyelash', 'irides', 'eyebrow', 'nose', 'mouth-open', 'eye-close', 'eye-cry', 'iris-silly', 'facedetail']
  for (const yaw of [-1, 0, 1]) { for (const pitch of [-0.85, 0, 0.85]) {
    frame.expression.angleX = yaw
    frame.headAngleY = pitch
    writeAnime25DShellRotation(yaw, pitch, frame.shellRotation)
    for (const [x, y] of [[85, 120], [117, 153], [155, 120]]) {
      const expected = { x, y }
      deformAnime25DSecondaryPoint(expected, x, y, 0, face, frame)
      for (const role of roles) {
        const feature = secondaryBinding(role, 'head', false)
        feature.shellMode = 'head'
        feature.source.depth = 1.15
        const actual = { x, y }
        deformAnime25DSecondaryPoint(actual, x, y, 0, feature, frame)
        assert.deepEqual(actual, expected, `${role}: yaw ${yaw}, pitch ${pitch}`)
      }
    }
  }
}
  for (const role of ['front-hair', 'back-hair', 'ears', 'eyewear', 'earwear', 'headwear', 'anger-mark', 'speechless-sweat', 'neckwear']) {
    assert.equal(secondaryBinding(role, 'head', false).facialSurface, false, role)
  }
})

test('pose residual is blended once, before parent roll, and never applied to the collar', () => {
  const profile = shellProfile()
  const rest = new Float32Array([profile.head.centerX, profile.head.centerY])
  const bound = bindPoseCorrections([{
    surface: 'head', at: { angleX: 0.8, angleY: -0.6 },
    patches: [{ x: 0, y: 0, radiusX: 0.6, radiusY: 0.4, dx: 0.04, dy: -0.03 }],
  }], 'head', rest, profile.head)!
  const driver = { ...IDENTITY_DRIVER, angleX: 0.8, angleY: -0.6 }
  writePoseCorrectionWeights(bound, driver)
  const frame = secondaryFrame(0.46, 17)
  frame.expression = driver
  frame.headAngleY = driver.angleY
  frame.shellProfile = profile
  writeAnime25DShellRotation(driver.angleX, driver.angleY, frame.shellRotation)
  const binding = { ...secondaryBinding('face', 'head', false), shellMode: 'head' as const }
  for (const roll of [-0.4, 0, 0.4]) {
    frame.headRotationCosine = Math.cos(roll)
    frame.headRotationSine = Math.sin(roll)
    for (const blend of [0, 0.5, 1]) {
      frame.shellBlend = blend
      const before = { x: rest[0], y: rest[1] }
      const after = { ...before }
      deformAnime25DSecondaryPoint(before, rest[0], rest[1], 0, binding, frame)
      deformAnime25DSecondaryPoint(after, rest[0], rest[1], 0, { ...binding, poseCorrections: bound }, frame)
      const dx = bound[0].offsets[0]
      const dy = bound[0].offsets[1]
      assert.ok(Math.abs(after.x - before.x - (dx * Math.cos(roll) - dy * Math.sin(roll)) * blend) < 1e-6)
      assert.ok(Math.abs(after.y - before.y - (dx * Math.sin(roll) + dy * Math.cos(roll)) * blend) < 1e-6)
    }
  }
  const collar = secondaryBinding('collar_front', 'body', false, true)
  const before = { x: rest[0], y: rest[1] }
  const after = { ...before }
  deformAnime25DSecondaryPoint(before, rest[0], rest[1], 0, collar, frame)
  deformAnime25DSecondaryPoint(after, rest[0], rest[1], 0, { ...collar, poseCorrections: bound }, frame)
  assert.deepEqual(after, before)
})

test('binds stable secondary roles and optional geometry fields once', () => {
  const topwear = secondaryBinding('topwear', 'body', false)
  assert.equal(topwear.topwear, true)
  assert.equal(topwear.head, false)
  assert.equal(topwear.frontCollar, false)

  const collar = secondaryBinding('collar_front', 'body', false, true)
  assert.equal(collar.frontCollar, true)
  assert.equal(collar.collarContact, true)

  const rearCollar = secondaryBinding('collar_back', 'body', false)
  assert.equal(rearCollar.rearCollar, true)
  assert.equal(rearCollar.frontCollar, false)

  const hair = secondaryBinding('front hair', 'head', false, false, true)
  assert.equal(hair.head, true)
  assert.equal(hair.frontHair, true)
  assert.equal(hair.springs?.length, 3)
})

test('primary shape and lateral hair motion retain the reference without fake downward stretch', () => {
  const bindings = secondaryBindings()
  for (let frameIndex = 0; frameIndex < 120; frameIndex += 1) {
    const progress = frameIndex / 119
    const frame = secondaryFrame(progress, frameIndex)
    // Compare the reference on its interpolation domain. Values above one
    // used negative stiff weights; the separate convexity test covers that fix.
    frame.expression.soft = Math.min(1, frame.expression.soft)
    frame.expression.fhSoft = Math.min(1, frame.expression.fhSoft)
    for (const binding of bindings) {
      for (let row = 0; row <= 5; row += 1) {
        for (let column = 0; column <= 7; column += 1) {
          const vertex = row * 8 + column
          const source = binding.source
          const restX = source.x + (source.w * column) / 7
          const restY = source.y + (source.h * row) / 5
          const initialX = restX + Math.sin(frameIndex * 0.17 + vertex) * 1.3
          const initialY = restY + Math.cos(frameIndex * 0.13 + vertex) * 1.1
          const actual = { x: initialX, y: initialY }
          const expected = { ...actual }
          legacyDeformSecondaryPoint(
            expected,
            restX,
            restY,
            vertex,
            binding,
            frame,
          )
          const primaryY = expected.y
          legacyDeformHairPoint(expected, vertex, binding, frame)
          // The old absolute-X term always pulled hair down. Independent
          // vertical dynamics below replace it; zero Y input has zero Y lag.
          expected.y = primaryY
          deformAnime25DSecondaryPoint(
            actual,
            restX,
            restY,
            vertex,
            binding,
            frame,
          )
          deformAnime25DHairPoint(actual, vertex, binding, frame)
          assert.deepEqual(
            actual,
            expected,
            `${binding.baseRole} frame ${frameIndex} vertex ${row}:${column}`,
          )
        }
      }
    }
  }
})

test('hair softness cannot extrapolate beyond either spring while amplitude remains independent', () => {
  for (const front of [false, true]) {
    const binding = secondaryBinding(front ? 'front-hair' : 'back-hair', 'head', false, false, true, front)
    binding.springs = [{ supportX: 0, supportY: 0, stiff: { x: 0, v: 0, dx: -10 }, soft: { x: 0, v: 0, dx: -30 }, vertical: { x: 0, v: 0, dx: 0 }, phase: 0, stiffnessScale: 1, dampingScale: 1 }]
    binding.strandWeights = new Float32Array(VERTEX_COUNT).fill(1)
    binding.alongStrand = new Float32Array(VERTEX_COUNT).fill(1)
    binding.bangWeights = null
    const frame = secondaryFrame(0.4, 1)
    frame.expression.phys = true
    for (const softness of [0, 0.5, 1, 2, 3]) {
      for (const amplitude of [0.5, 3]) {
        frame.expression.soft = frame.expression.fhSoft = softness
        frame.expression.physAmp = frame.expression.fhAmp = amplitude
        const point = { x: 0, y: 0 }
        deformAnime25DHairPoint(point, 0, binding, frame)
        assert.ok(Math.abs(point.x - (-10 - 20 * Math.min(1, softness)) * amplitude) < 1e-8)
      }
    }
  }
})

test('two-axis hair lag rotates into mesh space once and root pins hold both axes', () => {
  const binding = secondaryBinding('back-hair', 'head', false, false, true, false)
  const s = spring(12, 12)
  s.vertical.dx = -8
  binding.springs = [s]
  binding.strandWeights = new Float32Array(VERTEX_COUNT).fill(1)
  binding.alongStrand = new Float32Array(VERTEX_COUNT).fill(1)
  binding.bangWeights = null
  const frame = secondaryFrame(0.5, 1)
  frame.expression.phys = true
  frame.expression.physAmp = 1
  frame.shellActivation = 1
  for (const roll of [-0.3, 0, 0.3]) {
    const c = Math.cos(roll); const sine = Math.sin(roll)
    frame.bodyRotationCosine = c; frame.bodyRotationSine = sine
    const point = { x: 100, y: 100 }
    deformAnime25DHairPoint(point, 0, binding, frame)
    const dx = point.x - 100; const dy = point.y - 100
    assert.ok(Math.abs(dx * c - dy * sine - 12) < 1e-10)
    assert.ok(Math.abs(dx * sine + dy * c + 8) < 1e-10)
    binding.hairlinePinWeights = new Float32Array(VERTEX_COUNT).fill(1)
    const pinned = { x: 100, y: 100 }
    deformAnime25DHairPoint(pinned, 0, binding, frame)
    assert.deepEqual(pinned, { x: 100, y: 100 })
    binding.hairlinePinWeights = null
  }
})

test('ramps the shell into the secondary deformation path from exact legacy output', () => {
  const binding = {
    ...secondaryBinding('face', 'head', false),
    shellMode: 'head' as const,
  }
  const frame = secondaryFrame(0.46, 17)
  frame.expression = { ...frame.expression, angleX: 0.8 }
  frame.headAngleY = -0.35
  frame.headRotationCosine = 1
  frame.headRotationSine = 0
  frame.bodyBreathOffset = 0
  frame.headBreathOffset = 0
  frame.specialHeadOffset = 0
  frame.shellProfile = shellProfile()
  frame.shellRotation = {
    active: false,
    yawCosine: 1,
    yawSine: 0,
    pitchCosine: 1,
    pitchSine: 0,
  }
  writeAnime25DShellRotation(
    frame.expression.angleX,
    frame.headAngleY,
    frame.shellRotation,
  )

  const restX = 124
  const restY = 132
  const legacy = { x: restX, y: restY }
  legacyDeformSecondaryPoint(legacy, restX, restY, 0, binding, frame)

  frame.shellActivation = 0
  frame.shellBlend = 0
  const rampStart = { x: restX, y: restY }
  deformAnime25DSecondaryPoint(rampStart, restX, restY, 0, binding, frame)
  assert.deepEqual(rampStart, legacy)

  frame.shellActivation = 1
  frame.shellBlend = frame.shellProfile.blend
  const active = { x: restX, y: restY }
  deformAnime25DSecondaryPoint(active, restX, restY, 0, binding, frame)
  assert.equal(Number.isFinite(active.x), true)
  assert.equal(Number.isFinite(active.y), true)
  assert.notDeepEqual(active, legacy)
  assert.ok(Math.hypot(active.x - legacy.x, active.y - legacy.y) < 80)
})

test('combined yaw and pitch retain their local head shape when the head rolls', () => {
  for (const mode of ['head', 'front-hair', 'back-hair'] as const) {
    const binding = secondaryBinding(mode === 'head' ? 'face' : mode, 'head', false)
    binding.shellMode = mode
    const base = secondaryFrame(0.63, 21)
    base.shellProfile = shellProfile()
    base.shellBlend = 1
    base.shellActivation = 1
    base.headRotationCosine = 1
    base.headRotationSine = 0
    base.bodyBreathOffset = base.headBreathOffset = base.specialHeadOffset = 0
    const shellRotation = { active: false, yawCosine: 1, yawSine: 0, pitchCosine: 1, pitchSine: 0 }
    base.shellRotation = shellRotation
    for (const yaw of [-1, -0.4, 0.4, 1]) {
      for (const pitch of [-1, -0.5, 0.5, 1]) {
        base.expression.angleX = yaw
        base.headAngleY = pitch
        writeAnime25DShellRotation(yaw, pitch, shellRotation)
        for (const roll of [-0.3, -0.15, 0.15, 0.3]) {
          const c = Math.cos(roll)
          const s = Math.sin(roll)
          const rolled = { ...base, headRotationCosine: c, headRotationSine: s }
          for (const rest of [{ x: 80, y: 100 }, { x: 150, y: 100 }, { x: 117, y: 190 }]) {
            const local = deformSecondary(rest, binding, base)
            const actual = deformSecondary(rest, binding, rolled)
            const dx = local.x - base.neckPivotX
            const dy = local.y - base.neckPivotY
            const error = Math.hypot(actual.x - (base.neckPivotX + c * dx - s * dy), actual.y - (base.neckPivotY + s * dx + c * dy))
            assert.ok(error < 1e-8, `${mode} yaw=${yaw} pitch=${pitch} roll=${roll}: shape drift ${error}`)
          }
        }
      }
    }
  }
})

test('root pinning cannot change the projected volume of the same coiffure', () => {
  const frame = secondaryFrame(0.63, 21)
  frame.shellProfile = shellProfile()
  frame.shellProfile.hair.crownRound = 0.2
  frame.shellBlend = 1
  frame.shellActivation = 1
  const binding = secondaryBinding('front-hair', 'head', false)
  binding.shellMode = 'front-hair'
  for (const yaw of [-1, 1]) {
    for (const pitch of [-1, 1]) {
      frame.expression.angleX = yaw
      frame.headAngleY = pitch
      writeAnime25DShellRotation(yaw, pitch, frame.shellRotation)
      for (const rest of [{ x: 80, y: 70 }, { x: 105, y: 80 }, { x: 150, y: 120 }]) {
        for (const blend of [0, 0.5, 1]) {
          frame.shellBlend = blend
          const free = deformSecondary(rest, binding, frame)
          for (const pin of [0.1, 0.5, 1]) {
            const pinned = deformSecondary(rest, { ...binding, hairlinePinWeights: new Float32Array(VERTEX_COUNT).fill(pin) }, frame)
            assert.ok(Math.hypot(free.x - pinned.x, free.y - pinned.y) < 1e-8, 'pin suppresses relative motion, not the rest surface depth')
          }
        }
      }
    }
  }
})

test('fully pinned hairline vertices reject bang and spring displacement', () => {
  const unpinned = secondaryBinding('front hair', 'head', false, false, true)
  const hairlinePinWeights = new Float32Array(VERTEX_COUNT)
  hairlinePinWeights[VERTEX_COUNT - 1] = 1
  const pinned = { ...unpinned, hairlinePinWeights }
  const frame = secondaryFrame(0.63, 21)
  frame.shellActivation = 1
  const vertex = VERTEX_COUNT - 1
  const rest = { x: 132, y: 98 }
  const moving = { ...rest }
  const fixed = { ...rest }

  deformAnime25DHairPoint(moving, vertex, unpinned, frame)
  deformAnime25DHairPoint(fixed, vertex, pinned, frame)

  assert.notDeepEqual(moving, rest)
  assert.deepEqual(fixed, rest)
})

test('composes torso volume after existing topwear chest deformation', () => {
  const binding = {
    ...secondaryBinding('topwear', 'body', false),
    torsoShellMode: 'full' as const,
  }
  const frame = secondaryFrame(0.58, 23)
  const profile = shellProfile().torso
  assert.ok(profile)
  frame.torsoProfile = profile
  frame.torsoShellBlend = 0.25
  frame.torsoShellRotation = {
    active: true,
    yawCosine: Math.cos(0.24),
    yawSine: Math.sin(0.24),
  }
  frame.torsoChestShape = {
    centerX: 151,
    centerY: 126,
    radiusX: 60,
    radiusY: 48,
    scale: 0.8,
    field: frame.chestField,
  }
  const restX = 151
  const restY = 126
  const vertex = 19
  const expected = { x: restX, y: restY }
  legacyDeformSecondaryPoint(expected, restX, restY, vertex, binding, frame)
  deformAnime25DTorsoShellPoint(
    expected,
    profile,
    frame.torsoShellRotation,
    frame.torsoShellBlend,
    frame.torsoChestShape,
    binding.chestWeights?.[vertex] ?? 1,
    frame.chestVolumeScale,
  )

  const actual = { x: restX, y: restY }
  deformAnime25DSecondaryPoint(actual, restX, restY, vertex, binding, frame)
  assert.deepEqual(actual, expected)
  assert.notEqual(actual.x, restX)
})

test('keeps chest volume off collar layers', () => {
  const binding = {
    ...secondaryBinding('collar_front', 'body', false),
    torsoShellMode: 'collar' as const,
  }
  const withoutChest = secondaryFrame(0.58, 23)
  const withChest = secondaryFrame(0.58, 23)
  const profile = shellProfile().torso
  assert.ok(profile)
  for (const frame of [withoutChest, withChest]) {
    frame.torsoProfile = profile
    frame.torsoShellBlend = 0.25
    frame.torsoShellRotation = {
      active: true,
      yawCosine: Math.cos(0.24),
      yawSine: Math.sin(0.24),
    }
  }
  withChest.torsoChestShape = {
    centerX: 151,
    centerY: 126,
    radiusX: 60,
    radiusY: 48,
    scale: 0.8,
  }

  const rest = { x: 151, y: 126 }
  assert.deepEqual(
    deformSecondary({ ...rest }, binding, withChest),
    deformSecondary({ ...rest }, binding, withoutChest),
  )
})

test('uses the live chest center for both motion and torso volume', () => {
  const binding = {
    ...secondaryBinding('topwear', 'body', false),
    torsoShellMode: 'full' as const,
  }
  const aligned = secondaryFrame(0.58, 23)
  const stale = secondaryFrame(0.58, 23)
  const profile = shellProfile().torso
  assert.ok(profile)
  const rotation = {
    active: true,
    yawCosine: Math.cos(0.24),
    yawSine: Math.sin(0.24),
  }
  for (const frame of [aligned, stale]) {
    frame.torsoProfile = profile
    frame.torsoShellBlend = 0.25
    frame.torsoShellRotation = rotation
    frame.chestMotionCenterY = 154
    frame.torsoChestShape = {
      centerX: 151,
      centerY: 154,
      radiusX: 60,
      radiusY: 48,
      scale: 0.8,
      field: frame.chestField,
    }
  }
  stale.torsoChestShape!.centerY = 106
  const rest = { x: 151, y: 154 }
  const alignedPoint = { ...rest }
  const stalePoint = { ...rest }
  deformAnime25DSecondaryPoint(
    alignedPoint,
    rest.x,
    rest.y,
    19,
    binding,
    aligned,
  )
  deformAnime25DSecondaryPoint(stalePoint, rest.x, rest.y, 19, binding, stale)
  assert.notEqual(alignedPoint.x, stalePoint.x)
})

test('fades fallback front-collar torso motion from body edge to neck seam', () => {
  const binding = {
    ...secondaryBinding('collar_front', 'body', false),
    torsoShellMode: 'collar' as const,
  }
  const frame = secondaryFrame(0.58, 23)
  const profile = shellProfile().torso
  assert.ok(profile)
  frame.torsoProfile = profile
  frame.torsoShellBlend = 0.25
  frame.torsoShellRotation = {
    active: true,
    yawCosine: Math.cos(0.24),
    yawSine: Math.sin(0.24),
  }

  const neckSeamRest = { x: 117, y: 70 }
  const neckSeamLegacy = { ...neckSeamRest }
  legacyDeformSecondaryPoint(
    neckSeamLegacy,
    neckSeamRest.x,
    neckSeamRest.y,
    0,
    binding,
    frame,
  )
  const neckSeamActual = { ...neckSeamRest }
  deformAnime25DSecondaryPoint(
    neckSeamActual,
    neckSeamRest.x,
    neckSeamRest.y,
    0,
    binding,
    frame,
  )
  assert.deepEqual(neckSeamActual, neckSeamLegacy)

  const bodyEdgeRest = { x: 54, y: 224 }
  const bodyEdgeLegacy = { ...bodyEdgeRest }
  legacyDeformSecondaryPoint(
    bodyEdgeLegacy,
    bodyEdgeRest.x,
    bodyEdgeRest.y,
    0,
    binding,
    frame,
  )
  const bodyEdgeActual = { ...bodyEdgeRest }
  deformAnime25DSecondaryPoint(
    bodyEdgeActual,
    bodyEdgeRest.x,
    bodyEdgeRest.y,
    0,
    binding,
    frame,
  )
  assert.notDeepEqual(bodyEdgeActual, bodyEdgeLegacy)
})

test('keeps narrow contact-collar rows coherent at the allowed yaw limit', () => {
  const binding = secondaryBinding('collar_front', 'body', false, true)
  const frame = highCollarFrame()
  const leftOuter = deformSecondary({ x: 498, y: 635.17 }, binding, frame)
  const leftInner = deformSecondary({ x: 500, y: 635.17 }, binding, frame)
  const rightInner = deformSecondary({ x: 599, y: 635.17 }, binding, frame)
  const rightOuter = deformSecondary({ x: 604, y: 635.17 }, binding, frame)

  assert.ok(Math.abs(leftInner.x - leftOuter.x - 2) < 1e-9)
  assert.ok(Math.abs(rightOuter.x - rightInner.x - 5) < 1e-9)
  assert.equal(leftOuter.y, leftInner.y)
  assert.equal(rightInner.y, rightOuter.y)
})

test('front and rear high-collar layers share the same vertical motion field', () => {
  const front = secondaryBinding('collar_front', 'body', false, true)
  const rear = secondaryBinding('collar_back', 'body', false)
  const frame = highCollarFrame()
  const rest = { x: 552, y: 652.17 }

  assert.deepEqual(
    deformSecondary(rest, front, frame),
    deformSecondary(rest, rear, frame),
  )
})

test('fades split-collar torso volume with the inverse neck-follow field', () => {
  const binding = {
    ...secondaryBinding('collar_front', 'body', false, true),
    torsoShellMode: 'collar' as const,
  }
  const withoutTorso = highCollarFrame()
  const withTorso = highCollarFrame()
  const profile = shellProfile().torso
  assert.ok(profile)
  withTorso.torsoProfile = profile
  withTorso.torsoShellBlend = 0.25
  withTorso.torsoShellRotation = {
    active: true,
    yawCosine: Math.cos(0.24),
    yawSine: Math.sin(0.24),
  }

  const top = { x: 604, y: withTorso.neckFollowTop }
  assert.deepEqual(
    deformSecondary(top, binding, withTorso),
    deformSecondary(top, binding, withoutTorso),
  )

  const bottom = { x: 604, y: withTorso.neckBottom }
  assert.notDeepEqual(
    deformSecondary(bottom, binding, withTorso),
    deformSecondary(bottom, binding, withoutTorso),
  )
})

function secondaryBindings(): Anime25DSecondaryDeformationBinding[] {
  return [
    secondaryBinding('face', 'head', false),
    secondaryBinding('body', 'body', false),
    secondaryBinding('neck', 'body', false),
    secondaryBinding('collar_front', 'body', false),
    secondaryBinding('topwear', 'body', false),
    secondaryBinding('handwear', 'body', false),
    secondaryBinding('front hair', 'head', false, false, true),
    secondaryBinding('back hair', 'head', false, false, true, false),
    secondaryBinding('accessory', 'head', true),
  ]
}

function highCollarFrame(): Anime25DSecondaryDeformationFrame {
  const frame = secondaryFrame(0.58, 24)
  frame.expression = { ...frame.expression, angleX: 0.38 }
  frame.faceScale = 1.156156
  frame.headAngleY = 0
  frame.headRotationCosine = 1
  frame.headRotationSine = 0
  frame.neckPivotX = 548.9295
  frame.neckPivotY = 684.9667
  frame.neckBottom = 711.6667
  frame.neckFollowTop = 628.44745
  frame.neckFollowSpan = 83.21925
  frame.bodyBreathOffset = 0
  frame.headBreathOffset = 0
  frame.specialHeadOffset = 0
  frame.highCollar = true
  frame.torsoShellBlend = 0
  return frame
}

function deformSecondary(
  rest: { x: number; y: number },
  binding: Anime25DSecondaryDeformationBinding,
  frame: Anime25DSecondaryDeformationFrame,
): { x: number; y: number } {
  const point = { ...rest }
  deformAnime25DSecondaryPoint(point, rest.x, rest.y, 0, binding, frame)
  return point
}

function secondaryBinding(
  baseRole: string,
  group: Anime25DPlaybackLayer['group'],
  shaderGlobalTransform: boolean,
  collarContact = false,
  hair = false,
  frontHair = true,
): Anime25DSecondaryDeformationBinding {
  const source: Anime25DSecondaryDeformationBinding['source'] = {
    role: baseRole.replaceAll('_', '-').replaceAll('front hair', 'front-hair'),
    group,
    depth: group === 'head' ? 0.78 : 0.91,
    x: 54,
    y: 70,
    w: 126,
    h: 154,
  }
  const alongStrand = hair ? new Float32Array(VERTEX_COUNT) : null
  const frontHairParallaxScale = hair ? new Float32Array(VERTEX_COUNT) : null
  const bangWeights =
    hair && frontHair ? new Float32Array(VERTEX_COUNT * 3) : null
  const strandWeights = hair ? new Float32Array(VERTEX_COUNT * 3) : null
  for (let vertex = 0; vertex < VERTEX_COUNT; vertex += 1) {
    const progress = vertex / (VERTEX_COUNT - 1)
    if (alongStrand) alongStrand[vertex] = progress
    if (frontHairParallaxScale) {
      frontHairParallaxScale[vertex] = 0.2 + progress * 0.8
    }
    if (bangWeights) {
      bangWeights[vertex * 3] = 1 - progress
      bangWeights[vertex * 3 + 1] = 0.35 + progress * 0.3
      bangWeights[vertex * 3 + 2] = progress
    }
    if (strandWeights) {
      strandWeights[vertex * 3] = 0.42 - progress * 0.1
      strandWeights[vertex * 3 + 1] = 0.37
      strandWeights[vertex * 3 + 2] = 0.21 + progress * 0.1
    }
  }
  return createAnime25DSecondaryDeformationBinding({
    source,
    baseRole,
    shaderGlobalTransform,
    collarContact,
    frontHair: hair && frontHair,
    frontHairParallaxScale,
    chestWeights:
      baseRole === 'topwear'
        ? Float32Array.from(
            { length: VERTEX_COUNT },
            (_, vertex) => 0.25 + (vertex % 9) * 0.08,
          )
        : null,
    bangWeights,
    strandWeights,
    alongStrand,
    springs: hair ? hairSprings() : null,
  })
}

function hairSprings(): Anime25DLayerSpringBinding[] {
  return [spring(-2.6, 1.4), spring(0.8, -1.1), spring(3.2, 1.9)]
}

function shellProfile(): Anime25DShellProfile {
  return {
    version: 1,
    source: 'authored',
    enabled: true,
    blend: 0.5,
    head: {
      centerX: 117,
      centerY: 139,
      radiusX: 74,
      radiusY: 96,
      radiusZ: 58,
    },
    faceProfile: {
      enabled: true,
      startY: 74,
      endY: 206,
      points: [
        { v: 0, z: 0 },
        { v: 0.25, z: 0.12 },
        { v: 0.5, z: 0.22 },
        { v: 0.75, z: 0.1 },
        { v: 1, z: 0 },
      ],
    },
    hair: {
      centerX: 117,
      centerY: 139,
      radiusX: 82,
      radiusY: 106,
      radiusZ: 62,
      frontGap: 0.18,
      frontBulge: 1,
      backDepth: 0.35,
      crownRound: 1,
      hairlinePin: {
        enabled: true,
        centerX: 0,
        centerY: -0.45,
        halfWidth: 1.1,
        halfHeight: 0.32,
        feather: 0.06,
      },
    },
    torso: {
      enabled: true,
      blend: 0.5,
      centerX: 117,
      radiusX: 92,
      radiusZ: 54,
    },
  }
}

function spring(stiffDx: number, softDx: number): Anime25DLayerSpringBinding {
  return {
    supportX: 0,
    supportY: 0,
    stiff: { x: 0, v: 0, dx: stiffDx },
    soft: { x: 0, v: 0, dx: softDx },
    vertical: { x: 0, v: 0, dx: 0 },
    phase: 0,
    stiffnessScale: 1,
    dampingScale: 1,
  }
}

function secondaryFrame(
  progress: number,
  frameIndex: number,
): Anime25DSecondaryDeformationFrame {
  const rotation = Math.sin(progress * 4.7) * 0.16
  return {
    expression: {
      ...IDENTITY_DRIVER,
      angleX: Math.sin(progress * 5.1) * 0.85,
      armY: Math.sin(progress * 6.2) * 0.66,
      bangL: Math.sin(progress * 3.2) * 0.7,
      bangC: Math.cos(progress * 4.4) * 0.55,
      bangR: Math.sin(progress * 5.6) * -0.64,
      fhAmp: 0.35 + progress * 1.4,
      fhSoft: 0.2 + progress * 0.7,
      phys: frameIndex % 13 !== 0,
      physAmp: 0.45 + progress * 1.7,
      soft: 0.4 + progress * 1.3,
    },
    faceScale: 0.82,
    armAngleL: Math.sin(progress * 5.9) * 0.3,
    armAngleR: Math.cos(progress * 5.3) * 0.3,
    armDrapeL: Math.sin(progress * 5.1) * 0.2,
    armDrapeR: Math.cos(progress * 4.7) * 0.2,
    headAngleY: Math.cos(progress * 5.4) * 0.9,
    headRotationCosine: Math.cos(rotation),
    headRotationSine: Math.sin(rotation),
    neckPivotX: 117,
    neckPivotY: 161,
    neckBottom: 221,
    neckFollowTop: 148,
    neckFollowSpan: 73,
    faceCenterY: 109,
    bodyBreathOffset: 0.4 + Math.sin(progress * 6.8) * 0.9,
    headBreathOffset: 0.5 + Math.cos(progress * 6.1) * 0.75,
    specialHeadOffset: Math.sin(progress * 7.3) * 5.2,
    highCollar: frameIndex % 2 === 0,
    breath: 0.5 + Math.sin(progress * 5.7) * 0.5,
    chestCenterX: 116,
    chestRegionCenterY: 205,
    chestMotionCenterY: 207 + Math.cos(progress * 3.8) * 13,
    chestRadiusY: 62,
    inverseChestRadiusX: 1 / 78,
    inverseChestRadiusY: 1 / 62,
    chestOffsetX: frameIndex % 11 === 0 ? 0 : Math.sin(progress * 7.2) * 5,
    chestOffsetY: frameIndex % 11 === 0 ? 0 : Math.cos(progress * 6.4) * 7,
    chestField: resolveChestSpatialField({
      source: frameIndex % 3 === 0 ? 'ai-vision' : 'geometry-fallback',
      supportScale: 0.35,
      garmentMotionScale: 0.8,
    }),
    chestVolumeScale: 1,
    shellProfile: shellProfile(),
    shellBlend: 0,
    shellActivation: 0,
    shellRotation: {
      active: false,
      yawCosine: 1,
      yawSine: 0,
      pitchCosine: 1,
      pitchSine: 0,
    },
    torsoProfile: shellProfile().torso,
    torsoChestShape: null,
    torsoShellBlend: 0,
    torsoNeckOffsetX: 0,
    bodyRotationCosine: 1,
    bodyRotationSine: 0,
    torsoShellRotation: { active: false, yawCosine: 1, yawSine: 0 },
  }
}

function legacyDeformSecondaryPoint(
  point: { x: number; y: number },
  restX: number,
  restY: number,
  vertex: number,
  binding: Anime25DSecondaryDeformationBinding,
  frame: Anime25DSecondaryDeformationFrame,
): void {
  const { source } = binding
  const baseRole = binding.baseRole
  const isTopwear = baseRole === 'topwear'
  const isFrontCollar = baseRole === 'collar_front'
  const isHead = source.group === 'head'
  if (!binding.shaderGlobalTransform) {
    const neckFollowProgress =
      baseRole === 'neck'
        ? clamp((frame.neckBottom - restY) / frame.neckFollowSpan, 0, 1)
        : 0
    const neckFollowInput = frame.highCollar
      ? neckFollowProgress ** HIGH_COLLAR_NECK_FOLLOW_POWER
      : neckFollowProgress
    const neckHeadBlend = baseRole === 'neck' ? smoothstep(neckFollowInput) : 0
    const frontCollarProgress =
      isFrontCollar && !binding.collarContact
        ? clamp(
            1 -
              (restY - source.y) /
                Math.max(1, source.h * FRONT_COLLAR_FLEX_REGION),
            0,
            1,
          )
        : 0
    const frontCollarLocalX =
      isFrontCollar && !binding.collarContact
        ? Math.abs(
            (restX - (source.x + source.w / 2)) / Math.max(1, source.w / 2),
          )
        : 1
    const frontCollarInnerWeight =
      isFrontCollar && !binding.collarContact
        ? smoothstep((1 - frontCollarLocalX) / FRONT_COLLAR_INNER_REGION)
        : 0
    const frontCollarHeadBlend =
      smoothstep(frontCollarProgress) *
      frontCollarInnerWeight *
      FRONT_COLLAR_HEAD_FOLLOW
    let headFollow = isHead ? 1 : source.group === 'body' ? BODY_HEAD_FOLLOW : 0
    if (baseRole === 'neck') {
      headFollow = BODY_HEAD_FOLLOW + (1 - BODY_HEAD_FOLLOW) * neckHeadBlend
    } else if (isFrontCollar && !binding.collarContact) {
      headFollow =
        BODY_HEAD_FOLLOW + (1 - BODY_HEAD_FOLLOW) * frontCollarHeadBlend
    }
    if (!binding.collarContact && headFollow > 0) {
      const rotationX = point.x - frame.neckPivotX
      const rotationY = point.y - frame.neckPivotY
      const rotatedX =
        rotationX * frame.headRotationCosine -
        rotationY * frame.headRotationSine
      const rotatedY =
        rotationX * frame.headRotationSine +
        rotationY * frame.headRotationCosine
      point.x += (rotatedX - rotationX) * headFollow
      point.y += (rotatedY - rotationY) * headFollow
      let depthOffset =
        (source.depth - 1) * (binding.frontHairParallaxScale?.[vertex] ?? 1)
      if (baseRole === 'neck') depthOffset *= 1 - neckHeadBlend
      else if (isFrontCollar) depthOffset *= 1 - frontCollarHeadBlend
      point.x +=
        headFollow *
        frame.faceScale *
        (frame.expression.angleX * (14 + 40 * depthOffset) +
          frame.expression.angleX * (frame.neckPivotY - point.y) * 0.028)
      point.y +=
        headFollow *
        frame.faceScale *
        (-frame.headAngleY * (9 + 30 * depthOffset) -
          frame.headAngleY * depthOffset * (point.y - frame.faceCenterY) * 0.05)
    }
    if (!binding.collarContact && frame.specialHeadOffset !== 0) {
      const specialHeadFollow = isHead
        ? 1
        : baseRole === 'neck'
          ? neckHeadBlend
          : isFrontCollar
            ? frontCollarHeadBlend
            : 0
      point.y += frame.specialHeadOffset * specialHeadFollow
    }
    if (!binding.collarContact) {
      const breathOffset = isHead
        ? frame.headBreathOffset
        : baseRole === 'neck'
          ? frame.bodyBreathOffset +
            (frame.headBreathOffset - frame.bodyBreathOffset) * neckHeadBlend
          : isFrontCollar
            ? frame.bodyBreathOffset +
              (frame.headBreathOffset - frame.bodyBreathOffset) *
                frontCollarHeadBlend
            : frame.bodyBreathOffset
      point.y -= breathOffset * frame.faceScale
    }
  }
  if (isTopwear && point.y < frame.chestRegionCenterY) {
    point.y -=
      frame.breath *
      2.2 *
      frame.faceScale *
      smoothstep(
        (frame.chestRegionCenterY - point.y) / (frame.chestRadiusY * 2),
      )
  }
  if (isTopwear) {
    point.x =
      frame.neckPivotX +
      (point.x - frame.neckPivotX) * (1 + frame.breath * 0.003)
  }
  if (isTopwear && (frame.chestOffsetX !== 0 || frame.chestOffsetY !== 0)) {
    const normalizedX = (restX - frame.chestCenterX) * frame.inverseChestRadiusX
    const normalizedY =
      (restY - frame.chestMotionCenterY) * frame.inverseChestRadiusY
    const skinWeight = binding.chestWeights?.[vertex] ?? 1
    const chestWeight = chestDeformationWeight(
      frame.chestField,
      normalizedX,
      normalizedY,
      skinWeight,
    )
    point.x += frame.chestOffsetX * chestWeight
    point.y += frame.chestOffsetY * chestWeight
  }
  if (baseRole === 'handwear') {
    // These bindings carry no arm rig: a sleeve without a joint only shrugs.
    point.y -= frame.expression.armY * ARM_SHRUG * frame.faceScale
  }
}

function legacyDeformHairPoint(
  point: { x: number; y: number },
  vertex: number,
  binding: Anime25DSecondaryDeformationBinding,
  frame: Anime25DSecondaryDeformationFrame,
): void {
  if (binding.bangWeights && binding.alongStrand) {
    const along = binding.alongStrand[vertex]
    const amplitude = along ** 1.4 * 22 * frame.faceScale
    point.x +=
      (frame.expression.bangL * binding.bangWeights[vertex * 3] +
        frame.expression.bangC * binding.bangWeights[vertex * 3 + 1] +
        frame.expression.bangR * binding.bangWeights[vertex * 3 + 2]) *
      amplitude
  }
  const strandCount = binding.springs?.length ?? 0
  if (
    !strandCount ||
    !binding.springs ||
    !binding.strandWeights ||
    !binding.alongStrand ||
    !frame.expression.phys
  ) {
    return
  }
  const along = binding.alongStrand[vertex]
  const easedAlong = binding.frontHair ? Math.min(1, along * 1.6) : along
  const amplitude =
    easedAlong ** (binding.frontHair ? 1.8 : 2.1) *
    (binding.frontHair ? frame.expression.fhAmp : frame.expression.physAmp)
  const softMix =
    easedAlong ** 1.2 *
    (binding.frontHair ? frame.expression.fhSoft : frame.expression.soft)
  let offsetX = 0
  for (let strand = 0; strand < strandCount; strand += 1) {
    const weight = binding.strandWeights[vertex * strandCount + strand]
    if (weight < 0.001) continue
    const spring = binding.springs[strand]
    offsetX +=
      weight * (spring.stiff.dx * (1 - softMix) + spring.soft.dx * softMix)
  }
  const offset = offsetX * amplitude
  point.x += offset
  point.y += Math.abs(offset) * 0.12
}

function smoothstep(value: number): number {
  const bounded = clamp(value, 0, 1)
  return bounded * bounded * (3 - 2 * bounded)
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value))
}

function torsoTurnFrame(
  yawRadians: number,
  armAngle: number,
  armY = 0,
): Anime25DSecondaryDeformationFrame {
  const frame = secondaryFrame(0, 0)
  frame.expression = { ...frame.expression, armY }
  frame.breath = 0
  frame.torsoShellBlend = 0.5
  frame.torsoShellRotation = {
    active: yawRadians !== 0,
    yawCosine: Math.cos(yawRadians),
    yawSine: Math.sin(yawRadians),
  }
  frame.armAngleL = armAngle
  frame.armAngleR = armAngle
  frame.armDrapeL = armAngle
  frame.armDrapeR = armAngle
  return frame
}

const SLEEVE_BOUNDS = { L: { x: 30, w: 60 }, R: { x: 144, w: 60 } }

function bodyBinding(
  baseRole: string,
  side: Anime25DPlaybackLayer['side'],
  override?: { x: number; w: number },
): Anime25DSecondaryDeformationBinding {
  const bounds =
    override ??
    (baseRole === 'handwear' && side ? SLEEVE_BOUNDS[side] : { x: 54, w: 126 })
  return createAnime25DSecondaryDeformationBinding({
    source: {
      role: baseRole,
      group: 'body',
      depth: 0.91,
      side,
      x: bounds.x,
      y: 70,
      w: bounds.w,
      h: 154,
    },
    baseRole,
    shaderGlobalTransform: false,
    collarContact: false,
    frontHair: false,
    frontHairParallaxScale: null,
    chestWeights: null,
    bangWeights: null,
    strandWeights: null,
    alongStrand: null,
    springs: null,
    torsoShellMode: anime25DTorsoShellModeForLayer({
      group: 'body',
      role: baseRole,
    }),
  })
}

function turnedX(
  binding: Anime25DSecondaryDeformationBinding,
  yawRadians: number,
  armAngle: number,
  sampleX = 60,
  armY = 0,
): number {
  const restX = sampleX
  const restY = 180
  const rest = { x: restX, y: restY }
  deformAnime25DSecondaryPoint(
    rest,
    restX,
    restY,
    0,
    binding,
    torsoTurnFrame(0, 0),
  )
  const turned = { x: restX, y: restY }
  deformAnime25DSecondaryPoint(
    turned,
    restX,
    restY,
    0,
    binding,
    torsoTurnFrame(yawRadians, armAngle, armY),
  )
  return turned.x - rest.x
}

test('a turning garment carries the sleeve instead of sliding out from under it', () => {
  const garment = turnedX(bodyBinding('topwear', null), 0.45, 0)
  const sleeve = turnedX(bodyBinding('handwear', 'L'), 0.45, 0)
  assert.ok(Math.abs(garment) > 1)
  assert.ok(Math.sign(sleeve) === Math.sign(garment))
  // The sleeve hangs beside the cylinder rather than on it, so it takes most of the turn
  assert.ok(
    Math.abs(sleeve / garment - SLEEVE_TORSO_TRANSMISSION) < 1e-9,
    `${sleeve / garment}`,
  )
})

test('exposed shoulder seam shares body motion while the distal arm stays free', () => {
  const torso = bodyBinding('topwear', null)
  const arm = bodyBinding('handwear', 'L')
  const host = {
    rest: new Float32Array([90, 100, 110, 100, 90, 120]),
    deformed: new Float32Array(6),
    indices: new Uint16Array([0, 1, 2]),
  }
  const rest = new Float32Array([90, 100, 90, 180])
  const contact = bindSurfaceContact(host, rest, new Float32Array([1, 0]))
  for (const yaw of [-0.6, 0, 0.6]) {
    for (const lift of [-1, 0, 1]) {
      const frame = torsoTurnFrame(yaw, 0.8, lift)
      frame.breath = 0.8
      for (let i = 0; i < host.rest.length; i += 2) {
        const p = deformSecondary({ x: host.rest[i], y: host.rest[i + 1] }, torso, frame)
        // A later host correction must reach the seam too, without replaying the torso formula.
        host.deformed[i] = p.x + 0.75
        host.deformed[i + 1] = p.y - 0.25
      }
      const free = new Float32Array(rest.length)
      for (let i = 0; i < rest.length; i += 2) {
        const p = deformSecondary({ x: rest[i], y: rest[i + 1] }, arm, frame)
        free[i] = p.x
        free[i + 1] = p.y
      }
      const actual = free.slice()
      applySurfaceContact(contact, actual)
      assert.deepEqual(actual.slice(0, 2), host.deformed.slice(0, 2))
      assert.deepEqual(actual.slice(2), free.slice(2))
    }
  }
})

test('turning does not change how wide either sleeve is', () => {
  for (const side of ['L', 'R'] as const) {
    const binding = bodyBinding('handwear', side)
    const bounds = SLEEVE_BOUNDS[side]
    for (const yaw of [0.15, 0.3, 0.45, 0.6]) {
      const near = turnedX(binding, yaw, 0, bounds.x)
      const far = turnedX(binding, yaw, 0, bounds.x + bounds.w)
      assert.ok(Math.abs(far - near) < 1e-9, `${side} ${yaw}: ${far - near}`)
    }
  }
})

test('a turn still moves both sleeves, by amounts their positions decide', () => {
  const left = turnedX(bodyBinding('handwear', 'L'), 0.45, 0)
  const right = turnedX(bodyBinding('handwear', 'R'), 0.45, 0, 174)
  assert.ok(left > 1 && right > 0)
  assert.ok(left > right, `${left} !> ${right}`)
})

const ARM = {
  outward: 1 as const,
  pivotX: 70,
  pivotY: 90,
  radius: 12,
  reach: 300,
  length: 134,
  scale: 1,
  cutY: 224,
  drape: false,
}

function riggedSleeve(cutY: number | null = ARM.cutY, drape = false): Anime25DSecondaryDeformationBinding {
  const arm = { ...ARM, cutY, drape }
  const rest = new Float32Array([
    arm.pivotX, arm.pivotY, 40, 150, 80, 150, 40, 200, 80, 200, 40, 224, 80, 224, 60, 130,
  ])
  return createAnime25DSecondaryDeformationBinding({
    ...bodyBinding('handwear', 'L'),
    arm,
    armMesh: bindArmRigMesh(arm, rest),
  })
}

function swung(
  binding: Anime25DSecondaryDeformationBinding,
  vertex: number,
  restX: number,
  restY: number,
  angle: number,
  armY = 0,
  drape = angle,
): { x: number; y: number } {
  const point = { x: restX, y: restY }
  const frame = torsoTurnFrame(0, angle, armY)
  frame.armDrapeL = drape
  deformAnime25DSecondaryPoint(point, restX, restY, vertex, binding, frame)
  return point
}

test('a sleeve swings rigidly about its shoulder joint', () => {
  const sleeve = riggedSleeve(null)
  assert.deepEqual(
    swung(sleeve, 0, ARM.pivotX, ARM.pivotY, 0.3),
    swung(sleeve, 0, ARM.pivotX, ARM.pivotY, 0),
  )
  // Clear of the shoulder, the swing is exactly a rotation about the joint,
  // on top of whatever else carries the sleeve.
  for (const [vertex, x, y] of [[1, 40, 150], [4, 80, 200], [3, 40, 200]] as const) {
    const moved = swung(sleeve, vertex, x, y, 0.3)
    const still = swung(sleeve, vertex, x, y, 0)
    const dx = x - ARM.pivotX
    const dy = y - ARM.pivotY
    const expectedX = dx * Math.cos(0.3) - dy * Math.sin(0.3) - dx
    const expectedY = dx * Math.sin(0.3) + dy * Math.cos(0.3) - dy
    assert.ok(Math.abs(moved.x - still.x - expectedX) < 1e-9)
    assert.ok(Math.abs(moved.y - still.y - expectedY) < 1e-9)
  }
})

test('a positive swing carries a hanging hand toward image left', () => {
  const sleeve = riggedSleeve(null)
  const rest = swung(sleeve, 3, 40, 200, 0)
  assert.ok(swung(sleeve, 3, 40, 200, 0.2).x < rest.x - 10)
  assert.ok(swung(sleeve, 3, 40, 200, -0.2).x > rest.x + 10)
})

test('the arm bends into the shoulder instead of tearing from it', () => {
  const sleeve = riggedSleeve(null)
  const weights = sleeve.armMesh!.weights
  assert.equal(weights[0], 0)
  assert.equal(weights[1], 1)
  const near = swung(sleeve, 7, 60, 130, 0.3)
  const rigid = { x: ARM.pivotX + (60 - ARM.pivotX) * Math.cos(0.3) - (130 - ARM.pivotY) * Math.sin(0.3) }
  assert.ok(weights[7] > 0 && weights[7] <= 1)
  assert.ok(Math.abs(near.x - 60) <= Math.abs(rigid.x - 60) + 1e-9)
})

test('a cropped arm slides along the frame line instead of lifting off it', () => {
  const sleeve = riggedSleeve()
  for (const angle of [-0.35, -0.2, 0.2, 0.35]) {
    for (const lift of [-1, 0, 1]) {
      for (const [vertex, x] of [[5, 40], [6, 80]] as const) {
        const cut = swung(sleeve, vertex, x, ARM.cutY, angle, lift)
        const rest = swung(sleeve, vertex, x, ARM.cutY, 0, 0)
        assert.ok(Math.abs(cut.y - rest.y) < 1e-9, `${angle} ${lift} ${cut.y} ${rest.y}`)
        assert.ok(Math.abs(cut.x - rest.x) > 20, `${angle}`)
      }
    }
  }
  // Without a crop the same swing lifts the hand, as a rotation must.
  const free = riggedSleeve(null)
  assert.ok(swung(free, 5, 40, ARM.cutY, 0.35).y < swung(free, 5, 40, ARM.cutY, 0).y - 5)
})

test('a drape follows the arm at the shoulder and its own swing at the hem', () => {
  const draped = riggedSleeve(null, true)
  const plain = riggedSleeve(null, false)
  // Near the joint the cloth is the arm.
  assert.deepEqual(swung(draped, 7, 60, 130, 0.3, 0, 0.1), swung(plain, 7, 60, 130, 0.3))
  // At the hem it takes the drape's angle, not the arm's.
  const hem = swung(draped, 3, 40, ARM.pivotY + ARM.length, 0.3, 0, 0.1)
  const asDrape = swung(plain, 3, 40, ARM.pivotY + ARM.length, 0.1)
  assert.ok(Math.abs(hem.x - asDrape.x) < 1e-9 && Math.abs(hem.y - asDrape.y) < 1e-9)
  // A sleeve with its forearm showing never bends: the arm's angle all the way.
  assert.deepEqual(
    swung(plain, 3, 40, ARM.pivotY + ARM.length, 0.3, 0, 0.1),
    swung(plain, 3, 40, ARM.pivotY + ARM.length, 0.3),
  )
})

test('a cropped drape still slides its cut along the frame line', () => {
  const draped = riggedSleeve(ARM.cutY, true)
  for (const [angle, drape] of [[0.3, 0.1], [-0.25, -0.05], [0.1, 0.25]]) {
    const cut = swung(draped, 5, 40, ARM.cutY, angle, 0, drape)
    const rest = swung(draped, 5, 40, ARM.cutY, 0, 0, 0)
    assert.ok(Math.abs(cut.y - rest.y) < 1e-9, `${angle} ${drape}`)
  }
})

test('lifting raises the shoulder of every sleeve layer the same amount', () => {
  for (const binding of [bodyBinding('handwear', null, { x: 60, w: 120 }), riggedSleeve(null)]) {
    for (const [x, y] of [[70, 90], [100, 180], [60, 150]] as const) {
      const flat = { x, y }
      deformAnime25DSecondaryPoint(flat, x, y, 0, binding, torsoTurnFrame(0, 0))
      const lifted = { x, y }
      deformAnime25DSecondaryPoint(lifted, x, y, 0, binding, torsoTurnFrame(0, 0, 0.6))
      assert.equal(lifted.x, flat.x)
      assert.ok(Math.abs(flat.y - lifted.y - 0.6 * ARM_SHRUG * 0.82) < 1e-9)
    }
  }
})

test('an undivided sleeve layer never rotates apart', () => {
  const single = bodyBinding('handwear', null, { x: 60, w: 120 })
  assert.equal(single.arm, null)
  for (const sampleX of [70, 100, 140, 170]) {
    assert.equal(turnedX(single, 0, 0.3, sampleX), 0)
  }
})

test('an outboard sleeve drawing is still carried by the turn', () => {
  const garment = turnedX(bodyBinding('topwear', null), 0.45, 0)
  const outboard = turnedX(bodyBinding('handwear', 'R', { x: 200, w: 60 }), 0.45, 0, 230)
  assert.ok(garment > 0)
  assert.ok(outboard > 0, `${outboard}`)
})

test('a head tilt swings hair near the neck but not the length resting on the body or the cut', () => {
  const roll = 0.2
  const tilted = (frame: Anime25DSecondaryDeformationFrame, headRoll: number) => {
    frame.headRoll = headRoll
    frame.headRotationCosine = Math.cos(headRoll)
    frame.headRotationSine = Math.sin(headRoll)
    frame.hairDrapeLength = 60
    frame.bodyPivotY = 400
    frame.bodyBendHeight = 400 - frame.neckBottom
    return frame
  }
  const still = tilted(secondaryFrame(0.3, 13), 0)
  const moved = tilted(secondaryFrame(0.3, 13), roll)
  const turned = (role: string, group: Anime25DPlaybackLayer['group'], y: number) => {
    const binding = secondaryBinding(role, group, false)
    const a = deformSecondary({ x: 150, y }, binding, still)
    const b = deformSecondary({ x: 150, y }, binding, moved)
    const radius = Math.hypot(a.x - still.neckPivotX, a.y - still.neckPivotY)
    return Math.hypot(b.x - a.x, b.y - a.y) / radius
  }
  // Beside the face the hair turns with the whole tilt.
  assert.ok(Math.abs(turned('back-hair', 'head', 120) - roll) < 0.01)
  // Hanging past the shoulders it keeps only a little of it, and none at the cut.
  assert.ok(turned('back-hair', 'head', 300) < roll * 0.2)
  assert.ok(turned('back-hair', 'head', 400) < 1e-9)
  // The torso's share of a head tilt fades out onto the cut as well.
  assert.ok(turned('topwear', 'body', 400) < 1e-9)
})
