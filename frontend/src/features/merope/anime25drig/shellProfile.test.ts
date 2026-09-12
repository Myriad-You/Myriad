import type { Anime25DPlayback, Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  anime25DHairlinePinWeight,
  deformAnime25DShellPoint,
  sampleAnime25DHairlinePinWeights,
  writeAnime25DShellRotation,
} from './shellDeformation'
import { deriveAnime25DShellProfile } from './shellProfile'

const faceLayer: Anime25DPlaybackLayer = {
  name: 'face',
  role: 'face',
  z: 1,
  depth: 1,
  group: 'head',
  phys: null,
  fade: null,
  side: null,
  x: 210,
  y: 80,
  w: 348,
  h: 390,
  atlas: { x: 0, y: 0, w: 0.5, h: 0.5 },
  strands: [],
}

const frontHairLayer: Anime25DPlaybackLayer = {
  ...faceLayer,
  name: 'front-hair',
  role: 'front-hair',
  z: 2,
  depth: 1.28,
  phys: 'hair',
  x: 150,
  y: 25,
  w: 468,
  h: 420,
}

const topwearLayer: Anime25DPlaybackLayer = {
  ...faceLayer,
  name: 'topwear',
  role: 'topwear',
  z: 0,
  depth: 0.82,
  group: 'body',
  x: 96,
  y: 392,
  w: 576,
  h: 632,
}

const playbackSource = {
  anchors: {
    face: { x0: 230, y0: 92, x1: 538, y1: 405, cx: 384, cy: 248 },
    eyeL: {
      x0: 270,
      y0: 170,
      x1: 330,
      y1: 215,
      icx: 300,
      icy: 194,
      closeY: 201,
    },
    eyeR: {
      x0: 438,
      y0: 170,
      x1: 498,
      y1: 215,
      icx: 468,
      icy: 194,
      closeY: 201,
    },
    mouth: { x0: 347, y0: 310, x1: 421, y1: 344, cx: 384, cy: 327 },
    neckPivot: { x: 384, y: 426 },
    neckTop: 395,
    neckBottom: 482,
    bodyPivot: { x: 384, y: 1024 },
    faceScale: 0.925,
  },
  layers: [faceLayer, frontHairLayer, topwearLayer],
} satisfies Pick<Anime25DPlayback, 'anchors' | 'layers'>

test('derives a complete deterministic shell profile at import time', () => {
  const first = deriveAnime25DShellProfile(playbackSource)
  const second = deriveAnime25DShellProfile(playbackSource)
  assert.deepEqual(first, second)
  assert.equal(first.version, 1)
  assert.equal(first.source, 'anchor-derived')
  assert.equal(first.hair.hairlinePin.enabled, true)
  assert.equal(first.hair.hairlinePin.mode, 'strand-roots')
  assert.deepEqual(first.torso, {
    enabled: true,
    blend: 0.5,
    centerX: 384,
    radiusX: 308 * 0.95,
    radiusZ: 308 * 0.55,
    yawFollowScale: 1,
  })
})

test('shell projection is an exact neutral identity and finite at bounded turns', () => {
  const profile = deriveAnime25DShellProfile(playbackSource)
  const rotation = {
    active: false,
    yawCosine: 1,
    yawSine: 0,
    pitchCosine: 1,
    pitchSine: 0,
  }
  const neutral = { x: 417.25, y: 246.5 }
  writeAnime25DShellRotation(0, 0, rotation)
  deformAnime25DShellPoint(
    neutral,
    neutral.y,
    'head',
    profile,
    rotation,
    1.08,
  )
  assert.deepEqual(neutral, { x: 417.25, y: 246.5 })

  const turned = { ...neutral }
  writeAnime25DShellRotation(1, -0.8, rotation)
  deformAnime25DShellPoint(turned, turned.y, 'head', profile, rotation, 1.08)
  assert.equal(Number.isFinite(turned.x), true)
  assert.equal(Number.isFinite(turned.y), true)
  assert.notDeepEqual(turned, neutral)
})

test('a locally deformed eyelid samples depth at its current face position, not its original row', () => {
  const profile = deriveAnime25DShellProfile(playbackSource)
  const rotation = { active: false, yawCosine: 1, yawSine: 0, pitchCosine: 1, pitchSine: 0 }
  for (const yaw of [-1, -0.5, 0.5, 1]) {
    for (const pitch of [-1, -0.5, 0.5, 1]) {
      writeAnime25DShellRotation(yaw, pitch, rotation)
      const target = { x: 325, y: playbackSource.anchors.eyeL.closeY }
      const reference = { ...target }
      deformAnime25DShellPoint(reference, target.y, 'head', profile, rotation, 1.1)
      for (const originalY of [170, 194, 215]) {
        const moved = { ...target }
        deformAnime25DShellPoint(moved, originalY, 'head', profile, rotation, 1.1)
        assert.deepEqual(moved, reference, 'a shared eyelid location must not inherit a different surface because it came from another row')
      }
    }
  }
})

test('face depth stays slope-continuous across irregular landmark intervals and constant ends', () => {
  const profile = deriveAnime25DShellProfile(playbackSource)
  const rotation = { active: false, yawCosine: 1, yawSine: 0, pitchCosine: 1, pitchSine: 0 }
  const { startY, endY, points } = profile.faceProfile
  const epsilon = 0.0001
  for (const depths of [points.map(p => p.z), [0.02, 0.1, 0.24, 0.28, 0.4], [0.4, 0.28, 0.24, 0.1, 0.02]]) {
    points.forEach((p, i) => { p.z = depths[i] })
    for (const yaw of [-1, 1]) {
      for (const pitch of [-1, 1]) {
        writeAnime25DShellRotation(yaw, pitch, rotation)
        for (const landmark of points) {
          const y = startY + landmark.v * (endY - startY)
          const sample = (at: number) => {
            const point = { x: profile.head.centerX, y: at }
            deformAnime25DShellPoint(point, at, 'head', profile, rotation, 1)
            return point
          }
          const before = sample(y - epsilon)
          const at = sample(y)
          const after = sample(y + epsilon)
          const slopeJump = Math.hypot(after.x - 2 * at.x + before.x, after.y - 2 * at.y + before.y) / epsilon
          assert.ok(slopeJump < 0.0001, `${yaw}/${pitch} landmark ${landmark.v}: slope jump ${slopeJump}`)
        }
      }
    }
  }
})

test('face interpolation cannot invent a depth bulge beyond adjacent authored landmarks', () => {
  const profile = deriveAnime25DShellProfile(playbackSource)
  const rotation = { active: false, yawCosine: 1, yawSine: 0, pitchCosine: 1, pitchSine: 0 }
  writeAnime25DShellRotation(1, 0, rotation)
  const { startY, endY, points } = profile.faceProfile
  const sample = (v: number, depth?: number) => {
    const point = { x: profile.head.centerX, y: startY + v * (endY - startY) }
    const selected = depth === undefined ? profile : { ...profile, faceProfile: { ...profile.faceProfile, points: points.map(p => ({ ...p, z: depth })) } }
    deformAnime25DShellPoint(point, point.y, 'head', selected, rotation, 1)
    return point.x
  }
  for (let i = 0; i < points.length - 1; i++) {
    for (let n = 0; n <= 40; n++) {
      const v = points[i].v + (points[i + 1].v - points[i].v) * n / 40
      const a = sample(v, points[i].z)
      const b = sample(v, points[i + 1].z)
      const actual = sample(v)
      assert.ok(actual >= Math.min(a, b) - 1e-8 && actual <= Math.max(a, b) + 1e-8, `interval ${i} at ${v}`)
    }
  }
})

test('hairline pin stays full inside its scalp rectangle and fades outside', () => {
  const profile = deriveAnime25DShellProfile(playbackSource)
  const pin = profile.hair.hairlinePin
  const centerX = profile.head.centerX + pin.centerX * profile.head.radiusX
  const centerY = profile.head.centerY + pin.centerY * profile.head.radiusY
  assert.equal(anime25DHairlinePinWeight(centerX, centerY, profile), 1)
  assert.equal(
    anime25DHairlinePinWeight(
      centerX + profile.head.radiusX * (pin.halfWidth + pin.feather * 2),
      centerY,
      profile,
    ),
    0,
  )
})

test('crown correction is continuous through neutral instead of switching on at the first nonzero turn', () => {
  const profile = deriveAnime25DShellProfile(playbackSource)
  profile.hair.crownRound = 1
  const rest = {
    x: profile.head.centerX + profile.head.radiusX * 0.5,
    y: profile.head.centerY - profile.head.radiusY * 0.7,
  }
  const rotation = { active: false, yawCosine: 1, yawSine: 0, pitchCosine: 1, pitchSine: 0 }
  for (const sign of [-1, 1]) {
    const tiny = { ...rest }
    writeAnime25DShellRotation(sign * 1e-7, sign * 1e-7, rotation)
    deformAnime25DShellPoint(tiny, rest.y, 'front-hair', profile, rotation, 1.28)
    assert.ok(Math.hypot(tiny.x - rest.x, tiny.y - rest.y) < 0.0001, 'a tiny head input cannot inject a static crown reshape')
  }
  writeAnime25DShellRotation(1, 1, rotation)
  const corrected = { ...rest }
  deformAnime25DShellPoint(corrected, rest.y, 'front-hair', profile, rotation, 1.28)
  profile.hair.crownRound = 0
  const uncorrected = { ...rest }
  deformAnime25DShellPoint(uncorrected, rest.y, 'front-hair', profile, rotation, 1.28)
  assert.ok(Math.hypot(corrected.x - uncorrected.x, corrected.y - uncorrected.y) > 1, 'the correction remains active at a full turn')
})

test('automatic hairline attachment releases monotonically across two mesh rows', () => {
  const profile = deriveAnime25DShellProfile(playbackSource)
  const source: Anime25DPlaybackLayer = {
    ...frontHairLayer,
    x: 260,
    y: 60,
    w: 80,
    h: 80,
    strands: [{ x: 300, rootY: 100, tipY: 260 }],
  }
  const rest = new Float32Array([300, 60, 300, 100, 300, 140, 300, 180])

  const weights = sampleAnime25DHairlinePinWeights(rest, source, 2, profile)

  assert.ok(weights)
  assert.deepEqual(Iterator.from(weights).toArray(), [1, 1, 0.5, 0])
})

test('enables only a restrained crown fit when distributed roots confirm scalp hair', () => {
  const rootedFrontHair = {
    ...frontHairLayer,
    strands: [220, 300, 468, 548].map((x, index) => ({
      x,
      rootY: 82 + index * 4,
      tipY: 330,
    })),
  }
  const profile = deriveAnime25DShellProfile({
    ...playbackSource,
    layers: [faceLayer, rootedFrontHair, topwearLayer],
  })

  assert.ok(profile.hair.crownRound >= 0.08)
  assert.ok(profile.hair.crownRound <= 0.22)
  assert.ok(profile.hair.radiusX >= profile.head.radiusX * 1.05)
  assert.ok(profile.hair.radiusX <= profile.head.radiusX * 1.28)
})

test('keeps crown wrap dormant when layer bounds lack distributed strand evidence', () => {
  const oversizedHair = {
    ...frontHairLayer,
    x: -1000,
    y: -500,
    w: 4000,
    h: 2200,
    strands: [],
  }
  const profile = deriveAnime25DShellProfile({
    ...playbackSource,
    layers: [faceLayer, oversizedHair, topwearLayer],
  })

  assert.equal(profile.hair.crownRound, 0)
  assert.ok(profile.hair.radiusX <= profile.head.radiusX * 1.28)
  assert.ok(profile.hair.radiusY <= profile.head.radiusY * 1.24)
})
