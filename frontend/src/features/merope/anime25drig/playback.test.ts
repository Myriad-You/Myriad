import type { Anime25DPlaybackBuildLayer } from './playback'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { analyzeAnime25DMouthProfile } from './mouthProfile'
import { buildAnime25DPlayback, remapRiggerAnchors } from './playback'
import { isAnime25DPlayback } from './types'

function layer(
  partial: Omit<
    Anime25DPlaybackBuildLayer,
    'textureBounds' | 'strands' | 'group'
  > &
    Partial<
      Pick<Anime25DPlaybackBuildLayer, 'textureBounds' | 'strands' | 'group'>
    >,
): Anime25DPlaybackBuildLayer {
  return {
    group: 'head',
    textureBounds: { x: 0, y: 0, width: 0.2, height: 0.2 },
    strands: [],
    ...partial,
  }
}

function anchorsFor(width: number, height: number) {
  return {
    face: { x0: 230, y0: 92, x1: 538, y1: 368, cx: 384, cy: 246 },
    neckPivot: { x: 384, y: 390 },
    neckTop: 368,
    neckBottom: 428,
    bodyPivot: { x: 384, y: height },
    mouth: { x0: 350, y0: 300, x1: 418, y1: 330, cx: 384, cy: 315 },
    faceScale: 308 / 333,
    eyeL: {
      x0: 260,
      y0: 140,
      x1: 320,
      y1: 180,
      icx: 290,
      icy: 160,
      closeY: 168,
    },
    eyeR: {
      x0: 448,
      y0: 140,
      x1: 508,
      y1: 180,
      icx: 478,
      icy: 160,
      closeY: 168,
    },
  }
}

function mouthProfileFor(width: number, height: number) {
  const anchors = anchorsFor(width, height)
  return analyzeAnime25DMouthProfile(
    [],
    { x: 0, y: 0, width, height },
    anchors.mouth,
  )
}

describe('Anime2.5DRig playback', () => {
  it('builds a credited playback document from face-rig layers', () => {
    const playback = buildAnime25DPlayback({
      frameWidth: 768,
      frameHeight: 1024,
      anchors: anchorsFor(768, 1024),
      mouthProfile: mouthProfileFor(768, 1024),
      layers: [
        layer({
          id: 'face',
          role: 'face',
          side: null,
          bounds: { x: 0.3, y: 0.12, width: 0.4, height: 0.36 },
        }),
        layer({
          id: 'front-hair',
          role: 'front-hair',
          side: null,
          bounds: { x: 0.28, y: 0.08, width: 0.44, height: 0.3 },
          textureBounds: { x: 0.2, y: 0, width: 0.2, height: 0.2 },
          strands: [{ x: 0.4, rootY: 0.1, tipY: 0.28 }],
        }),
        layer({
          id: 'topwear',
          role: 'topwear',
          side: null,
          group: 'body',
          bounds: { x: 0.18, y: 0.36, width: 0.64, height: 0.64 },
          textureBounds: { x: 0.4, y: 0, width: 0.2, height: 0.2 },
        }),
      ],
    })
    assert.equal(isAnime25DPlayback(playback), true)
    assert.equal(playback.engine, 'Anime2.5DRig')
    assert.equal(playback.engineUrl, 'https://github.com/852wa/Anime2.5DRig')
    assert.equal(playback.license, 'MIT')
    assert.equal(playback.layers[1]?.phys, 'hair')
    assert.equal(playback.layers[1]?.strands.length, 1)
    assert.equal(playback.layers[0]?.z, 0)
    assert.equal(playback.layers[1]?.z, 1)
    assert.equal(playback.anchors.eyeL?.closeY, 168)
    assert.equal(playback.anchors.bodyPivot.y, 1024)
    assert.ok(playback.anchors.faceScale > 0)
    assert.equal(playback.version, 7)
    assert.equal(playback.shellProfile.version, 1)
    assert.equal(playback.shellProfile.source, 'anchor-derived')
    assert.equal(playback.shellProfile.hair.hairlinePin.enabled, true)
    assert.equal(playback.shellProfile.torso.enabled, true)
    assert.equal(playback.shellProfile.torso.centerX, 384)
    assert.equal(playback.shellProfile.torso.radiusX, 308 * 0.95)
    assert.equal(playback.shellProfile.torso.radiusZ, 308 * 0.55)
    assert.equal(playback.chestProfile.source, 'geometry-fallback')
  })

  it('keeps independent front and rear hair layers', () => {
    const playback = buildAnime25DPlayback({
      frameWidth: 768,
      frameHeight: 1024,
      anchors: anchorsFor(768, 1024),
      mouthProfile: mouthProfileFor(768, 1024),
      layers: [
        layer({
          id: 'back-hair',
          role: 'back-hair',
          side: null,
          group: 'head',
          bounds: { x: 0.2, y: 0.06, width: 0.6, height: 0.5 },
          textureBounds: { x: 0, y: 0.2, width: 0.2, height: 0.2 },
          strands: [
            { x: 0.3, rootY: 0.08, tipY: 0.5 },
            { x: 0.5, rootY: 0.08, tipY: 0.48 },
            { x: 0.7, rootY: 0.08, tipY: 0.52 },
          ],
        }),
        layer({
          id: 'face',
          role: 'face',
          side: null,
          bounds: { x: 0.3, y: 0.12, width: 0.4, height: 0.36 },
        }),
        layer({
          id: 'front-hair',
          role: 'front-hair',
          side: null,
          bounds: { x: 0.28, y: 0.08, width: 0.44, height: 0.3 },
          textureBounds: { x: 0.2, y: 0, width: 0.2, height: 0.2 },
          strands: [
            { x: 0.35, rootY: 0.1, tipY: 0.26 },
            { x: 0.5, rootY: 0.09, tipY: 0.24 },
            { x: 0.65, rootY: 0.1, tipY: 0.27 },
          ],
        }),
      ],
    })
    const front = playback.layers.find((item) => item.role === 'front-hair')
    const back = playback.layers.find((item) => item.role === 'back-hair')
    assert.equal(front?.phys, 'hair')
    assert.equal(back?.phys, 'hair')
    assert.equal(front?.group, 'head')
    assert.equal(back?.group, 'head')
    assert.equal(front?.strands.length, 3)
    assert.equal(back?.strands.length, 3)
    assert.ok((front?.depth ?? 0) > (back?.depth ?? 1))
    assert.equal(back?.z, 0)
    assert.equal(front?.z, 2)
  })

  it('maps a dedicated per-eye dizzy layer without treating it as a blink', () => {
    const playback = buildAnime25DPlayback({
      frameWidth: 768,
      frameHeight: 1024,
      anchors: anchorsFor(768, 1024),
      mouthProfile: mouthProfileFor(768, 1024),
      layers: [
        layer({
          id: 'face',
          role: 'face',
          side: null,
          bounds: { x: 0.3, y: 0.12, width: 0.4, height: 0.36 },
        }),
        layer({
          id: 'eye-dizzy-left',
          role: 'eye-dizzy',
          side: 'left',
          bounds: { x: 0.36, y: 0.16, width: 0.06, height: 0.06 },
        }),
      ],
    })
    const dizzy = playback.layers.find((item) => item.role === 'eye-dizzy')
    assert.equal(dizzy?.side, 'L')
    assert.equal(dizzy?.fade, 'eyeDizzy')
  })

  it('maps a dedicated per-eye squeeze layer without treating it as a blink', () => {
    const playback = buildAnime25DPlayback({
      frameWidth: 768,
      frameHeight: 1024,
      anchors: anchorsFor(768, 1024),
      mouthProfile: mouthProfileFor(768, 1024),
      layers: [
        layer({
          id: 'face',
          role: 'face',
          side: null,
          bounds: { x: 0.3, y: 0.12, width: 0.4, height: 0.36 },
        }),
        layer({
          id: 'eye-squeeze-left',
          role: 'eye-squeeze',
          side: 'left',
          bounds: { x: 0.36, y: 0.16, width: 0.06, height: 0.04 },
        }),
      ],
    })
    const squeeze = playback.layers.find((item) => item.role === 'eye-squeeze')
    assert.equal(squeeze?.side, 'L')
    assert.equal(squeeze?.fade, 'eyeSqueeze')
  })

  it('maps a complete per-eye crying replacement independently', () => {
    const playback = buildAnime25DPlayback({
      frameWidth: 768,
      frameHeight: 1024,
      anchors: anchorsFor(768, 1024),
      mouthProfile: mouthProfileFor(768, 1024),
      layers: [
        layer({
          id: 'face',
          role: 'face',
          side: null,
          bounds: { x: 0.3, y: 0.12, width: 0.4, height: 0.36 },
        }),
        layer({
          id: 'eye-cry-left',
          role: 'eye-cry',
          side: 'left',
          bounds: { x: 0.36, y: 0.16, width: 0.07, height: 0.12 },
        }),
      ],
    })
    const cry = playback.layers.find((item) => item.role === 'eye-cry')
    assert.equal(cry?.side, 'L')
    assert.equal(cry?.fade, 'eyeCry')
  })

  it('remaps rigger anchors into the 3:4 frame without rebuilding them', () => {
    const remapped = remapRiggerAnchors(
      {
        face: { cx: 128, cy: 80, x0: 60, x1: 196, y0: 30, y1: 140 },
        eyeL: {
          x0: 80,
          y0: 70,
          x1: 110,
          y1: 88,
          icx: 95,
          icy: 79,
          closeY: 78,
        },
        mouth: { x0: 108, x1: 148, y0: 112, y1: 130, cx: 128, cy: 121 },
        neckPivot: { cx: 130, cy: 160 },
        neckTop: 140,
        neckBottom: 180,
        bodyPivot: { cx: 130, cy: 256 },
        faceScale: 136 / 333,
      },
      { x: 16, y: -20, width: 240, height: 320 },
    )
    assert.equal(remapped.face.cx, 112)
    assert.equal(remapped.face.y0, 50)
    assert.equal(remapped.eyeL?.closeY, 98)
    assert.equal(remapped.mouth.cy, 141)
    assert.equal(remapped.neckPivot.x, 114)
    assert.equal(remapped.neckPivot.y, 180)
    assert.deepEqual(remapped.bodyPivot, { x: 114, y: 320 })
    assert.equal(remapped.faceScale, 136 / 333)
  })
})
