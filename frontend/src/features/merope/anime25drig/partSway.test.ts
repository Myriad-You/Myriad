import type { Anime25DPart, Anime25DPartProfile } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { createFigurePose } from './figureSkeleton'
import { applyPartSways, bindPartSways, PartSways, swayShare } from './partSway'

function part(patch: Partial<Anime25DPart>): Anime25DPart {
  return {
    name: 'cardigan front',
    layer: 'topwear',
    separate: false,
    box: [100, 500, 300, 900],
    anchor: [200, 500],
    bone: 'chest',
    motion: 'sway',
    material: 'knit',
    stiffness: 0.4,
    weight: 0.5,
    ...patch,
  }
}

const profile: Anime25DPartProfile = {
  version: 1,
  source: 'ai-vision',
  parts: [
    part({}),
    part({ name: 'earring', layer: 'recovered-earwear', separate: true, motion: 'dangle' }),
    part({ name: 'button', motion: 'rigid' }),
  ],
}

test('only cloth hanging free in a shared layer sways there', () => {
  const sways = bindPartSways(profile, 'topwear')!
  assert.equal(sways.length, 1)
  assert.equal(sways[0].spring, 0)
  assert.equal(bindPartSways(profile, 'recovered-earwear'), null, 'a part alone in its layer swings as a whole')
  assert.equal(bindPartSways(undefined, 'topwear'), null)
})

test('the region takes none of the lag at its anchor and all of it at the far end', () => {
  const [sway] = bindPartSways(profile, 'topwear')!
  assert.equal(swayShare(sway, 200, 500), 0)
  assert.equal(swayShare(sway, 200, 900), 1)
  assert.ok(swayShare(sway, 200, 700) > 0 && swayShare(sway, 200, 700) < 0.5)
  assert.equal(swayShare(sway, 50, 900), 0, 'outside its sides')
  assert.equal(swayShare(sway, 200, 400), 0, 'above its anchor')
})

test('moved by its bone, the region trails it, then settles', () => {
  const sways = new PartSways(profile)
  const pose = createFigurePose()
  sways.step(1 / 60, pose, true)
  assert.equal(sways.lags[0], 0)
  pose.chest.tx = 30
  sways.step(1 / 60, pose, true)
  assert.ok(sways.lags[0] < -5, `it trails: ${sways.lags[0]}`)
  assert.ok(sways.lags[0] >= -0.02 * 400, `a lurch drags it only so far off: ${sways.lags[0]}`)
  for (let i = 0; i < 600; i++) sways.step(1 / 60, pose, true)
  assert.ok(Math.abs(sways.lags[0]) < 0.5, `it settles: ${sways.lags[0]}`)
  const point = { x: 200, y: 900 }
  pose.chest.tx = 0
  sways.step(1 / 60, pose, true)
  applyPartSways(point, 200, 900, bindPartSways(profile, 'topwear')!, sways.lags)
  assert.ok(point.x > 200, 'the hem is left behind as the chest moves back')
  sways.step(1 / 60, pose, false)
  assert.equal(sways.lags[0], 0, 'without physics it follows at once')
})
