import assert from 'node:assert/strict'
import test from 'node:test'
import {
  createHeadTurn,
  headSilhouetteFromFace,
  headSilhouetteRow,
  headTurnFeatureKey,
  headTurnFeatures,
  headTurnOffset,
  moveHeadFeature,
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

const silhouette = () => headSilhouetteFromFace(face, ellipseFace(100, 120))!
function slideAcross(turn: ReturnType<typeof createHeadTurn>, u: number) {
  const row = { centerX: 0, halfWidth: 0, weight: 0 }
  headSilhouetteRow(turn.silhouette!, 300, row)
  return headTurnOffset(turn, row.centerX + u * row.halfWidth, 300, 'skin', 0, 0, { x: 0, y: 0 }).x / row.halfWidth
}

test('a head that does not turn does not move', () => {
  const turn = createHeadTurn(silhouette())
  updateHeadTurn(turn, 0, 0)
  for (let u = -1.4; u <= 1.4; u += 0.1) assert.equal(slideAcross(turn, u), 0)
})

test('the far rim stays while the near rim comes in', () => {
  const turn = createHeadTurn(silhouette())
  updateHeadTurn(turn, 1, 0)
  assert.equal(slideAcross(turn, 1), 0)
  assert.equal(slideAcross(turn, 1.3), 0)
  const near = slideAcross(turn, -1)
  assert.ok(near > 0.06 && near < 0.12, `near rim ${near}`)
  assert.ok(Math.abs(slideAcross(turn, -1.3) - near) < 1e-9)
  assert.ok(slideAcross(turn, 0) > 0.3, `middle ${slideAcross(turn, 0)}`)
})

test('every point moves in step with the angle, none stopping early', () => {
  const turn = createHeadTurn(silhouette())
  for (const u of [-0.9, -0.4, 0, 0.4, 0.8]) {
    updateHeadTurn(turn, 1, 0)
    const full = slideAcross(turn, u)
    for (const angle of [0.2, 0.5, 0.8]) {
      updateHeadTurn(turn, angle, 0)
      assert.ok(Math.abs(slideAcross(turn, u) - full * angle) < 1e-9, `u ${u} angle ${angle}`)
    }
  }
})

test('the skin never folds, and stretches or crowds only so far', () => {
  const turn = createHeadTurn(silhouette())
  updateHeadTurn(turn, 1, 0)
  for (let u = -0.99; u <= 0.99; u += 0.02) {
    const slope = 1 + (slideAcross(turn, u + 0.01) - slideAcross(turn, u - 0.01)) / 0.02
    assert.ok(slope > 0.38 && slope < 1.62, `slope ${slope} at ${u}`)
  }
})

test('a turn the other way is the mirror image', () => {
  const right = createHeadTurn(silhouette())
  const left = createHeadTurn(silhouette())
  updateHeadTurn(right, 0.6, 0)
  updateHeadTurn(left, -0.6, 0)
  for (let u = -1.2; u <= 1.2; u += 0.15) {
    assert.ok(Math.abs(slideAcross(left, u) + slideAcross(right, -u)) < 1e-6)
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
  assert.ok(skin > 12, `skin ${skin}`)
  assert.ok(nose > skin + 5, `nose ${nose}`)
  assert.ok(rear < -12, `rear ${rear}`)
  assert.ok(crown > 12, `crown ${crown}`)
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

test('every drawing of one eye turns as one piece', () => {
  const layer = (role: string, side: string | null, x = 0) => ({ group: 'head', role, side, x, y: 300, w: 40, h: 20 })
  assert.equal(headTurnFeatureKey(layer('eyewhite', 'L')), 'eye:L')
  assert.equal(headTurnFeatureKey(layer('eye-dizzy', 'L')), 'eye:L')
  assert.equal(headTurnFeatureKey(layer('iris-silly', 'R')), 'eye:R')
  assert.equal(headTurnFeatureKey(layer('eyebrow', 'R')), 'brow:R')
  assert.equal(headTurnFeatureKey(layer('mouth-open', null)), 'mouth')
  assert.equal(headTurnFeatureKey(layer('maniac-mouth-shadow', null)), null)
  assert.equal(headTurnFeatureKey(layer('face', null)), null)
  assert.equal(headTurnFeatureKey({ ...layer('nose', null), group: 'body' }), null)
  const white = layer('eyewhite', 'L', 140)
  const iris = layer('irides', 'L', 150)
  const dizzy = { ...layer('eye-dizzy', 'L', 120), w: 90 }
  const features = headTurnFeatures([white, iris, dizzy, layer('face', null)])
  assert.equal(features.get(white), features.get(dizzy))
  // Bounds come from the everyday drawings, not a larger effect overlay.
  assert.equal(features.get(white)!.centerX, 165)
  assert.equal(features.size, 3)
})

test('a feature keeps its drawn shape as it rides the ball', () => {
  const turn = createHeadTurn(headSilhouetteFromFace(face, ellipseFace(100, 120)))
  updateHeadTurn(turn, 1, 0)
  const far = { centerX: 250, centerY: 290, halfWidth: 20, halfHeight: 10 }
  const near = { ...far, centerX: 150 }
  const width = (feature: typeof far) => {
    const left = { x: feature.centerX - 20, y: 290 }
    const right = { x: feature.centerX + 20, y: 290 }
    moveHeadFeature(turn, left, feature, 0)
    moveHeadFeature(turn, right, feature, 0)
    return (right.x - left.x) / 40
  }
  const farWidth = width(far)
  const nearWidth = width(near)
  assert.ok(farWidth < 1 && farWidth > 0.859, `far ${farWidth}`)
  assert.ok(nearWidth > 1 && nearWidth <= 1.08, `near ${nearWidth}`)
  // Top and bottom of the feature move together: it is not sheared.
  const top = { x: 250, y: 280 }
  const bottom = { x: 250, y: 300 }
  moveHeadFeature(turn, top, far, 0)
  moveHeadFeature(turn, bottom, far, 0)
  assert.ok(Math.abs(top.x - bottom.x) < 1e-9)
})

test('a raised face foreshortens: the mouth rises at least as far as the eyes, never left behind', () => {
  const silhouette = headSilhouetteFromFace(face, ellipseFace(100, 120))!
  const turn = createHeadTurn(silhouette)
  const eyeY = silhouette.widestY + 10
  const mouthY = silhouette.chinY - 30
  for (const angle of [1, -1]) {
    updateHeadTurn(turn, 0, angle)
    const eye = headTurnOffset(turn, 200, eyeY, 'skin', 0, 0, { x: 0, y: 0 }).y
    const mouth = headTurnOffset(turn, 200, mouthY, 'skin', 0, 0, { x: 0, y: 0 }).y
    // Raised, both rise and the gap between them closes; lowered, it closes too.
    assert.ok(angle > 0 ? mouth <= eye && eye < 0 : mouth >= 0, `angle ${angle}: eye ${eye} mouth ${mouth}`)
  }
})
