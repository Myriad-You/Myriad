import type { Anime25DChestProfile, Anime25DTorsoShellProfile } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  anime25DLayerUsesTorsoShell,
  anime25DSleeveAnchorX,
  anime25DTorsoShellModeForLayer,
  anime25DTorsoShellOffsetX,
  anime25DTorsoYawFollow,
  deformAnime25DTorsoShellPoint,
  resolveAnime25DTorsoChestShape,
  stepAnime25DTorsoShellRotation,
} from './torsoDeformation'

const profile: Anime25DTorsoShellProfile = {
  enabled: true,
  blend: 0.5,
  centerX: 384,
  radiusX: 292.6,
  radiusZ: 169.4,
}

const chestProfile: Anime25DChestProfile = {
  version: 2,
  enabled: true,
  source: 'ai-vision',
  centerX: 384,
  centerY: 612.5,
  radiusX: 140,
  radiusY: 90,
  visibleScale: 0.8,
  motionScale: 1.1,
  frequencyScale: 0.94,
  supportScale: 0.2,
  garmentMotionScale: 1,
  confidence: 0.9,
}

test('binds body garments fully and both split-collar layers through one field', () => {
  for (const role of ['topwear', 'bottomwear'] as const) {
    const source = { group: 'body' as const, role }
    assert.equal(anime25DTorsoShellModeForLayer(source), 'full')
    assert.equal(anime25DLayerUsesTorsoShell(source), true)
  }
  for (const role of ['collar-front', 'collar-back'] as const) {
    const source = { group: 'body' as const, role }
    assert.equal(anime25DTorsoShellModeForLayer(source), 'collar')
    assert.equal(anime25DLayerUsesTorsoShell(source), true)
  }
  const sleeve = { group: 'body' as const, role: 'handwear' }
  assert.equal(anime25DTorsoShellModeForLayer(sleeve), 'sleeve')
  assert.equal(anime25DLayerUsesTorsoShell(sleeve), true)
  assert.equal(
    anime25DTorsoShellModeForLayer({ group: 'body', role: 'neck' }),
    'neck',
  )
  for (const source of [
    { group: 'body', role: 'neckwear' },
    { group: 'head', role: 'topwear' },
  ] as const) {
    assert.equal(anime25DTorsoShellModeForLayer(source), null)
    assert.equal(anime25DLayerUsesTorsoShell(source), false)
  }
})

test('matches the fork low-pass torso yaw response', () => {
  const state = { value: 0 }
  const rotation = { active: false, yawCosine: 1, yawSine: 0 }
  stepAnime25DTorsoShellRotation(state, 1, 0, 0.05, rotation)
  const expectedYaw = 0.45 * (0.05 * 2.5)
  assert.equal(state.value, expectedYaw)
  assert.equal(rotation.active, true)
  assert.equal(rotation.yawCosine, Math.cos(expectedYaw))
  assert.equal(rotation.yawSine, Math.sin(expectedYaw))

  stepAnime25DTorsoShellRotation(state, 1, -0.5, 0, rotation)
  assert.equal(state.value, expectedYaw)
})

test('the per-model follow scales the head share and leaves the body alone', () => {
  const stepped = (
    angleX: number,
    body: number,
    yawFollowScale?: number,
  ): number => {
    const state = { value: 0 }
    const rotation = { active: false, yawCosine: 1, yawSine: 0 }
    stepAnime25DTorsoShellRotation(
      state,
      angleX,
      body,
      0.05,
      rotation,
      yawFollowScale,
    )
    return state.value
  }
  assert.ok(Math.abs(stepped(1, 0, 0.5) - stepped(1, 0) * 0.5) < 1e-12)
  assert.equal(stepped(1, 0, 0), 0)
  assert.equal(stepped(0, 1, 0), stepped(0, 1))
  assert.equal(stepped(0, 1, 0.25), stepped(0, 1))
})

test('a manifest with no authored follow, or a bad one, turns fully', () => {
  const full = (() => {
    const state = { value: 0 }
    stepAnime25DTorsoShellRotation(state, 1, 0, 0.05, {
      active: false,
      yawCosine: 1,
      yawSine: 0,
    })
    return state.value
  })()
  for (const scale of [undefined, Number.NaN, Number.POSITIVE_INFINITY, 4, 1]) {
    const state = { value: 0 }
    stepAnime25DTorsoShellRotation(
      state,
      1,
      0,
      0.05,
      { active: false, yawCosine: 1, yawSine: 0 },
      scale,
    )
    assert.equal(state.value, full, `${scale}`)
  }
  const negative = { value: 0 }
  stepAnime25DTorsoShellRotation(
    negative,
    1,
    0,
    0.05,
    { active: false, yawCosine: 1, yawSine: 0 },
    -2,
  )
  assert.equal(negative.value, 0)
})

test('keeps the frontal pose exact and matches the fork cylinder projection', () => {
  const chestShape = resolveAnime25DTorsoChestShape(chestProfile)
  assert.ok(chestShape)
  const neutral = { x: 417.25, y: 612.5 }
  deformAnime25DTorsoShellPoint(
    neutral,
    profile,
    { active: true, yawCosine: 1, yawSine: 0 },
    0.25,
    chestShape,
  )
  assert.deepEqual(neutral, { x: 417.25, y: 612.5 })

  const yaw = 0.31
  const turned = { x: 521.4, y: 731.2 }
  const expectedX = forkTorsoProjectionX(turned.x, yaw, 0.25)
  deformAnime25DTorsoShellPoint(
    turned,
    profile,
    {
      active: true,
      yawCosine: Math.cos(yaw),
      yawSine: Math.sin(yaw),
    },
    0.25,
  )
  assert.equal(turned.x, expectedX)
  assert.equal(turned.y, 731.2)
})

test('adds bounded paired chest volume and near-side silhouette only while turning', () => {
  const softShape = resolveAnime25DTorsoChestShape(chestProfile)
  const structuredShape = resolveAnime25DTorsoChestShape({
    ...chestProfile,
    garmentMotionScale: 0,
  })
  assert.ok(softShape)
  assert.ok(structuredShape)
  const yaw = 0.31
  const rightX = chestProfile.centerX + chestProfile.radiusX * 0.58
  const leftX = chestProfile.centerX - chestProfile.radiusX * 0.58
  const baseRight = projectedX(rightX, chestProfile.centerY, yaw, null)
  const baseLeft = projectedX(leftX, chestProfile.centerY, yaw, null)
  const softRight = projectedX(rightX, chestProfile.centerY, yaw, softShape)
  const softLeft = projectedX(leftX, chestProfile.centerY, yaw, softShape)
  const structuredRight = projectedX(
    rightX,
    chestProfile.centerY,
    yaw,
    structuredShape,
  )
  const rightGain = softRight - baseRight
  const leftGain = softLeft - baseLeft

  assert.ok(rightGain > leftGain)
  assert.ok(leftGain > 0)
  assert.ok(softRight - baseRight > structuredRight - baseRight)
  assert.equal(
    projectedX(
      rightX,
      chestProfile.centerY + chestProfile.radiusY * 2,
      yaw,
      softShape,
    ),
    projectedX(
      rightX,
      chestProfile.centerY + chestProfile.radiusY * 2,
      yaw,
      null,
    ),
  )
  assert.equal(
    resolveAnime25DTorsoChestShape({ ...chestProfile, enabled: false }),
    null,
  )
})

test('stays finite outside the fitted torso silhouette', () => {
  const rotation = {
    active: true,
    yawCosine: Math.cos(-0.8),
    yawSine: Math.sin(-0.8),
  }
  for (const x of [-2_000, 0, 384, 768, 2_000]) {
    const point = { x, y: 820 }
    deformAnime25DTorsoShellPoint(point, profile, rotation, 0.25)
    assert.equal(Number.isFinite(point.x), true)
    assert.equal(point.y, 820)
  }
})

test('uses the same geometry weight to suppress dynamic and yaw chest volume', () => {
  const geometryShape = resolveAnime25DTorsoChestShape({
    ...chestProfile,
    source: 'geometry-fallback',
  })
  assert.ok(geometryShape)
  const yaw = 0.31
  const x = chestProfile.centerX + chestProfile.radiusX * 0.58
  const base = projectedX(x, chestProfile.centerY, yaw, null)
  const suppressed = { x, y: chestProfile.centerY }
  deformAnime25DTorsoShellPoint(
    suppressed,
    profile,
    {
      active: true,
      yawCosine: Math.cos(yaw),
      yawSine: Math.sin(yaw),
    },
    0.25,
    geometryShape,
    0,
  )
  assert.equal(suppressed.x, base)
})

test('breathing modulates only projected depth and preserves frontal identity', () => {
  const shape = resolveAnime25DTorsoChestShape(chestProfile)
  assert.ok(shape)
  const x = chestProfile.centerX + chestProfile.radiusX * 0.58
  const yaw = 0.31
  const quiet = { x, y: chestProfile.centerY }
  const inhale = { ...quiet }
  const frontal = { ...quiet }
  const rotation = {
    active: true,
    yawCosine: Math.cos(yaw),
    yawSine: Math.sin(yaw),
  }
  deformAnime25DTorsoShellPoint(quiet, profile, rotation, 0.25, shape, 1, 1)
  deformAnime25DTorsoShellPoint(inhale, profile, rotation, 0.25, shape, 1, 1.04)
  deformAnime25DTorsoShellPoint(
    frontal,
    profile,
    { active: true, yawCosine: 1, yawSine: 0 },
    0.25,
    shape,
    1,
    1.04,
  )
  assert.notEqual(inhale.x, quiet.x)
  assert.equal(frontal.x, x)
})

function forkTorsoProjectionX(x: number, yaw: number, blend: number): number {
  const localX = x - profile.centerX
  const normalizedX = localX / profile.radiusX
  const z =
    Math.sqrt(Math.max(0, 1 - Math.min(1, normalizedX * normalizedX))) *
    profile.radiusZ
  const rotatedX = localX * Math.cos(yaw) + z * Math.sin(yaw)
  const rotatedZ = -localX * Math.sin(yaw) + z * Math.cos(yaw)
  const focalLength = profile.radiusZ * 6
  const rotatedScale = focalLength / Math.max(1, focalLength - rotatedZ * 0.5)
  const restScale = focalLength / Math.max(1, focalLength - z * 0.5)
  return x + (rotatedX * rotatedScale - localX * restScale) * blend
}

function projectedX(
  x: number,
  y: number,
  yaw: number,
  chestShape: ReturnType<typeof resolveAnime25DTorsoChestShape>,
): number {
  const point = { x, y }
  deformAnime25DTorsoShellPoint(
    point,
    profile,
    {
      active: true,
      yawCosine: Math.cos(yaw),
      yawSine: Math.sin(yaw),
    },
    0.25,
    chestShape,
  )
  return point.x
}

const SLEEVE_TORSO = {
  enabled: true,
  blend: 0.5,
  centerX: 117,
  radiusX: 92,
  radiusZ: 54,
}

function sleeveCarry(normalizedX: number, yawRadians: number): number {
  const anchorX = SLEEVE_TORSO.centerX + normalizedX * SLEEVE_TORSO.radiusX
  return anime25DTorsoShellOffsetX(
    anime25DSleeveAnchorX(anchorX, SLEEVE_TORSO),
    SLEEVE_TORSO,
    {
      active: yawRadians !== 0,
      yawCosine: Math.cos(yawRadians),
      yawSine: Math.sin(yawRadians),
    },
    0.5,
  )
}

test('a sleeve drawn past the body silhouette is not dragged against it', () => {
  for (const yaw of [0.2, 0.45, 0.6]) {
    const garment = anime25DTorsoShellOffsetX(
      SLEEVE_TORSO.centerX - 0.5 * SLEEVE_TORSO.radiusX,
      SLEEVE_TORSO,
      { active: true, yawCosine: Math.cos(yaw), yawSine: Math.sin(yaw) },
      0.5,
    )
    assert.ok(garment > 0)
    for (const normalizedX of [-1.5, -1.25, -0.9]) {
      assert.ok(sleeveCarry(normalizedX, yaw) > 0, `${normalizedX} @ ${yaw}`)
    }
    for (const normalizedX of [0.9, 1.25, 1.5]) {
      const carry = sleeveCarry(normalizedX, yaw)
      assert.ok(carry > -0.05 * garment, `${normalizedX} @ ${yaw}: ${carry}`)
    }
  }
})

test('how far out a sleeve is drawn stops mattering past the shoulder', () => {
  for (const side of [-1, 1]) {
    const shoulder = sleeveCarry(side * 0.8, 0.45)
    for (const normalizedX of [0.9, 1, 1.25, 1.5, 3]) {
      assert.ok(
        Math.abs(sleeveCarry(side * normalizedX, 0.45) - shoulder) < 1e-9,
        `${side * normalizedX}`,
      )
    }
  }
})

test('a sleeve drawn on the body reads the turn where it is drawn', () => {
  for (const normalizedX of [-0.8, -0.62, -0.3, 0.3, 0.62, 0.8]) {
    const anchorX = SLEEVE_TORSO.centerX + normalizedX * SLEEVE_TORSO.radiusX
    assert.ok(Math.abs(anime25DSleeveAnchorX(anchorX, SLEEVE_TORSO) - anchorX) < 1e-9)
  }
  assert.ok(sleeveCarry(-0.3, 0.45) > sleeveCarry(-0.8, 0.45))
})

test('the compiled follow and the audition are one value, not two', () => {
  assert.equal(anime25DTorsoYawFollow({ yawFollowScale: 0.5 }, 0.5), 0.25)
  assert.equal(anime25DTorsoYawFollow({}, 1), 1)
  assert.equal(anime25DTorsoYawFollow({}, 0.4), 0.4)
  // The site never writes bodyYaw, so the compiled value governs alone.
  assert.equal(anime25DTorsoYawFollow({ yawFollowScale: 0.6 }, 1), 0.6)
  for (const authored of [Number.NaN, undefined, 4, -1]) {
    for (const bodyYaw of [Number.NaN, Number.POSITIVE_INFINITY, 4, -1]) {
      const follow = anime25DTorsoYawFollow({ yawFollowScale: authored }, bodyYaw)
      assert.ok(follow >= 0 && follow <= 1, `${authored} ${bodyYaw}: ${follow}`)
    }
  }
})
