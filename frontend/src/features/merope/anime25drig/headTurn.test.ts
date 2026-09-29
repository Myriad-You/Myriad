import assert from 'node:assert/strict'
import test from 'node:test'
import {
  createHeadTurn,
  HEAD_TURN_RADIANS,
  headSilhouetteFromFace,
  headSilhouetteRow,
  headTurnOffset,
  HeadTurnTable,
  updateHeadTurn,
} from './headTurn'

function ellipseFace(width: number, height: number) {
  const pixels = new Uint8ClampedArray(width * height * 4)
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const nx = (x + 0.5 - width / 2) / (width / 2)
      // Widest a third of the way down, narrowing to a chin.
      const ny = (y + 0.5 - height / 3) / (y < height / 3 ? height / 3 : (height * 2) / 3)
      if (nx * nx + ny * ny <= 1) pixels[(y * width + x) * 4 + 3] = 255
    }
  }
  return { pixels, width, height }
}

const face = { x: 100, y: 200, w: 200, h: 240 }

test('a head that does not turn does not move', () => {
  const table = new HeadTurnTable()
  table.update(0)
  for (let u = -1.4; u <= 1.4; u += 0.1) assert.equal(table.slideAt(u), 0)
})

test('the far rim stays while the near rim comes in', () => {
  const table = new HeadTurnTable()
  table.update(0.42)
  assert.equal(table.slideAt(1), 0)
  assert.equal(table.slideAt(1.3), 0)
  const near = table.slideAt(-1)
  assert.ok(near > 0.04 && near < 0.2, `near rim ${near}`)
  assert.equal(table.slideAt(-1.3), near)
  // The middle of the face travels about as far as a ball's would.
  assert.ok(Math.abs(table.slideAt(0) - Math.sin(0.42)) < 0.05, `middle ${table.slideAt(0)}`)
})

test('the turn never folds the face over itself', () => {
  const table = new HeadTurnTable()
  table.update(HEAD_TURN_RADIANS)
  let previous = -Infinity
  for (let u = -1; u <= 1.0001; u += 0.01) {
    const turned = u + table.slideAt(u)
    assert.ok(turned > previous, `fold at ${u}`)
    if (Number.isFinite(previous)) assert.ok((turned - previous) / 0.01 > 0.35, `crushed at ${u}`)
    previous = turned
  }
})

test('a turn the other way is the mirror image', () => {
  const right = new HeadTurnTable()
  const left = new HeadTurnTable()
  right.update(0.3)
  left.update(-0.3)
  for (let u = -1.2; u <= 1.2; u += 0.15) {
    assert.ok(Math.abs(left.slideAt(u) + right.slideAt(-u)) < 1e-6)
  }
})

test('the silhouette follows the drawn outline below its widest row', () => {
  const silhouette = headSilhouetteFromFace(face, ellipseFace(100, 120))
  assert.ok(silhouette)
  assert.ok(Math.abs(silhouette.radius - 100) < 6, `radius ${silhouette.radius}`)
  assert.ok(Math.abs(silhouette.widestY - 280) < 8, `widest ${silhouette.widestY}`)
  assert.ok(silhouette.crownY < silhouette.widestY - silhouette.radius)
  const row = { centerX: 0, halfWidth: 0, weight: 0 }
  headSilhouetteRow(silhouette, 400, row)
  assert.ok(row.halfWidth < 70, `jaw ${row.halfWidth}`)
  assert.ok(Math.abs(row.centerX - 200) < 2)
  assert.equal(row.weight, 1)
  // Above the widest row the skull stays round, however little forehead shows.
  headSilhouetteRow(silhouette, 230, row)
  assert.ok(row.halfWidth > 80, `skull ${row.halfWidth}`)
  headSilhouetteRow(silhouette, 700, row)
  assert.equal(row.weight, 0)
})

function turnedX(turn: ReturnType<typeof createHeadTurn>, x: number, y: number, surface: 'skin' | 'front-hair' | 'back-hair', lift: number, crown: number) {
  return headTurnOffset(turn, x, y, surface, lift, crown, { x: 0, y: 0 }).x
}

test('what stands off the face travels further, and the back of the head goes the other way', () => {
  const turn = createHeadTurn(headSilhouetteFromFace(face, ellipseFace(100, 120)))
  updateHeadTurn(turn, 1, 0)
  const skin = turnedX(turn, 200, 300, 'skin', 0, 0)
  const nose = turnedX(turn, 200, 300, 'skin', 30, 0)
  const rear = turnedX(turn, 200, 300, 'back-hair', 0, 0)
  const crown = turnedX(turn, 200, 300, 'back-hair', 0, 1)
  assert.ok(skin > 20, `skin ${skin}`)
  assert.ok(nose > skin + 8, `nose ${nose}`)
  assert.ok(rear < -20, `rear ${rear}`)
  assert.ok(crown > 20, `crown ${crown}`)
  assert.equal(turnedX(turn, 200, 900, 'skin', 0, 0), 0)
  updateHeadTurn(turn, 0, 0)
  assert.equal(turnedX(turn, 200, 300, 'skin', 30, 0), 0)
})

test('a raised face slides toward the crown, which stays, while the chin comes up', () => {
  const silhouette = headSilhouetteFromFace(face, ellipseFace(100, 120))!
  const turn = createHeadTurn(silhouette)
  updateHeadTurn(turn, 0, 1)
  const at = (y: number, surface: 'skin' | 'back-hair' = 'skin', lift = 0) =>
    headTurnOffset(turn, 200, y, surface, lift, 0, { x: 0, y: 0 })
  const middle = (silhouette.crownY + silhouette.chinY) / 2
  assert.ok(at(middle).y < -15, `middle ${at(middle).y}`)
  assert.ok(Math.abs(at(silhouette.crownY + 1).y) < 3, `crown ${at(silhouette.crownY + 1).y}`)
  assert.ok(at(silhouette.chinY).y < -2, `chin ${at(silhouette.chinY).y}`)
  assert.ok(at(middle, 'skin', 30).y < at(middle).y - 5)
  assert.ok(at(middle, 'back-hair').y > 15)
  assert.equal(at(middle).x, 0)
})

test('a face with nothing to read turns the old way', () => {
  assert.equal(headSilhouetteFromFace(face, null), null)
  const turn = createHeadTurn(null)
  updateHeadTurn(turn, 1, 1)
  assert.equal(turn.active, false)
  assert.equal(turnedX(turn, 200, 300, 'skin', 30, 0), 0)
})
