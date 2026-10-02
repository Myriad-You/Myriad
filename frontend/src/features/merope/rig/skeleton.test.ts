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
        // One elbow is unsure: the pair is dropped rather than half kept.
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
  assert.equal(skeleton.joints.hipL, undefined)
  assert.equal(
    skeletonInFrame({ model: 'dwpose', keypoints: keypoints({}) }, { x: 0, y: 0, width: 10, height: 10 }),
    null,
  )
})
