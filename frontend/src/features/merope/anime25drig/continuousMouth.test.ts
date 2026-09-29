import type { Anime25DPlaybackLayer } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { paintContinuousLipMouth, paintContinuousMouth } from './continuousMouth'
import { prepareContinuousMouth } from './continuousMouthTexture'
import { IDENTITY_DRIVER } from './driver'
import { createAnime25DOpacityFrame, fadeOpacityFromFrame, writeAnime25DOpacityFrame } from './mouthRuntime'

const PALETTE = {
  line: { red: 104, green: 57, blue: 75 },
  cavity: { red: 91, green: 45, blue: 65 },
  fill: { red: 232, green: 139, blue: 151 },
}
const LIPS = { lips: { red: 176, green: 62, blue: 92 }, teeth: { red: 246, green: 240, blue: 238 } }
const W = 60
const H = 36

function coverage(data: Uint8ClampedArray) {
  let total = 0
  for (let index = 3; index < data.length; index += 4) total += data[index] / 255
  return total
}

function difference(first: Uint8ClampedArray, second: Uint8ClampedArray) {
  let total = 0
  for (let index = 0; index < first.length; index++) total += Math.abs(first[index] - second[index])
  return total / first.length
}

/** Rows and columns with any paint, as the trimmed bounds the drawing fills. */
function extent(data: Uint8ClampedArray, width: number, height: number) {
  let left = width
  let right = -1
  let top = height
  let bottom = -1
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (data[(y * width + x) * 4 + 3] < 8) continue
      left = Math.min(left, x)
      right = Math.max(right, x)
      top = Math.min(top, y)
      bottom = Math.max(bottom, y)
    }
  }
  return { left, right, top, bottom }
}

test('a small change in the voice is a small change in the mouth, never a swap', () => {
  const before = new Uint8ClampedArray(W * H * 4)
  const after = new Uint8ClampedArray(W * H * 4)
  const far = new Uint8ClampedArray(W * H * 4)
  for (const lips of [false, true]) {
    const paint = (shape: { wide: number, round: number, narrow: number }, out: Uint8ClampedArray) =>
      lips ? paintContinuousLipMouth(shape, LIPS, W, H, out) : paintContinuousMouth(shape, PALETTE, W, H, out)
    paint({ wide: 0.3, round: 0.2, narrow: 0 }, before)
    paint({ wide: 0.32, round: 0.2, narrow: 0 }, after)
    paint({ wide: 0, round: 1, narrow: 0 }, far)
    const step = difference(before, after)
    const jump = difference(before, far)
    assert.ok(step < jump / 8, `${lips ? 'lips' : 'line'}: step ${step} jump ${jump}`)
  }
})

test('the mouth fills its place as a trimmed drawing does, tilted with the face', () => {
  const out = new Uint8ClampedArray(W * H * 4)
  for (const roll of [0, 0.3]) {
    paintContinuousMouth({ wide: 0.5, round: 0, narrow: 0 }, PALETTE, W, H, out, roll)
    const line = extent(out, W, H)
    assert.ok(line.left <= 1 && line.right >= W - 2 && line.top <= 1 && line.bottom >= H - 2, JSON.stringify(line))
    paintContinuousLipMouth({ wide: 0.5, round: 0, narrow: 0 }, LIPS, W, H, out, roll)
    const lips = extent(out, W, H)
    assert.ok(lips.left <= 1 && lips.right >= W - 2 && lips.top <= 1 && lips.bottom >= H - 2, JSON.stringify(lips))
  }
})

test('a narrow mouth nearly shuts its lips; an open one shows its opening', () => {
  const cavityShare = (narrow: number) => {
    const out = new Uint8ClampedArray(W * H * 4)
    paintContinuousLipMouth({ wide: 0, round: 0, narrow }, LIPS, W, H, out)
    let dark = 0
    for (let index = 0; index < out.length; index += 4) {
      if (out[index + 3] > 200 && out[index] + out[index + 1] + out[index + 2] < 200) dark++
    }
    return dark / coverage(out)
  }
  assert.ok(cavityShare(1) < cavityShare(0) * 0.8, `${cavityShare(1)} vs ${cavityShare(0)}`)
})

function layer(fade: Anime25DPlaybackLayer['fade'], synthetic: boolean): Anime25DPlaybackLayer {
  return {
    name: String(fade),
    role: String(fade),
    depth: 1.08,
    group: 'head',
    phys: null,
    fade,
    side: null,
    x: 100,
    y: 200,
    w: 40,
    h: 24,
    atlas: { x: 0.1, y: 0.1, w: 0.04, h: 0.024 },
    strands: [],
    ...(synthetic ? { synthetic: true as const } : {}),
  }
}

const ANCHORS = {
  face: { x0: 0, x1: 300, y0: 0, y1: 400, cx: 150, cy: 200 },
  neckPivot: { x: 150, y: 420 },
  neckTop: 400,
  neckBottom: 440,
  bodyPivot: { x: 150, y: 800 },
  mouth: { x0: 130, x1: 170, y0: 300, y1: 310, cx: 150, cy: 305 },
  faceScale: 1,
}

function closedPixels(lips: boolean) {
  const width = 30
  const height = lips ? 14 : 4
  const pixels = new Uint8ClampedArray(width * height * 4)
  for (let index = 0; index < pixels.length; index += 4) {
    pixels.set(lips ? [180, 60, 90, 255] : [90, 50, 60, 255], index)
  }
  return { width, height, pixels }
}

test('only the importer\'s speaking mouths are redrawn, in the portrait\'s own painting', () => {
  const fades = ['mouthClose', 'mouthOpen', 'mouthWide', 'mouthRound', 'mouthNarrow'] as const
  const drawn = fades.map((fade) => layer(fade, fade !== 'mouthClose'))
  const setup = prepareContinuousMouth(drawn, ANCHORS, 1000, 1000, () => closedPixels(false))
  assert.ok(setup)
  assert.equal(setup.layer.fade, 'mouthOpen')
  assert.equal(setup.lips, null)
  assert.equal(setup.regionWidth, 40)
  const painted = prepareContinuousMouth(drawn, ANCHORS, 1000, 1000, () => closedPixels(true))
  assert.ok(painted?.lips)
  // An artist's own open mouth is kept.
  const authored = fades.map((fade) => layer(fade, fade !== 'mouthClose' && fade !== 'mouthRound'))
  assert.equal(prepareContinuousMouth(authored, ANCHORS, 1000, 1000, () => closedPixels(false)), null)
  assert.equal(prepareContinuousMouth(drawn, ANCHORS, 1000, 1000, () => null), null)
})

test('drawn live, the open mouth stands in for every speaking shape', () => {
  const frame = createAnime25DOpacityFrame()
  const driver = { ...IDENTITY_DRIVER, mouthOpen: 0.8, mouthRound: 1 }
  const opacity = (fade: Anime25DPlaybackLayer['fade']) => fadeOpacityFromFrame({ fade, side: null }, frame)
  writeAnime25DOpacityFrame(frame, driver, 'mouthRound', 1, undefined, false)
  assert.equal(opacity('mouthOpen'), 0)
  assert.ok(opacity('mouthRound') > 0.99)
  writeAnime25DOpacityFrame(frame, driver, 'mouthRound', 1, { previous: 'mouthWide', handoff: 0.3 }, true)
  assert.ok(opacity('mouthOpen') > 0.99)
  assert.equal(opacity('mouthRound'), 0)
  assert.equal(opacity('mouthWide'), 0)
})
