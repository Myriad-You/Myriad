import assert from 'node:assert/strict'
import test from 'node:test'
import { isDocumentTurnKeyforms, placeBodyTurnKeyforms, placeTurnKeyforms } from './turnKeyformImport'

const lattice = { box: [100, 200, 300, 400] as [number, number, number, number], grid: 2, back: [0, 0, 0, 0, 0, 0, 0, 0] }
const key = { plus: lattice, minus: lattice }

test("a figure's body keys come with its head keys and are cut as the playback is", () => {
  const keys = { canvas: [1000, 2000], keyforms: { face: key }, bodyKeyforms: { topwear: key } }
  assert.ok(isDocumentTurnKeyforms(keys))
  assert.ok(!isDocumentTurnKeyforms({ ...keys, bodyKeyforms: { topwear: { plus: lattice } } }))
  const document = { width: 1000, height: 2000 }
  const frame = { x: -20, y: 10 }
  const head = placeTurnKeyforms(keys as never, document, frame)!
  const body = placeBodyTurnKeyforms(keys as never, document, frame)!
  assert.deepEqual(head.face.plus.box, [120, 190, 320, 390])
  assert.deepEqual(body.topwear.minus.box, [120, 190, 320, 390])
  assert.equal(placeBodyTurnKeyforms({ canvas: [1000, 2000], keyforms: { face: key } }, document, frame), undefined)
  const weighted = { ...keys, weightKeyforms: { legwear: key } }
  assert.ok(isDocumentTurnKeyforms(weighted))
  assert.deepEqual(placeBodyTurnKeyforms(weighted as never, document, frame, 'weightKeyforms')!.legwear.plus.box, [120, 190, 320, 390])
  assert.equal(placeBodyTurnKeyforms(keys as never, document, frame, 'weightKeyforms'), undefined)
  assert.equal(placeBodyTurnKeyforms(keys as never, { width: 500, height: 2000 }, frame), undefined)
})
