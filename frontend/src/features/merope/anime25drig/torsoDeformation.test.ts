import type { Anime25DChestProfile, Anime25DTorsoShellProfile } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  anime25DLayerUsesTorsoShell,
  anime25DTorsoShellModeForLayer,
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
  for (const source of [
    { group: 'body', role: 'neck' },
    { group: 'body', role: 'handwear' },
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
