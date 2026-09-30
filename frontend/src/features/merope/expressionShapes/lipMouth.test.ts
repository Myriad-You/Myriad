import assert from 'node:assert/strict'
import test from 'node:test'
import { createLipMouthBitmap, detectPaintedLips, LIP_MOUTH_KINDS, lipMouthSize } from './lipMouth'

function mouth(width: number, height: number, paint: (x: number, y: number) => readonly [number, number, number] | null) {
  const data = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const color = paint(x, y)
      if (color) data.set([...color, 255], (y * width + x) * 4)
    }
  }
  return { width, height, data }
}

// A made-up smile: red lips around a band of teeth.
const lipstick = mouth(60, 30, (x, y) => {
  const u = (x - 30) / 30
  const v = (y - 15) / 15
  if (u * u + v * v > 1) return null
  return Math.abs(v) < 0.3 ? [238, 232, 230] : [170, 60, 80]
})
// A cel mouth: one thin pale stroke.
const line = mouth(60, 12, (_x, y) => (y >= 5 && y <= 6 ? [220, 170, 170] : null))

test('a painted mouth with lips is told apart from a cel line', () => {
  const lips = detectPaintedLips(lipstick)
  assert.ok(lips)
  assert.ok(lips.lips.red > 150 && lips.lips.green < 90)
  assert.ok(lips.teeth.red > 220 && lips.teeth.green > 220)
  assert.equal(detectPaintedLips(line), null)
})

test('lip mouths are painted in the portrait\'s lips and teeth, with no dark cel rim', () => {
  const painted = detectPaintedLips(lipstick)!
  for (const kind of ['open', 'wide', 'round', 'narrow'] as const) {
    const size = lipMouthSize(kind, { width: 120, height: 180 })
    assert.ok(size.height < 120 * 0.8, `${kind} keeps a lip mouth's height`)
    const bitmap = createLipMouthBitmap(kind, size, painted)
    let opaque = 0
    let nearBlack = 0
    let lipLike = 0
    let teethLike = 0
    for (let index = 0; index < bitmap.data.length; index += 4) {
      if (bitmap.data[index + 3] < 200) continue
      opaque++
      const [red, green, blue] = [bitmap.data[index], bitmap.data[index + 1], bitmap.data[index + 2]]
      if (red + green + blue < 90) nearBlack++
      if (red > green * 1.3) lipLike++
      const bright = red * 0.299 + green * 0.587 + blue * 0.114
      if (bright > 170 && (Math.max(red, green, blue) - Math.min(red, green, blue)) / Math.max(red, green, blue) < 0.15) teethLike++
    }
    assert.ok(opaque > 50, kind)
    assert.ok(nearBlack / opaque < 0.05, `${kind}: ${nearBlack}/${opaque} near-black`)
    assert.ok(lipLike / opaque > 0.4, `${kind}: lips`)
    if (kind !== 'narrow') assert.ok(teethLike > 0, `${kind}: teeth`)
  }
})

test('a face with painted lips gets its crying, laughing and tongue-out mouths painted in them too', () => {
  const painted = { lips: { red: 176, green: 62, blue: 92 }, teeth: { red: 246, green: 240, blue: 238 } }
  for (const kind of ['cry', 'maniac', 'silly'] as const) {
    assert.ok(LIP_MOUTH_KINDS.has(kind))
    const bitmap = createLipMouthBitmap(kind, { width: 90, height: 70 }, painted)
    let lip = 0
    let near = 0
    let white = 0
    for (let index = 0; index < bitmap.data.length; index += 4) {
      if (bitmap.data[index + 3] < 200) continue
      const [red, green, blue] = [bitmap.data[index], bitmap.data[index + 1], bitmap.data[index + 2]]
      if (red > green * 1.6 && red > 120) lip++
      if (Math.max(red, green, blue) < 40) near++
      if (red > 200 && green > 200 && blue > 200) white++
    }
    // Lip-coloured, never a near-black cel rim.
    assert.ok(lip > 60, `${kind} lip ${lip}`)
    assert.ok(near < lip * 0.05, `${kind} black ${near}`)
    // A tongue out shows no teeth; a wail and a laugh do.
    if (kind === 'silly') assert.equal(white, 0)
    else assert.ok(white > 10, `${kind} teeth ${white}`)
  }
})
