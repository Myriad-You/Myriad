import assert from 'node:assert/strict'
import test from 'node:test'
import { alignSkeletonToPsd, skeletonInFrame } from './skeleton'

function keypoints(points: Record<number, readonly [number, number, number]>) {
  return Array.from({ length: 133 }, (_, index) => points[index] ?? ([0, 0, 0] as const))
}

test('a portrait\'s joints land where the portrait was padded and scaled onto the PSD', () => {
  // A 3:4 portrait padded to 1536 square and scaled to a 1280 PSD.
  const aligned = alignSkeletonToPsd(
    {
      sourceMasterAssetId: '/m.png',
      model: 'dwpose',
      width: 1152,
      height: 1536,
      keypoints: keypoints({ 0: [576, 300, 0.9] }),
    },
    1280,
    1280,
  )!
  const [x, y, score] = aligned.keypoints[0]
  assert.equal(x, ((576 + 192) * 1280) / 1536)
  assert.equal(y, (300 * 1280) / 1536)
  assert.equal(score, 0.9)
  assert.equal(
    alignSkeletonToPsd({ sourceMasterAssetId: '/m.png', model: 'dwpose', width: 10, height: 10, keypoints: [] }, 100, 80),
    null,
    'a PSD that is not square was not split from the portrait',
  )
})

test('joints are named by side of the picture and kept only when seen inside the frame', () => {
  const skeleton = skeletonInFrame(
    {
      model: 'dwpose',
      keypoints: keypoints({
        0: [500, 200, 0.9],
        // The person's left shoulder is on the right of the picture.
        5: [600, 400, 0.95],
        6: [400, 410, 0.9],
        // One elbow is unsure: the other is still kept, on its own side.
        7: [620, 600, 0.9],
        8: [380, 600, 0.2],
        // Hips below the frame.
        11: [560, 2000, 0.9],
        12: [440, 2000, 0.9],
      }),
    },
    { x: 100, y: 50, width: 800, height: 1200 },
  )!
  assert.deepEqual(skeleton.joints.nose, { x: 400, y: 150, score: 0.9 })
  assert.deepEqual(skeleton.joints.shoulderL, { x: 300, y: 360, score: 0.9 })
  assert.deepEqual(skeleton.joints.shoulderR, { x: 500, y: 350, score: 0.95 })
  assert.equal(skeleton.joints.elbowL, undefined)
  assert.deepEqual(skeleton.joints.elbowR, { x: 520, y: 550, score: 0.9 })
  assert.equal(skeleton.joints.hipL, undefined)
  assert.equal(
    skeletonInFrame({ model: 'dwpose', keypoints: keypoints({}) }, { x: 0, y: 0, width: 10, height: 10 }),
    null,
  )
})

test('a limb crossing over keeps the side of the torso it hangs from', () => {
  const skeleton = skeletonInFrame(
    {
      model: 'dwpose',
      keypoints: keypoints({
        // Facing the viewer: the person's left shoulder and hip are on the right.
        5: [600, 400, 0.9],
        6: [400, 400, 0.9],
        11: [560, 800, 0.9],
        12: [440, 800, 0.9],
        // The left wrist has crossed to the picture's left, past the right one.
        9: [380, 700, 0.9],
        10: [450, 700, 0.9],
        // Legs crossed at the ankles.
        15: [470, 1500, 0.9],
        16: [530, 1500, 0.9],
      }),
    },
    { x: 0, y: 0, width: 1000, height: 1600 },
  )!
  assert.equal(skeleton.joints.wristR?.x, 380)
  assert.equal(skeleton.joints.wristL?.x, 450)
  assert.equal(skeleton.joints.ankleR?.x, 470)
  assert.equal(skeleton.joints.ankleL?.x, 530)
})

test('seen from behind, the person\'s left is the picture\'s left; without a torso a pair goes by position', () => {
  const behind = skeletonInFrame(
    {
      model: 'dwpose',
      keypoints: keypoints({ 5: [400, 400, 0.9], 6: [600, 400, 0.9], 13: [430, 1000, 0.9] }),
    },
    { x: 0, y: 0, width: 1000, height: 1600 },
  )!
  // The hips are not seen: the shoulders tell the legs' sides too.
  assert.equal(behind.joints.kneeL?.x, 430)
  assert.equal(behind.joints.kneeR, undefined)
  const torsoless = skeletonInFrame(
    {
      model: 'dwpose',
      keypoints: keypoints({ 13: [600, 1000, 0.9], 14: [400, 1000, 0.9], 15: [600, 1400, 0.9] }),
    },
    { x: 0, y: 0, width: 1000, height: 1600 },
  )!
  assert.equal(torsoless.joints.kneeL?.x, 400)
  assert.equal(torsoless.joints.kneeR?.x, 600)
  assert.equal(torsoless.joints.ankleL, undefined, 'a lone joint without a torso has no side')
  assert.equal(torsoless.joints.ankleR, undefined)
})
