import assert from 'node:assert/strict'
import test from 'node:test'
import { anime25DArmsTouch, anime25DHandTouchesHead, armDrapeWeight, bindArmRig, bindArmRigMesh } from './armRig'

const ANCHORS = {
  face: { x0: 300, y0: 150, x1: 700, y1: 650, cx: 500, cy: 400 },
  neckPivot: { x: 500, y: 740 },
  neckBottom: 770,
}

type Paint = (x: number, y: number) => boolean

const FABRIC = [200, 205, 235]
const SKIN = [250, 222, 205]

function sleeve(
  layer: { x: number; y: number; w: number; h: number },
  paint: Paint,
  color: (x: number, y: number) => number[] = () => FABRIC,
) {
  const pixels = new Uint8ClampedArray(layer.w * layer.h * 4)
  for (let y = 0; y < layer.h; y++) {
    for (let x = 0; x < layer.w; x++) {
      if (!paint(layer.x + x, layer.y + y)) continue
      pixels.set([...color(layer.x + x, layer.y + y), 255], (y * layer.w + x) * 4)
    }
  }
  return { pixels, width: layer.w, height: layer.h }
}

// A hanging image-left sleeve: a 160px-wide column from the shoulder to the crop.
const LEFT = { x: 120, y: 780, w: 220, h: 540, side: 'L' as const }
const hanging: Paint = (x, y) => x >= 150 && x < 310 && y >= 790

test('the joint sits inside the sleeve top nearest the anatomical shoulder', () => {
  const rig = bindArmRig(LEFT, sleeve(LEFT, hanging), ANCHORS, 1320)!
  assert.equal(rig.outward, 1)
  assert.ok(rig.pivotX > 150 && rig.pivotX < 310, `${rig.pivotX}`)
  assert.ok(rig.pivotY > 790 && rig.pivotY < 900, `${rig.pivotY}`)
  assert.equal(rig.scale, 1)
  assert.equal(rig.cutY, 1320)
})

test('the image-right sleeve swings the other way', () => {
  const right = { ...LEFT, x: 660, side: 'R' as const }
  const rig = bindArmRig(right, sleeve(right, (x, y) => x >= 690 && x < 850 && y >= 790), ANCHORS, 1320)!
  assert.equal(rig.outward, -1)
  assert.ok(rig.pivotX > 690 && rig.pivotX < 850)
})

test('a sleeve that ends above the crop has no cut to slide along', () => {
  const short = { ...LEFT, h: 300 }
  const rig = bindArmRig(short, sleeve(short, (x, y) => hanging(x, y) && y < 1060), ANCHORS, 1320)!
  assert.equal(rig.cutY, null)
})

test('a tapered sleeve tip reaching the crop edge is not taken for a cut', () => {
  const rig = bindArmRig(LEFT, sleeve(LEFT, (x, y) => hanging(x, y) && (y < 1300 || x < 156)), ANCHORS, 1320)!
  assert.equal(rig.cutY, null)
})

test('a hand raised beside the face is not mistaken for the shoulder', () => {
  const posed = { x: 120, y: 500, w: 260, h: 820, side: 'L' as const }
  // The forearm rises from the shoulder to a hand level with the chin.
  const paint: Paint = (x, y) =>
    (x >= 150 && x < 310 && y >= 790) || (x >= 300 && x < 380 && y >= 520 && y < 800)
  const rig = bindArmRig(posed, sleeve(posed, paint), ANCHORS, 1320)!
  assert.ok(rig.pivotY >= ANCHORS.neckBottom - 25, `${rig.pivotY}`)
  assert.ok(rig.scale < 1)
})

test('no pixels, no side, or an empty drawing bind no joint', () => {
  assert.equal(bindArmRig(LEFT, null, ANCHORS, 1320), null)
  assert.equal(bindArmRig({ ...LEFT, side: null }, sleeve(LEFT, hanging), ANCHORS, 1320), null)
  assert.equal(bindArmRig(LEFT, sleeve(LEFT, () => false), ANCHORS, 1320), null)
})

test('mesh weights are zero at the joint and whole beyond the shoulder', () => {
  const rig = bindArmRig(LEFT, sleeve(LEFT, hanging), ANCHORS, 1320)!
  const rest = new Float32Array([rig.pivotX, rig.pivotY, rig.pivotX, rig.pivotY + rig.radius * 0.8, 200, 1300])
  const mesh = bindArmRigMesh(rig, rest)
  assert.equal(mesh.jointVertex, 0)
  assert.equal(mesh.weights[0], 0)
  assert.ok(mesh.weights[1] > 0 && mesh.weights[1] < 1)
  assert.equal(mesh.weights[2], 1)
})

test('a sleeve that is fabric down to the crop is a drape; a bare forearm is not', () => {
  const kimono = bindArmRig(LEFT, sleeve(LEFT, hanging), ANCHORS, 1320)!
  assert.equal(kimono.drape, true)
  // A few warm printed flowers do not make an arm.
  const printed = bindArmRig(LEFT, sleeve(LEFT, hanging, (x, y) => (x + y) % 40 === 0 ? SKIN : FABRIC), ANCHORS, 1320)!
  assert.equal(printed.drape, true)
  const forearm = bindArmRig(LEFT, sleeve(LEFT, hanging, (_x, y) => (y > 1150 ? SKIN : FABRIC)), ANCHORS, 1320)!
  assert.equal(forearm.drape, false)
})

test('a raised arm swings less and never drapes', () => {
  const posed = { x: 120, y: 500, w: 260, h: 820, side: 'L' as const }
  const paint: Paint = (x, y) =>
    (x >= 150 && x < 310 && y >= 790) || (x >= 300 && x < 380 && y >= 520 && y < 800)
  const rig = bindArmRig(posed, sleeve(posed, paint), ANCHORS, 1320)!
  assert.ok(rig.scale < 1)
  assert.equal(rig.drape, false)
})

test('drape weight is zero on the upper arm and whole at the hem', () => {
  const rig = bindArmRig(LEFT, sleeve(LEFT, hanging), ANCHORS, 1320)!
  assert.equal(armDrapeWeight(rig, rig.pivotY + rig.length * 0.3), 0)
  assert.equal(armDrapeWeight(rig, rig.pivotY + rig.length), 1)
  assert.equal(armDrapeWeight({ ...rig, drape: false }, rig.pivotY + rig.length), 0)
})

test('hands holding each other join the arms; arms apart stay apart', () => {
  const RIGHT = { x: 660, y: 780, w: 220, h: 540, side: 'R' as const }
  // Two hanging sleeves, nowhere near each other.
  const leftAlone = sleeve(LEFT, hanging)
  const rightAlone = sleeve(RIGHT, (x, y) => x >= 690 && x < 850 && y >= 790)
  assert.equal(anime25DArmsTouch(LEFT, leftAlone, RIGHT, rightAlone), false)
  // The left hand reaches across and grips the right one.
  const reaching = { x: 120, y: 780, w: 600, h: 540, side: 'L' as const }
  const leftReach = sleeve(reaching, (x, y) => (x >= 150 && x < 310 && y >= 790) || (y >= 1000 && y < 1060 && x >= 150 && x < 700))
  const rightHeld = sleeve(RIGHT, (x, y) => (x >= 690 && x < 850 && y >= 790) || (y >= 1000 && y < 1060 && x >= 700 && x < 850))
  assert.equal(anime25DArmsTouch(reaching, leftReach, RIGHT, rightHeld), true)
  assert.equal(anime25DArmsTouch(reaching, null, RIGHT, rightHeld), false)
})

test('a hand raised to the cheek rests on the head; hanging arms do not', () => {
  const hangingArm = { layer: LEFT, image: sleeve(LEFT, hanging) }
  assert.equal(anime25DHandTouchesHead([hangingArm], ANCHORS.face), false)
  // A hand lifted beside the face, inside the ear line.
  const raised = { x: 560, y: 350, w: 200, h: 700, side: 'R' as const }
  const toCheek = { layer: raised, image: sleeve(raised, (x, y) => (x >= 620 && x < 700 && y >= 380) || (y >= 380 && y < 520 && x >= 600 && x < 740)) }
  assert.equal(anime25DHandTouchesHead([hangingArm, toCheek], ANCHORS.face), true)
})
