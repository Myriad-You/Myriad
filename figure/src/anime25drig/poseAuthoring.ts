import type { Anime25DDriver } from './driver'
import type { PoseCorrection } from './poseCorrections'
import type { Anime25DPlayback } from './types'
import { WORKBENCH_DRIVER } from './driver'
import { isPoseCorrections, poseCorrectionCornerKey } from './poseCorrections'

export const POSE_CORRECTION_REGIONS = ['leftEye', 'rightEye', 'mouth', 'chin', 'frontCrown', 'rearCrown'] as const
export type PoseCorrectionRegion = typeof POSE_CORRECTION_REGIONS[number]

export function appendPoseCorrectionPatch(current: readonly PoseCorrection[], candidate: PoseCorrection): { corrections: PoseCorrection[]; index: number; patch: number } | null {
  const corrections = structuredClone(current) as PoseCorrection[]
  let index = corrections.findIndex(c => poseCorrectionCornerKey(c) === poseCorrectionCornerKey(candidate))
  if (index < 0) { index = corrections.length; corrections.push(structuredClone(candidate)) }
  else { corrections[index].patches.push(...structuredClone(candidate.patches))
}
  return isPoseCorrections(corrections) ? { corrections, index, patch: corrections[index].patches.length - 1 } : null
}

export function poseCorrectionPreviewDriver(correction: PoseCorrection, current: Anime25DDriver = WORKBENCH_DRIVER): Anime25DDriver {
  const { eyeCloseL, eyeCloseR, ...angles } = correction.at
  return { ...current, ...angles,
    ...(eyeCloseL !== undefined ? { eyeOpenL: 1 - eyeCloseL } : {}),
    ...(eyeCloseR !== undefined ? { eyeOpenR: 1 - eyeCloseR } : {}),
    angleZ: 0, idle: false, blink: false, rand: false, talk: false, mouse: false, phys: false,
  }
}

interface Point { x: number; y: number }
type Surface = PoseCorrection['surface']

/** Where a named region sits on this asset, in head radii, if the asset has it. */
export function poseCorrectionRegionPoint(playback: Anime25DPlayback, region: PoseCorrectionRegion): (Point & { surface: Surface }) | null {
  const head = playback.shellProfile.head
  const anchors = playback.anchors
  const eye = region === 'leftEye' ? anchors.eyeL : region === 'rightEye' ? anchors.eyeR : undefined
  if ((region === 'leftEye' || region === 'rightEye') && !eye) return null
  const crown = region === 'frontCrown' || region === 'rearCrown'
  const surface: Surface = crown ? region === 'frontCrown' ? 'front-hair' : 'back-hair' : 'head'
  if (!playback.layers.some(l => crown ? l.role === surface : l.role === 'face')) return null
  const x = eye?.icx ?? (region === 'mouth' ? anchors.mouth.cx : head.centerX)
  const y = eye?.closeY ?? (region === 'mouth' ? anchors.mouth.cy : crown ? head.centerY - head.radiusY * 0.85 : anchors.face.y1 - head.radiusY * 0.08)
  return { surface, x: (x - head.centerX) / head.radiusX, y: (y - head.centerY) / head.radiusY }
}

/** The named region a patch sits on, when one is close enough to name it by. */
export function nearestPoseCorrectionRegion(playback: Anime25DPlayback, surface: Surface, point: Point): PoseCorrectionRegion | null {
  let best: PoseCorrectionRegion | null = null
  let distance = 0.45
  for (const region of POSE_CORRECTION_REGIONS) {
    const anchor = poseCorrectionRegionPoint(playback, region)
    if (!anchor || anchor.surface !== surface) continue
    const next = Math.hypot(anchor.x - point.x, anchor.y - point.y)
    if (next < distance) {
      distance = next
      best = region
    }
  }
  return best
}

const PATCH_REACH: Record<PoseCorrectionRegion | 'other', [number, number]> = {
  leftEye: [0.3, 0.22],
  rightEye: [0.3, 0.22],
  mouth: [0.3, 0.2],
  chin: [0.45, 0.3],
  frontCrown: [0.7, 0.55],
  rearCrown: [0.7, 0.55],
  other: [0.35, 0.3],
}

/** A still patch at a point, its reach sized to the feature it sits on. */
export function newPoseCorrectionPatch(playback: Anime25DPlayback, surface: Surface, point: Point): PoseCorrection['patches'][number] {
  const [radiusX, radiusY] = PATCH_REACH[nearestPoseCorrectionRegion(playback, surface, point) ?? 'other']
  const clamp = (v: number) => Math.max(-2, Math.min(2, Math.round(v * 1000) / 1000))
  return { x: clamp(point.x), y: clamp(point.y), radiusX, radiusY, dx: 0, dy: 0 }
}

export type PoseCorrectionProblem = 'needsTurn' | 'needsSecond'

/**
 * The pose a correction made here keys on: the head turn and nod held now,
 * plus the eye or mouth the patch sits on when that is closed or open. A
 * hair patch never waits for a blink.
 */
export function poseCorrectionCorner(
  playback: Anime25DPlayback,
  driver: Anime25DDriver,
  surface: Surface,
  point: Point,
): { at: PoseCorrection['at'] } | { problem: PoseCorrectionProblem } {
  const round = (v: number) => Math.round(v * 100) / 100
  const at: PoseCorrection['at'] = {}
  for (const axis of ['angleX', 'angleY'] as const) {
    if (Math.abs(driver[axis]) >= 0.05) at[axis] = round(driver[axis])
  }
  if (!Object.keys(at).length) return { problem: 'needsTurn' }
  const region = surface === 'head' ? nearestPoseCorrectionRegion(playback, surface, point) : null
  if (region === 'leftEye' && 1 - driver.eyeOpenL >= 0.05) at.eyeCloseL = round(1 - driver.eyeOpenL)
  if (region === 'rightEye' && 1 - driver.eyeOpenR >= 0.05) at.eyeCloseR = round(1 - driver.eyeOpenR)
  if (region === 'mouth' && driver.mouthOpen >= 0.05) at.mouthOpen = round(driver.mouthOpen)
  return Object.keys(at).length < 2 ? { problem: 'needsSecond' } : { at }
}

/** The pose part way from facing front to a correction's own pose. */
export function poseCorrectionTransitionDriver(correction: PoseCorrection, current: Anime25DDriver, t: number): Anime25DDriver {
  const at: PoseCorrection['at'] = {}
  for (const [axis, value] of Object.entries(correction.at) as Array<[keyof PoseCorrection['at'], number]>) at[axis] = value * t
  return poseCorrectionPreviewDriver({ ...correction, at }, current)
}

/** Patch bounds the package accepts. */
export function clampPoseCorrectionPatch(patch: PoseCorrection['patches'][number]): PoseCorrection['patches'][number] {
  const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, Math.round(v * 1000) / 1000))
  return {
    x: clamp(patch.x, -2, 2),
    y: clamp(patch.y, -2, 2),
    radiusX: clamp(patch.radiusX, 0.1, 2),
    radiusY: clamp(patch.radiusY, 0.1, 2),
    dx: clamp(patch.dx, -0.25, 0.25),
    dy: clamp(patch.dy, -0.25, 0.25),
  }
}

/** Patches added, removed or changed since the saved package. */
export function countPoseCorrectionChanges(baseline: readonly PoseCorrection[], draft: readonly PoseCorrection[]): number {
  const corners = (list: readonly PoseCorrection[]) => new Map(list.map(c => [poseCorrectionCornerKey(c), c]))
  const before = corners(baseline)
  const after = corners(draft)
  let changed = 0
  for (const key of new Set([...before.keys(), ...after.keys()])) {
    const a = before.get(key)
    const b = after.get(key)
    if (JSON.stringify(a?.at) !== JSON.stringify(b?.at) && a && b) changed += 1
    const length = Math.max(a?.patches.length ?? 0, b?.patches.length ?? 0)
    for (let i = 0; i < length; i++) {
      if (JSON.stringify(a?.patches[i]) !== JSON.stringify(b?.patches[i])) changed += 1
    }
  }
  return changed
}
