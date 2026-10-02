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
 * COCO-WholeBody's body and feet, as pairs for the person's left and right,
 * or one point. Which of a pair is on the left of the picture is decided by
 * where it was found, so a figure turned away still gets its sides right.
 */
const JOINTS: ReadonlyArray<
  readonly [Anime25DJointName, number] | readonly [string, number, number]
> = [
  ['nose', 0],
  ['eye', 1, 2],
  ['ear', 3, 4],
  ['shoulder', 5, 6],
  ['elbow', 7, 8],
  ['wrist', 9, 10],
  ['hip', 11, 12],
  ['knee', 13, 14],
  ['ankle', 15, 16],
  ['bigToe', 17, 20],
  ['smallToe', 18, 21],
  ['heel', 19, 22],
]

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
  const seen = (index: number) => {
    const point = skeleton.keypoints[index]
    if (!point) return null
    const [x, y, score] = point
    const fx = x - frame.x
    const fy = y - frame.y
    if (!(score >= MIN_JOINT_SCORE)) return null
    if (fx < 0 || fy < 0 || fx > frame.width || fy > frame.height) return null
    return { x: fx, y: fy, score }
  }
  for (const joint of JOINTS) {
    if (joint.length === 2) {
      const point = seen(joint[1])
      if (point) joints[joint[0]] = point
      continue
    }
    const [name, first, second] = joint
    const a = seen(first)
    const b = seen(second)
    const [left, right] = a && b ? (a.x <= b.x ? [a, b] : [b, a]) : [null, null]
    if (left && right) {
      joints[`${name}L` as Anime25DJointName] = left
      joints[`${name}R` as Anime25DJointName] = right
    }
  }
  return Object.keys(joints).length > 0 ? { model: skeleton.model, joints } : null
}
