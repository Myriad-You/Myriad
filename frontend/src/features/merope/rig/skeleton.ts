import type { Anime25DJointName, Anime25DSkeleton } from '../anime25drig/types'

/**
 * The joints the backend found on a master portrait (DWPose, COCO-WholeBody):
 * `[x, y, score]` per keypoint, in that portrait's own pixels.
 */
export interface DetectedSkeleton {
  sourceMasterAssetId: string
  model: string
  width: number
  height: number
  keypoints: ReadonlyArray<readonly [number, number, number]>
}

/** The same keypoints moved onto the PSD canvas the portrait was split into. */
export interface PsdSkeleton {
  model: string
  keypoints: ReadonlyArray<readonly [number, number, number]>
}

/** Below this the model is guessing, often at a joint the frame cuts off. */
export const MIN_JOINT_SCORE = 0.5

/**
 * COCO-WholeBody's body and feet: one point, or the person's own left and
 * right of a pair. Which side of the picture a pair lands on follows the
 * torso, so an arm or leg crossing over keeps the joints of its own chain.
 */
const JOINTS: ReadonlyArray<
  readonly [Anime25DJointName, number] | readonly [string, number, number, 'upper' | 'lower']
> = [
  ['nose', 0],
  ['eye', 1, 2, 'upper'],
  ['ear', 3, 4, 'upper'],
  ['shoulder', 5, 6, 'upper'],
  ['elbow', 7, 8, 'upper'],
  ['wrist', 9, 10, 'upper'],
  ['hip', 11, 12, 'lower'],
  ['knee', 13, 14, 'lower'],
  ['ankle', 15, 16, 'lower'],
  ['bigToe', 17, 20, 'lower'],
  ['smallToe', 18, 21, 'lower'],
  ['heel', 19, 22, 'lower'],
]

/**
 * Pairs that show which way a half of the body faces, most telling first: the
 * shoulders for the arms and head, the hips for the legs, each standing in
 * for the other when it is not seen.
 */
const FACING = {
  upper: [[5, 6], [11, 12], [1, 2], [3, 4]],
  lower: [[11, 12], [5, 6]],
} as const

/**
 * The PSD is the portrait padded to a square and scaled: its keypoints take
 * the same padding and scale. A PSD that is not square was not split from
 * this portrait, so it has no skeleton.
 */
export function alignSkeletonToPsd(
  skeleton: Readonly<DetectedSkeleton>,
  width: number,
  height: number,
): PsdSkeleton | null {
  if (width !== height || !(skeleton.width > 0 && skeleton.height > 0)) return null
  const edge = Math.max(skeleton.width, skeleton.height)
  const paddingX = Math.floor((edge - skeleton.width) / 2)
  const paddingY = Math.floor((edge - skeleton.height) / 2)
  const scaleX = width / edge
  const scaleY = height / edge
  return {
    model: skeleton.model,
    keypoints: skeleton.keypoints.map(([x, y, score]) => [
      (x + paddingX) * scaleX,
      (y + paddingY) * scaleY,
      score,
    ]),
  }
}

/**
 * The joints seen clearly inside the content frame, in frame pixels and named
 * by side of the picture, as playback carries them.
 */
export function skeletonInFrame(
  skeleton: Readonly<PsdSkeleton>,
  frame: { x: number; y: number; width: number; height: number },
): Anime25DSkeleton | null {
  const joints: Anime25DSkeleton['joints'] = {}
  const sure = (index: number) => {
    const point = skeleton.keypoints[index]
    return point && point[2] >= MIN_JOINT_SCORE ? point : null
  }
  const seen = (index: number) => {
    const point = sure(index)
    if (!point) return null
    const [x, y, score] = point
    const fx = x - frame.x
    const fy = y - frame.y
    if (fx < 0 || fy < 0 || fx > frame.width || fy > frame.height) return null
    return { x: fx, y: fy, score }
  }
  // True when the person's left is on the left of the picture: seen from behind.
  const facing = (half: 'upper' | 'lower') => {
    for (const [left, right] of FACING[half]) {
      const a = sure(left)
      const b = sure(right)
      if (a && b && a[0] !== b[0]) return a[0] < b[0]
    }
    return null
  }
  const leftOnLeft = { upper: facing('upper'), lower: facing('lower') }
  for (const joint of JOINTS) {
    if (joint.length === 2) {
      const point = seen(joint[1])
      if (point) joints[joint[0]] = point
      continue
    }
    const [name, first, second, half] = joint
    const a = seen(first)
    const b = seen(second)
    const onLeft = leftOnLeft[half]
    if (onLeft === null) {
      // No torso to go by: a pair seen together is told apart by position.
      if (a && b) {
        joints[`${name}L` as Anime25DJointName] = a.x <= b.x ? a : b
        joints[`${name}R` as Anime25DJointName] = a.x <= b.x ? b : a
      }
      continue
    }
    const [left, right] = onLeft ? [a, b] : [b, a]
    if (left) joints[`${name}L` as Anime25DJointName] = left
    if (right) joints[`${name}R` as Anime25DJointName] = right
  }
  return Object.keys(joints).length > 0 ? { model: skeleton.model, joints } : null
}
