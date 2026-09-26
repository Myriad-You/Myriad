import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import {
  ANIME25D_MOTION_ENVELOPE_PROBES,
  deriveAnime25DMotionEnvelopeProfile,
  projectAnime25DMotionEnvelope,
} from './motionEnvelope'

const fittedProfile = deriveAnime25DMotionEnvelopeProfile({ layers: [] })

test('high collar transfers unsafe pitch instead of globally shrinking the pose', () => {
  const driver = {
    ...IDENTITY_DRIVER,
    angleY: 1,
    angleZ: -0.2,
    body: 0.2,
  }
  const result = projectAnime25DMotionEnvelope(
    driver,
    deriveAnime25DMotionEnvelopeProfile({
      layers: [{ role: 'collar-front' }, { role: 'handwear' }],
    }),
    { clippedEnergy: 0, transferredEnergy: 0 },
  )
  assert.equal(driver.angleY, 0.8)
  assert.ok(driver.angleZ < -0.2)
  assert.ok(driver.armY > 0)
  assert.ok(result.clippedEnergy > 0)
  assert.equal(result.clippedEnergy, result.transferredEnergy)
})

test('ordinary poses pass through the joint envelope unchanged', () => {
  const driver = { ...IDENTITY_DRIVER, angleX: 0.2, angleY: 0.3, body: 0.4 }
  projectAnime25DMotionEnvelope(
    driver,
    deriveAnime25DMotionEnvelopeProfile({
      layers: [{ role: 'collar-front' }],
    }),
    { clippedEnergy: 0, transferredEnergy: 0 },
  )
  assert.equal(driver.angleX, 0.2)
  assert.equal(driver.angleY, 0.3)
  assert.equal(driver.body, 0.4)
})

test('full input reaches the fitted asset range instead of asymptotically shrinking', () => {
  const driver = { ...IDENTITY_DRIVER, angleY: 1, body: -1 }
  const result = projectAnime25DMotionEnvelope(driver, fittedProfile, {
    clippedEnergy: 0,
    transferredEnergy: 0,
  })
  assert.equal(driver.angleY, 1)
  assert.equal(driver.body, -1)
  assert.equal(result.transferredEnergy, 0)
})

test('outfit calibration limits torso and rigid sleeve motion and redistributes it', () => {
  const profile = deriveAnime25DMotionEnvelopeProfile(
    { layers: [{ role: 'handwear' }] },
    {
      outfitProfile: {
        topologies: ['armor'],
        secondaryPartIds: [],
        torsoTwistScale: 0.62,
        secondaryMotionScale: 0.45,
      },
    },
  )
  assert.equal(profile.torso.limit, 0.62)
  assert.equal(profile.rigidArm.limit, 0.45)

  const driver = {
    ...IDENTITY_DRIVER,
    angleX: -0.1,
    body: 1,
    armY: 1,
    armPos: -1,
  }
  const result = projectAnime25DMotionEnvelope(driver, profile, {
    clippedEnergy: 0,
    transferredEnergy: 0,
  })
  assert.ok(Math.abs(driver.body) <= profile.torso.limit)
  assert.ok(driver.body > profile.torso.startsAt)
  assert.equal(driver.armY, profile.rigidArm.limit)
  assert.equal(driver.armPos, -profile.rigidArm.limit)
  assert.notEqual(driver.angleX, -0.1)
  assert.ok(result.transferredEnergy > 0)
  assert.equal(result.clippedEnergy, result.transferredEnergy)
})

test('assets without rigid sleeves do not advertise an arm envelope', () => {
  const profile = deriveAnime25DMotionEnvelopeProfile({
    layers: [{ role: 'collar-front' }],
  })
  assert.equal(profile.armMotion, false)
  assert.deepEqual(profile.rigidArm, { startsAt: 0, limit: 0 })
  const driver = { ...IDENTITY_DRIVER, angleY: 1 }
  projectAnime25DMotionEnvelope(driver, profile, {
    clippedEnergy: 0,
    transferredEnergy: 0,
  })
  assert.equal(driver.armY, 0)
  assert.equal(driver.armPos, 0)
})

test('arms whose hands hold each other take no arm gesture; the body carries it', () => {
  const profile = deriveAnime25DMotionEnvelopeProfile({ layers: [{ role: 'handwear' }] }, undefined, true)
  assert.equal(profile.armMotion, true)
  assert.equal(profile.rigidArm.limit, 0)
  const driver = { ...IDENTITY_DRIVER, armY: 0.8, armPos: -0.6 }
  const result = projectAnime25DMotionEnvelope(driver, profile, { clippedEnergy: 0, transferredEnergy: 0 })
  assert.equal(driver.armY, 0)
  assert.equal(driver.armPos, 0)
  assert.notEqual(driver.body, 0)
  assert.ok(result.transferredEnergy > 1.3)
})

test('all reproducible boundary probes stay inside a restrictive asset envelope', () => {
  const profile = deriveAnime25DMotionEnvelopeProfile(
    {
      layers: [{ role: 'collar-back' }, { role: 'handwear' }],
    },
    {
      outfitProfile: {
        topologies: ['wide-sleeve', 'armor'],
        secondaryPartIds: [],
        torsoTwistScale: 0.62,
        secondaryMotionScale: 0.45,
      },
    },
  )
  for (const probe of ANIME25D_MOTION_ENVELOPE_PROBES) {
    const driver = { ...IDENTITY_DRIVER, ...probe.driver }
    const result = projectAnime25DMotionEnvelope(driver, profile, {
      clippedEnergy: 0,
      transferredEnergy: 0,
    })
    assert.ok(Math.abs(driver.angleY) <= profile.pitch.limit, probe.id)
    assert.ok(Math.abs(driver.body) <= profile.torso.limit, probe.id)
    assert.ok(Math.abs(driver.armY) <= profile.rigidArm.limit, probe.id)
    assert.ok(Math.abs(driver.armPos) <= profile.rigidArm.limit, probe.id)
    for (const value of [
      driver.angleX,
      driver.angleY,
      driver.angleZ,
      driver.body,
      driver.armY,
      driver.armPos,
      driver.eyeX,
      driver.eyeY,
      result.clippedEnergy,
      result.transferredEnergy,
    ]) {
      assert.equal(Number.isFinite(value), true, probe.id)
    }
  }
})

test('clipped energy changes sides continuously instead of snapping across', () => {
  const profile = deriveAnime25DMotionEnvelopeProfile({
    layers: [{ role: 'collar-front' }, { role: 'collar-back' }],
  })
  const result = { clippedEnergy: 0, transferredEnergy: 0 }
  const step = 0.002
  let previous: number | null = null
  let worst = 0
  for (let index = -30; index <= 30; index += 1) {
    const target = {
      ...IDENTITY_DRIVER,
      angleY: 0.95,
      angleZ: index * step,
      angleX: 0,
    }
    projectAnime25DMotionEnvelope(target, profile, result)
    if (previous !== null) {
      worst = Math.max(worst, Math.abs(target.angleX - previous))
    }
    previous = target.angleX
  }
  assert.ok(worst <= step, `yaw moved ${worst} for a roll step of ${step}`)
})
