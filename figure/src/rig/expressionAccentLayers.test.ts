import type { Anime25DRiggerAnchors } from '../anime25drig/playback'
import type { RasterLayer } from './anime25dImportTypes'
import assert from 'node:assert/strict'
import test from 'node:test'
import { resolveAnime25DFaceFrame } from '../expressionShapes/faceFrame'
import { EXPRESSION_ACCENT_ROLES, synthesizeMissingExpressionAccents } from './expressionAccentLayers'

const anchors = {
  face: { x0: 300, x1: 600, y0: 200, y1: 560, cx: 450, cy: 380 },
  eyeL: { x0: 340, y0: 340, x1: 420, y1: 400, icx: 380, icy: 372, closeY: 385 },
  eyeR: { x0: 480, y0: 340, x1: 560, y1: 400, icx: 520, icy: 372, closeY: 385 },
} as unknown as Anime25DRiggerAnchors

function layer(id: string, left: number, top: number, width: number, height: number): RasterLayer {
  return {
    id, role: 'face', sourceName: id, order: 0, side: null, group: 'head',
    left, top, width, height, data: new Uint8ClampedArray(width * height * 4),
  }
}

test('every missing accent is added beside the face, inside the picture', () => {
  const base = [layer('face', 0, 0, 900, 1200)]
  const output = synthesizeMissingExpressionAccents(base, anchors, resolveAnime25DFaceFrame(anchors), 10)
  const added = output.slice(base.length)
  assert.deepEqual(added.map((part) => part.role), EXPRESSION_ACCENT_ROLES.map(([role]) => role))
  for (const part of added) {
    assert.ok(part.left >= 0 && part.top >= 0, part.role)
    assert.ok(part.left + part.width <= 900 && part.top + part.height <= 1200, part.role)
    assert.equal(part.group, 'head')
  }
  const blush = added.find((part) => part.role === 'accent-blush')!
  assert.ok(blush.top > 385 - blush.height && blush.top < 460, 'under the eyes')
  const question = added.find((part) => part.role === 'accent-question')!
  const notes = added.find((part) => part.role === 'accent-music-notes')!
  assert.ok(question.left + question.width / 2 < 450 && notes.left + notes.width / 2 > 450)
})

test('the part budget keeps the first accents and drops the rest', () => {
  const base = [layer('face', 0, 0, 900, 1200)]
  const output = synthesizeMissingExpressionAccents(base, anchors, resolveAnime25DFaceFrame(anchors), 2)
  assert.deepEqual(output.slice(1).map((part) => part.role), ['accent-music-notes', 'accent-blush'])
  assert.equal(synthesizeMissingExpressionAccents(base, anchors, resolveAnime25DFaceFrame(anchors), 0).length, 1)
})

test('an accent the package already has is not drawn again', () => {
  const base = [layer('face', 0, 0, 900, 1200), { ...layer('notes', 10, 10, 20, 20), role: 'accent-music-notes' as const }]
  const output = synthesizeMissingExpressionAccents(base, anchors, resolveAnime25DFaceFrame(anchors), 10)
  assert.equal(output.filter((part) => part.role === 'accent-music-notes').length, 1)
})
