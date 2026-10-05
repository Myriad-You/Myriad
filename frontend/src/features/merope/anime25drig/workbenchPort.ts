import type { Anime25DDriver } from './driver'
import type { Anime25DDebugSnapshot } from './player'
import type { PoseCorrection } from './poseCorrections'
import type { PoseCorrectionSurface } from './poseCorrectionStage'

/** A correctable point under the pointer: its surface and rest place in head radii. */
export interface PoseCorrectionStagePick {
  surface: PoseCorrectionSurface
  x: number
  y: number
}

/**
 * A patch on screen: where its spot is drawn without it, where it pushes the
 * spot, and how long one head radius is there.
 */
export interface PoseCorrectionStagePoint {
  originX: number
  originY: number
  pushedX: number
  pushedY: number
  /** The share of the correction this pose applies; the push shows at it. */
  weight: number
  unitX: number
  unitY: number
}

/** Anime2.5D-only authoring and diagnostics */
export interface Anime25DWorkbenchPort {
  previewPoseCorrections: (corrections: readonly PoseCorrection[] | null) => void
  pickPoseCorrectionPoint: (clientX: number, clientY: number) => PoseCorrectionStagePick | null
  projectPoseCorrectionPatch: (correction: Pick<PoseCorrection, 'surface' | 'at'>, patch: Readonly<PoseCorrection['patches'][number]>) => PoseCorrectionStagePoint | null
  poseCorrectionDelta: (surface: PoseCorrectionSurface, clientDx: number, clientDy: number) => { dx: number; dy: number } | null
  /** The part of the stage on show, on screen. */
  poseCorrectionStageRect: () => DOMRect | null
  /** Zoom the stage onto the head while fixing poses; a large head stays as is. */
  zoomPoseCorrectionHead: (on: boolean) => void
  setDriver: (partial: Partial<Anime25DDriver>) => void
  replaceDriver: (driver: Anime25DDriver) => void
  blinkNow: () => void
  debugSnapshot: () => Anime25DDebugSnapshot | null
}
