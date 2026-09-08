import type { Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { ClosedEyePresentation } from './closedEyePresentation'
import { IDENTITY_DRIVER } from './driver'
import { stepAnime25DBlink } from './driverComposition'
import {
  deformAnime25DUpstreamFeaturePoint,
  resolveAnime25DUpstreamFeature,
} from './layerDeformation'
import { fadeOpacity } from './mouthRuntime'

function layer(role: string, side: 'L' | 'R'): Anime25DPlaybackLayer {
  return { role, side, fade: 'eyeClose' } as Anime25DPlaybackLayer
}

test('automatic blinks keep ordinary art; deliberate closure selects alternate per side', () => {
  const selection = new ClosedEyePresentation()
  selection.bind([layer('eye-close2', 'L')])
  const blink = { activeSeconds: 0, nextAtSeconds: 99 }
  for (let i = 0; i < 120; i++) {
    const target = { ...IDENTITY_DRIVER }
    selection.step(target, 1 / 60)
    stepAnime25DBlink(target, blink, i / 60, 1 / 60, true, false)
    assert.equal(selection.opacity(layer('eye-close2', 'L')), 0)
    assert.equal(selection.opacity(layer('eye-close', 'L')), 1)
  }
  let previous = 0
  for (let i = 0; i < 60; i++) {
    selection.step({ eyeOpenL: 0, eyeOpenR: 0 }, 1 / 60)
    const alternate = selection.opacity(layer('eye-close2', 'L'))
    assert.ok(alternate >= previous && alternate - previous < 0.27)
    assert.equal(alternate + selection.opacity(layer('eye-close', 'L')), 1)
    assert.equal(selection.opacity(layer('eye-close', 'R')), 1)
    previous = alternate
  }
  assert.equal(previous, 1)
  for (const special of [
    'eyeCry',
    'eyeDizzy',
    'eyeSqueeze',
    'silly',
  ] as const) {
    const driver = { ...IDENTITY_DRIVER, eyeOpenL: 0, [special]: 1 }
    for (const role of ['eye-close', 'eye-close2']) {
      const source = layer(role, 'L')
      assert.equal(fadeOpacity(source, driver) * selection.opacity(source), 0)
    }
  }
  for (let i = 0; i < 60; i++) selection.step(IDENTITY_DRIVER, 1 / 60)
  assert.equal(selection.opacity(layer('eye-close2', 'L')), 0)
  selection.step({ eyeOpenL: 0, eyeOpenR: 0 }, 1)
  selection.bind([])
  assert.equal(selection.opacity(layer('eye-close', 'L')), 1)
})

test('alternate closed art uses exactly the ordinary scale, angle and eye-follow deformation', () => {
  for (const role of ['eye-close', 'eye-close2']) {
    assert.equal(
      resolveAnime25DUpstreamFeature({ role, fade: 'eyeClose' }, true),
      'eye-close',
    )
  }
  const input = {
    kind: 'eye-close' as const,
    side: 'L' as const,
    eye: { x0: 0, x1: 20, y0: 0, y1: 20, icx: 10, icy: 10, closeY: 10 },
    centerX: 10,
    centerY: 10,
    faceScale: 1,
    expression: {
      ...IDENTITY_DRIVER,
      eyeScaleL: 1.4,
      eyeCY: 0.2,
      eyeCAng: 0.3,
    },
  }
  const point = { x: 15, y: 11 }
  deformAnime25DUpstreamFeaturePoint(point, input)
  assert.notDeepEqual(point, { x: 15, y: 11 })
  assert.ok(Number.isFinite(point.x) && Number.isFinite(point.y))
})

test('unbound artwork cannot take opacity from an ordinary closed eye', () => {
  const selection = new ClosedEyePresentation()
  selection.bind([{ ...layer('eye-close2', 'L'), fade: null }])
  for (let i = 0; i < 60; i++)
    selection.step({ eyeOpenL: 0, eyeOpenR: 0 }, 1 / 60)
  assert.equal(selection.opacity(layer('eye-close', 'L')), 1)
})
