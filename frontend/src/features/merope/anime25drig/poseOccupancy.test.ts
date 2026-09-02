import assert from 'node:assert/strict'
import test from 'node:test'
import { composeOccupancyOffsets } from './poseCompositor'
import {
  COSPEECH_SPEAKING,
  GLANCE_SINGING,
  GLANCE_SPEAKING,
  GROOVE_SINGING,
  occupancyTargets,
  PoseOccupancyController,
  RANDOM_SPEAKING,
} from './poseOccupancy'

const idle = {
  speaking: false,
  singing: false,
  thinking: false,
  pointerDriven: false,
  automation: true,
  sticker: 0,
}

test('idle puts glance and random high and leaves groove and co-speech off', () => {
  const occupancy = occupancyTargets(idle)
  assert.equal(occupancy.glance, 1)
  assert.equal(occupancy.random, 1)
  assert.equal(occupancy.coSpeech, 0)
  assert.equal(occupancy.groove, 0)
  assert.equal(occupancy.speechMouth, 0)
  assert.equal(occupancy.grooveMouth, 0)
})

test('speaking keeps glance and co-speech alive and does not zero random', () => {
  const occupancy = occupancyTargets({ ...idle, speaking: true })
  assert.equal(occupancy.glance, GLANCE_SPEAKING)
  assert.ok(occupancy.glance > 0)
  assert.equal(occupancy.random, RANDOM_SPEAKING)
  assert.ok(occupancy.random > 0)
  assert.equal(occupancy.coSpeech, COSPEECH_SPEAKING)
  assert.ok(occupancy.coSpeech > 0)
  assert.equal(occupancy.speechMouth, 1)
  assert.equal(occupancy.groove, 0)
})

test('singing keeps glance occupancy well above the old headKeep floor', () => {
  const occupancy = occupancyTargets({ ...idle, singing: true })
  assert.equal(occupancy.glance, GLANCE_SINGING)
  assert.ok(occupancy.glance > 0.12)
  assert.equal(occupancy.groove, GROOVE_SINGING)
  assert.equal(occupancy.grooveMouth, 1)
  assert.equal(occupancy.speechMouth, 0)
})

test('speaking while singing gives the mouth to speech and keeps groove on the body', () => {
  const occupancy = occupancyTargets({ ...idle, speaking: true, singing: true })
  assert.equal(occupancy.speechMouth, 1)
  assert.equal(occupancy.grooveMouth, 0)
  assert.equal(occupancy.groove, GROOVE_SINGING)
  assert.equal(occupancy.glance, GLANCE_SPEAKING)
  assert.equal(occupancy.coSpeech, COSPEECH_SPEAKING)
})

test('stickers compress living sources instead of switching them off', () => {
  const occupancy = occupancyTargets({ ...idle, speaking: true, sticker: 1 })
  assert.ok(occupancy.glance > 0)
  assert.ok(occupancy.glance < GLANCE_SPEAKING)
  assert.ok(occupancy.random > 0)
  assert.ok(occupancy.coSpeech > 0)
  assert.ok(occupancy.coSpeech < COSPEECH_SPEAKING)
})

test('occupancy eases when the situation flips and does not step the weights', () => {
  const occupancy = new PoseOccupancyController()
  occupancy.sample(0, idle)
  const started = occupancy.sample(1 / 60, { ...idle, speaking: true })
  assert.ok(started.glance > GLANCE_SPEAKING)
  assert.ok(started.glance < 1)
  assert.ok(started.coSpeech > 0)
  assert.ok(started.coSpeech < COSPEECH_SPEAKING)
  let previous = { ...started }
  let largestGlanceStep = 0
  for (let frame = 2; frame <= 60; frame += 1) {
    const current = occupancy.sample(1 / 60, { ...idle, speaking: true })
    largestGlanceStep = Math.max(
      largestGlanceStep,
      Math.abs(current.glance - previous.glance),
    )
    previous = { ...current }
  }
  assert.ok(largestGlanceStep < 0.12)
  assert.ok(Math.abs(previous.glance - GLANCE_SPEAKING) < 0.02)
})

test('a new semantic source becomes useful within the first eighty milliseconds', () => {
  const occupancy = new PoseOccupancyController()
  occupancy.sample(0, idle)
  let current = occupancy.sample(1 / 60, { ...idle, speaking: true })
  for (let frame = 2; frame <= 5; frame += 1) {
    current = occupancy.sample(1 / 60, { ...idle, speaking: true })
  }
  assert.ok(current.coSpeech > COSPEECH_SPEAKING * 0.7)
  assert.ok(current.speechMouth > 0.7)
})

test('speech end lets idle glance rise without a 2400ms baseline hold', () => {
  const occupancy = new PoseOccupancyController()
  occupancy.sample(0, { ...idle, speaking: true })
  let current = occupancy.sample(0, { ...idle, speaking: false })
  for (let frame = 1; frame <= 36; frame += 1) {
    current = occupancy.sample(1 / 60, idle)
  }
  assert.ok(current.glance > GLANCE_SPEAKING)
  assert.ok(Math.abs(current.glance - 1) < 0.08)
  assert.ok(36 / 60 < 2.4)
})

test('occupancy does not consult a motion-policy owner to return glance', () => {
  const occupancy = occupancyTargets(idle)
  assert.equal(occupancy.glance, 1)
  assert.equal(occupancy.random, 1)
  const singing = occupancyTargets({ ...idle, singing: true })
  assert.ok(singing.glance > 0.12)
  assert.equal(singing.groove, GROOVE_SINGING)
})

test('compositor keeps singing groove and idle glance visible together', () => {
  const everywhere = (amount: number) => ({
    gaze: amount,
    headBody: amount,
    expression: amount,
  })
  const mixed = composeOccupancyOffsets([
    {
      weights: everywhere(GLANCE_SINGING),
      offset: { angleX: 0.3, eyeX: 0.4 },
    },
    {
      weights: everywhere(GROOVE_SINGING),
      offset: { angleY: 0.3, body: 0.2, armY: 0.25, brow: 0.1 },
    },
  ])
  assert.ok(mixed.angleX > 0.1)
  assert.ok(mixed.eyeX > 0.1)
  assert.ok(mixed.angleY > 0.1)
  assert.ok(mixed.armY > 0.1)
})

test('a layer that lost a channel keeps the ones it still owns', () => {
  const composed = composeOccupancyOffsets([
    {
      // Music owns the body but not the eyes.
      weights: { gaze: 0, headBody: 1, expression: 1 },
      offset: { angleY: 0.4, eyeX: 0.5, brow: 0.2 },
    },
  ])
  assert.equal(composed.eyeX, 0)
  assert.ok(composed.angleY > 0.3)
  assert.ok(composed.brow > 0.15)
})
